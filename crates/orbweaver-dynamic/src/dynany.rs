//! `DynAny`: reading and changing a value whose type is only known at run time.
//!
//! [`encode`](crate::encode) and [`decode`](crate::decode) move a whole value
//! across the wire in one step. `DynAny` is the other half of §4.4: walking
//! *into* a value that arrived without a static type, looking at one component
//! at a time, and changing one of them without rebuilding the rest. It is what
//! an agent holding an `any` needs, and what `docs/COMPONENTS.md` recorded as
//! this crate's remaining gap.
//!
//! # The cursor is a path, not an index
//!
//! CORBA's `DynAny` hands out a *reference* to a component, which the caller
//! then navigates independently. That is the interface's classic hazard: the
//! component object holds an index into a value that can be reshaped
//! underneath it — shrink the sequence it indexed into, change the
//! discriminator that selected it, and the reference still answers, about
//! something else.
//!
//! Here there is exactly one navigable object and its cursor is the **path
//! from the root to the focused node**. Every operation re-resolves that path
//! against the value as it is *now*, so a component cannot outlive the value
//! it indexed into: an index that no longer names anything is an [`Error`]
//! carrying the path, never a different component's value. Two structural
//! consequences are worth stating because they are what makes the guarantee
//! cheap rather than careful:
//!
//! - Nothing exists below the focus, so a mutation at the focus can never
//!   orphan a deeper index — there is no deeper index to orphan.
//! - [`DynAny::seek`] and [`DynAny::next`] may leave the cursor past the end,
//!   exactly as CORBA's do. Past-the-end is representable and never readable:
//!   [`DynAny::current_value`], [`DynAny::enter`] and [`DynAny::set`] all
//!   refuse it by naming the path and the count they were measured against.
//!
//! *커서는 인덱스가 아니라 루트에서 현재 노드까지의 경로다. 모든 연산이 그
//! 경로를 지금의 값에 대해 다시 해석하므로, 가리키던 값보다 오래 남은 인덱스가
//! 다른 값을 대신 답하는 일은 일어나지 않는다.*
//!
//! # Mutation is refused, not repaired
//!
//! [`DynAny::set`] type-checks against the focused node's `TypeCode` before it
//! writes anything, through the same encoder the wire uses — so a `short`
//! member cannot be given a `long`, and the diagnostic names the full path the
//! way every other error in this crate does. The two shape-changing operations
//! are separate on purpose:
//!
//! - A union's discriminator is **not** settable through `set`. Writing it
//!   alone would leave the previously selected branch's value beside a
//!   discriminator that no longer selects it — a value that cannot encode.
//!   [`DynAny::set_discriminator`] is called on the union instead, and it
//!   rebuilds the branch. A discriminator that selects no branch at all is
//!   refused there, which is the "branch the discriminator does not select"
//!   case caught at the point of the call.
//! - A sequence's length is [`DynAny::set_length`]. An array's is part of its
//!   type and the call says so rather than silently resizing.
//!
//! # Why the types are cloned
//!
//! A focus owns its `TypeCode` rather than borrowing one out of the root. It
//! has to: the type of a value inside an `any` lives in the **value**, not in
//! the containing type, so borrowing the type would borrow the value, and
//! borrowing the value would forbid mutating it. Cloning a `TypeCode` per
//! navigation step is the price of being able to navigate into an `any` at
//! all, which is the one place a dynamic interface is not optional.

use orbweaver_cdr::{Decoder, Endian};
use orbweaver_giop::typecode::TypeCode;

use crate::{Error, MAX_NESTING, Result, Value, check_within, describe, resolved, type_id_of};

/// What a component is called inside its parent.
///
/// Rendered exactly as the marshalling errors render it, so a diagnostic from
/// [`DynAny::set`] and one from [`encode`](crate::encode) name the same place
/// the same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    /// A struct or exception member, or the selected branch of a union.
    Member(String),
    /// An element of a sequence or an array.
    Index(usize),
    /// A union's discriminator, written `_d` as §4.5 writes it.
    Discriminator,
    /// The value inside an `any`, written `_v` as §4.5 writes it.
    Contained,
}

fn push_label(out: &mut String, label: &Label) {
    match label {
        Label::Index(i) => out.push_str(&format!("[{i}]")),
        Label::Member(name) => {
            if !out.is_empty() {
                out.push('.');
            }
            out.push_str(name);
        }
        Label::Discriminator | Label::Contained => {
            if !out.is_empty() {
                out.push('.');
            }
            out.push_str(if *label == Label::Discriminator { "_d" } else { "_v" });
        }
    }
}

/// Where the cursor is standing, resolved against the value as it is now.
struct Focus {
    /// The focused node's type, with aliases and recursion markers followed.
    tc: TypeCode,
    /// The constructed types the focus is inside, outermost first.
    ///
    /// Handed to [`check_within`] so that a [`TypeCode::Recursive`] marker
    /// below the focus resolves against the same enclosing types the encoder
    /// would have been standing on had it walked here itself.
    open: Vec<TypeCode>,
    /// The rendered path, e.g. `order.lines[2].quantity`.
    rendered: String,
    /// The focus's own label, or `None` at the root.
    label: Option<Label>,
}

impl Focus {
    fn open_refs(&self) -> Vec<&TypeCode> {
        self.open.iter().collect()
    }
}

/// Follows aliases and recursion markers to the type that actually has
/// components, recording every link it passes so a marker below can resolve.
///
/// The alias links go onto `open` because that is what the encoder does: the
/// registry's marker for `typedef sequence<Tree> TreeSeq; struct Tree {
/// TreeSeq kids; }` names `TreeSeq`, not `Tree`, so an alias that is not
/// recorded is a cycle that cannot be closed.
fn open_type(mut t: TypeCode, open: &mut Vec<TypeCode>, path: &str) -> Result<TypeCode> {
    enum Step {
        Alias(TypeCode),
        Marker(TypeCode),
    }
    loop {
        if open.len() > MAX_NESTING {
            return Err(Error {
                path: path.to_string(),
                message: format!(
                    "this type nests deeper than {MAX_NESTING} levels; refusing to follow it"
                ),
            });
        }
        let step = match &t {
            TypeCode::Alias { aliased, .. } => Step::Alias((**aliased).clone()),
            TypeCode::Recursive(id) => {
                let target = open
                    .iter()
                    .rev()
                    .find(|o| type_id_of(o) == Some(id.as_str()))
                    .cloned()
                    .ok_or_else(|| Error {
                        path: path.to_string(),
                        message: format!(
                            "recursive type {id} is not inside the type it names, so the cycle \
                             cannot be resolved; navigate from the whole type rather than the \
                             fragment"
                        ),
                    })?;
                Step::Marker(target)
            }
            _ => return Ok(t),
        };
        match step {
            Step::Alias(next) => {
                open.push(t);
                t = next;
            }
            Step::Marker(next) => t = next,
        }
    }
}

/// How many components the value at this node has.
///
/// Counted from the **value**, never from the type: a union has one component
/// when no branch is active and two when one is, and the value is the only
/// thing that knows which. Counting from the type is how a navigator starts
/// disagreeing with what it will later encode.
fn component_count_of(v: &Value) -> usize {
    match v {
        Value::Struct(members) => members.len(),
        Value::Union { value, .. } => 1 + usize::from(value.is_some()),
        Value::List(items) => items.len(),
        Value::Any(..) => 1,
        _ => 0,
    }
}

fn child_value(v: &Value, i: usize) -> Option<&Value> {
    match v {
        Value::Struct(members) => members.get(i).map(|(_, v)| v),
        Value::Union { discriminator, value } => match i {
            0 => Some(discriminator),
            1 => value.as_deref(),
            _ => None,
        },
        Value::List(items) => items.get(i),
        Value::Any(_, inner) => (i == 0).then_some(&**inner),
        _ => None,
    }
}

fn child_value_mut(v: &mut Value, i: usize) -> Option<&mut Value> {
    match v {
        Value::Struct(members) => members.get_mut(i).map(|(_, v)| v),
        Value::Union { discriminator, value } => match i {
            0 => Some(discriminator),
            1 => value.as_deref_mut(),
            _ => None,
        },
        Value::List(items) => items.get_mut(i),
        Value::Any(_, inner) => (i == 0).then_some(&mut **inner),
        _ => None,
    }
}

/// The label and declared type of component `i` of `node`.
fn child_type(node: &TypeCode, v: &Value, i: usize, path: &str) -> Result<(Label, TypeCode)> {
    let found = match node {
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
            members.get(i).map(|m| (Label::Member(m.name.clone()), m.tc.clone()))
        }
        TypeCode::Union { discriminator, cases, default_index, name, .. } => match i {
            0 => Some((Label::Discriminator, (**discriminator).clone())),
            1 => {
                let Value::Union { discriminator: d, .. } = v else {
                    return Err(Error {
                        path: path.to_string(),
                        message: format!("{} is a union but the value is not", describe(node)),
                    });
                };
                crate::select_case_public(discriminator, cases, *default_index, d, name)?
                    .map(|c| (Label::Member(c.name.clone()), c.tc.clone()))
            }
            _ => None,
        },
        TypeCode::Sequence { element, .. } | TypeCode::Array { element, .. } => {
            Some((Label::Index(i), (**element).clone()))
        }
        TypeCode::Any => match v {
            Value::Any(inner_tc, _) => (i == 0).then(|| (Label::Contained, (**inner_tc).clone())),
            _ => None,
        },
        _ => None,
    };
    found.ok_or_else(|| Error {
        path: path.to_string(),
        message: format!("{} has no component {i}", describe(node)),
    })
}

fn value_at<'v>(root: &'v Value, cursor: &[usize]) -> Option<&'v Value> {
    let mut v = root;
    for &i in cursor {
        v = child_value(v, i)?;
    }
    Some(v)
}

fn value_at_mut<'v>(root: &'v mut Value, cursor: &[usize]) -> Option<&'v mut Value> {
    let mut v = root;
    for &i in cursor {
        v = child_value_mut(v, i)?;
    }
    Some(v)
}

/// Resolves `cursor` against the value as it is now.
fn locate(root_tc: &TypeCode, root_v: &Value, cursor: &[usize]) -> Result<Focus> {
    let mut tc = root_tc.clone();
    let mut v = root_v;
    let mut open: Vec<TypeCode> = Vec::new();
    let mut rendered = String::new();
    let mut label = None;

    for &i in cursor {
        let node = open_type(tc, &mut open, &rendered)?;
        let n = component_count_of(v);
        if i >= n {
            return Err(Error {
                path: rendered.clone(),
                message: format!(
                    "there is no component {i} here: this {} has {n} component(s), so the \
                     cursor names nothing",
                    describe(&node)
                ),
            });
        }
        let (l, child_tc) = child_type(&node, v, i, &rendered)?;
        let child = child_value(v, i).ok_or_else(|| Error {
            path: rendered.clone(),
            message: format!(
                "component {i} is declared by {} but absent from the value",
                describe(&node)
            ),
        })?;
        open.push(node);
        push_label(&mut rendered, &l);
        label = Some(l);
        tc = child_tc;
        v = child;
    }
    let tc = open_type(tc, &mut open, &rendered)?;
    Ok(Focus { tc, open, rendered, label })
}

/// Re-roots a diagnostic produced at the focus onto the full cursor path.
fn under(prefix: &str, e: Error) -> Error {
    if prefix.is_empty() {
        return e;
    }
    let path = if e.path.is_empty() {
        prefix.to_string()
    } else if e.path.starts_with('[') {
        format!("{prefix}{}", e.path)
    } else {
        format!("{prefix}.{}", e.path)
    };
    Error { path, message: e.message }
}

/// The value a type starts at: zeroes, the first enumerator, an empty
/// sequence, and for a union the discriminator that selects one of its
/// branches.
///
/// Not a wire concept — CDR has no defaults — but a navigator that can only
/// read values somebody else built is half an interface. This is what
/// [`DynAny::empty`] starts from, what [`DynAny::set_length`] grows a sequence
/// with, and what [`DynAny::set_discriminator`] rebuilds a branch with.
pub fn default_value(tc: &TypeCode) -> Result<Value> {
    default_within(tc, &mut Vec::new(), "")
}

fn default_within(tc: &TypeCode, open: &mut Vec<TypeCode>, path: &str) -> Result<Value> {
    let node = open_type(tc.clone(), open, path)?;
    let unsupported = |what: &str| Error {
        path: path.to_string(),
        message: format!(
            "{what} has no value in the dynamic path, so there is nothing to start it at; see \
             docs/PLAN.md §4.4"
        ),
    };
    Ok(match &node {
        TypeCode::Null | TypeCode::Void => Value::Struct(Vec::new()),
        TypeCode::Boolean => Value::Bool(false),
        TypeCode::Octet => Value::Octet(0),
        TypeCode::Char => Value::Char(0),
        TypeCode::WChar => Value::WChar('\0'),
        TypeCode::Short => Value::Short(0),
        TypeCode::UShort => Value::UShort(0),
        TypeCode::Long => Value::Long(0),
        TypeCode::ULong => Value::ULong(0),
        TypeCode::LongLong => Value::LongLong(0),
        TypeCode::ULongLong => Value::ULongLong(0),
        TypeCode::Float => Value::Float(0.0),
        TypeCode::Double => Value::Double(0.0),
        TypeCode::LongDouble => Value::LongDouble([0; 16]),
        TypeCode::String(_) => Value::String(String::new()),
        TypeCode::WString(_) => Value::WString(String::new()),
        TypeCode::ObjRef { .. } => Value::ObjRef(None),
        // tk_null is the one TypeCode that always encodes and describes
        // nothing, which is exactly what an `any` nobody has filled in holds.
        TypeCode::Any => Value::Any(Box::new(TypeCode::Null), Box::new(Value::Struct(Vec::new()))),
        TypeCode::TypeCode => Value::TypeCode(Box::new(TypeCode::Null)),
        TypeCode::Enum { members, name, .. } => match members.first() {
            Some(m) => Value::Enum(m.clone()),
            None => {
                return Err(Error {
                    path: path.to_string(),
                    message: format!("enum {name} has no enumerators, so it has no value"),
                });
            }
        },
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
            open.push(node.clone());
            let mut out = Vec::with_capacity(members.len());
            for m in members {
                let mut sub = String::from(path);
                push_label(&mut sub, &Label::Member(m.name.clone()));
                out.push((m.name.clone(), default_within(&m.tc, open, &sub)?));
            }
            open.pop();
            Value::Struct(out)
        }
        TypeCode::Sequence { .. } => Value::List(Vec::new()),
        TypeCode::Array { element, length } => {
            open.push(node.clone());
            let mut out = Vec::with_capacity(*length as usize);
            for i in 0..*length as usize {
                let mut sub = String::from(path);
                push_label(&mut sub, &Label::Index(i));
                out.push(default_within(element, open, &sub)?);
            }
            open.pop();
            Value::List(out)
        }
        TypeCode::Union { discriminator, cases, default_index, name, .. } => {
            // Try each declared label in turn, then the discriminator's own
            // default. The first one that selects a branch wins, so the value
            // that comes back is always one `select_case` agrees with — a
            // default built by guessing zero is a union that will not encode.
            let mut candidates: Vec<Value> = cases
                .iter()
                .filter_map(|c| {
                    crate::decode(&mut Decoder::new(&c.label, Endian::Big), discriminator).ok()
                })
                .collect();
            candidates.push(default_within(discriminator, open, path)?);
            for d in candidates {
                let Ok(case) =
                    crate::select_case_public(discriminator, cases, *default_index, &d, name)
                else {
                    continue;
                };
                open.push(node.clone());
                let branch = match case {
                    Some(c) => {
                        let mut sub = String::from(path);
                        push_label(&mut sub, &Label::Member(c.name.clone()));
                        Some(Box::new(default_within(&c.tc, open, &sub)?))
                    }
                    None => None,
                };
                open.pop();
                return Ok(Value::Union { discriminator: Box::new(d), value: branch });
            }
            return Err(Error {
                path: path.to_string(),
                message: format!(
                    "no discriminator value selects a branch of {name}, so it has no starting \
                     value"
                ),
            });
        }
        TypeCode::Fixed { .. } => return Err(unsupported("`fixed`")),
        // Refused, and refused *here* rather than by falling through to the
        // reference case. A valuetype's state goes on the wire inline behind a
        // value tag (CORBA 3.4 Part 2, §9.3.4) and an abstract interface goes
        // as the union of a value and a reference; marshalling either as an
        // IOR is not a partial implementation of §4.4's deferral, it is the
        // wrong bytes. The registry used to record both as `ObjRef` and this
        // path marshalled them without a word.
        TypeCode::Value { .. } => return Err(unsupported("`valuetype`")),
        TypeCode::AbstractInterface { .. } => return Err(unsupported("an abstract interface")),
        // And the fourth. `ObjRef` gave it a perfectly good default — `None`,
        // a nil reference — which is the shape of every wrong answer in this
        // class: legal, silent, and about a different type.
        TypeCode::Native { .. } => return Err(unsupported("a `native`")),
        TypeCode::Principal => return Err(unsupported("`Principal`")),
        // open_type followed both; arriving here would mean it had not.
        TypeCode::Recursive(_) | TypeCode::Alias { .. } => {
            return Err(Error {
                path: path.to_string(),
                message: "an alias or recursion marker survived resolution".into(),
            });
        }
    })
}

/// A value being navigated, and a cursor into it.
///
/// See the module documentation for what the cursor is and why it cannot lie.
#[derive(Debug, Clone, PartialEq)]
pub struct DynAny {
    tc: TypeCode,
    value: Value,
    cursor: Vec<usize>,
}

impl DynAny {
    /// Wraps a value, refusing one its `TypeCode` forbids.
    ///
    /// The check is the encoder's own, so anything this accepts will encode
    /// and anything it refuses would have failed on the wire instead — which
    /// is later, and against a peer.
    pub fn new(tc: TypeCode, value: Value) -> Result<Self> {
        check_within(&tc, &value, &[])?;
        Ok(Self { tc, value, cursor: Vec::new() })
    }

    /// A value of `tc` at its [`default_value`], focused at the root.
    pub fn empty(tc: TypeCode) -> Result<Self> {
        let value = default_value(&tc)?;
        Self::new(tc, value)
    }

    /// The type of the whole value.
    pub fn type_code(&self) -> &TypeCode {
        &self.tc
    }

    /// The whole value.
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// The whole value, consuming the navigator.
    pub fn into_value(self) -> Value {
        self.value
    }

    /// The cursor, as component indices from the root.
    pub fn cursor(&self) -> &[usize] {
        &self.cursor
    }

    /// How far the cursor has descended. Zero at the root.
    pub fn depth(&self) -> usize {
        self.cursor.len()
    }

    /// The cursor rendered the way a diagnostic renders it.
    ///
    /// Never fails: a cursor that no longer resolves renders as far as it got,
    /// which is the part a reader needs in order to see where it stopped.
    pub fn path(&self) -> String {
        match locate(&self.tc, &self.value, &self.cursor) {
            Ok(f) => f.rendered,
            Err(e) => e.path,
        }
    }

    /// The focused node's label, or `None` at the root.
    pub fn current_label(&self) -> Result<Option<Label>> {
        Ok(locate(&self.tc, &self.value, &self.cursor)?.label)
    }

    /// The focused node's type, aliases and recursion markers followed.
    pub fn current_type(&self) -> Result<TypeCode> {
        Ok(locate(&self.tc, &self.value, &self.cursor)?.tc)
    }

    /// The focused node's value.
    pub fn current_value(&self) -> Result<&Value> {
        let f = locate(&self.tc, &self.value, &self.cursor)?;
        value_at(&self.value, &self.cursor).ok_or_else(|| Error {
            path: f.rendered,
            message: "the cursor resolved against the type but not against the value".into(),
        })
    }

    /// How many components the focused node has. Zero means a leaf.
    pub fn component_count(&self) -> Result<usize> {
        Ok(component_count_of(self.current_value()?))
    }

    /// What the focused node's components are called, in order.
    pub fn component_labels(&self) -> Result<Vec<Label>> {
        let f = locate(&self.tc, &self.value, &self.cursor)?;
        let v = self.current_value()?;
        (0..component_count_of(v)).map(|i| Ok(child_type(&f.tc, v, i, &f.rendered)?.0)).collect()
    }

    /// Descends into the focused node's first component.
    ///
    /// CORBA's `current_component` in two halves: this moves, and
    /// [`Self::current_value`] reads. Splitting them is what removes the
    /// component object that could outlive its parent.
    pub fn enter(&mut self) -> Result<()> {
        let f = locate(&self.tc, &self.value, &self.cursor)?;
        if component_count_of(self.current_value()?) == 0 {
            return Err(Error {
                path: f.rendered,
                message: format!("{} has no components to enter", describe(&f.tc)),
            });
        }
        self.cursor.push(0);
        Ok(())
    }

    /// Ascends to the parent. An error at the root, which has none.
    pub fn leave(&mut self) -> Result<()> {
        if self.cursor.pop().is_none() {
            return Err(Error {
                path: String::new(),
                message: "the cursor is already at the root".into(),
            });
        }
        Ok(())
    }

    /// Returns the cursor to the root.
    pub fn reset(&mut self) {
        self.cursor.clear();
    }

    /// Moves to the next sibling, reporting whether one was there.
    ///
    /// `false` leaves the cursor past the end, where reading and writing are
    /// both refused. That is CORBA's shape, and it is safe here because
    /// past-the-end is re-checked at every use rather than remembered.
    // `next` is what the OMG specification calls this operation, and a reader
    // holding the specification is the reader this API is for. It is not an
    // `Iterator`: it returns a Result, it moves a cursor rather than yielding,
    // and the value is read separately.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<bool> {
        let n = self.sibling_count()?;
        let last = self.cursor.len() - 1;
        self.cursor[last] = self.cursor[last].saturating_add(1).min(n);
        Ok(self.cursor[last] < n)
    }

    /// Moves to the first sibling.
    pub fn rewind(&mut self) -> Result<()> {
        let n = self.sibling_count()?;
        let last = self.cursor.len() - 1;
        self.cursor[last] = 0;
        if n == 0 {
            return Err(Error {
                path: self.path(),
                message: "there are no components to rewind to".into(),
            });
        }
        Ok(())
    }

    /// Moves to sibling `i`, reporting whether it exists.
    pub fn seek(&mut self, i: usize) -> Result<bool> {
        let n = self.sibling_count()?;
        let last = self.cursor.len() - 1;
        self.cursor[last] = i;
        Ok(i < n)
    }

    fn sibling_count(&self) -> Result<usize> {
        if self.cursor.is_empty() {
            return Err(Error {
                path: String::new(),
                message: "the root has no siblings; enter() descends into its components".into(),
            });
        }
        let parent = &self.cursor[..self.cursor.len() - 1];
        let f = locate(&self.tc, &self.value, parent)?;
        let v = value_at(&self.value, parent).ok_or_else(|| Error {
            path: f.rendered,
            message: "the cursor resolved against the type but not against the value".into(),
        })?;
        Ok(component_count_of(v))
    }

    /// Replaces the focused node's value, or refuses.
    ///
    /// Refuses a value the focused `TypeCode` forbids, and refuses a union's
    /// discriminator outright — see the module documentation for why that one
    /// is [`Self::set_discriminator`]'s job instead.
    pub fn set(&mut self, v: Value) -> Result<()> {
        let f = locate(&self.tc, &self.value, &self.cursor)?;
        if f.label == Some(Label::Discriminator) {
            return Err(Error {
                path: f.rendered,
                message: "a union's discriminator cannot be set on its own: the branch beside \
                          it would no longer be the one it selects. Move to the union and call \
                          set_discriminator, which rebuilds the branch"
                    .into(),
            });
        }
        check_within(&f.tc, &v, &f.open_refs()).map_err(|e| under(&f.rendered, e))?;
        let slot = value_at_mut(&mut self.value, &self.cursor).ok_or_else(|| Error {
            path: f.rendered,
            message: "the cursor resolved against the type but not against the value".into(),
        })?;
        *slot = v;
        Ok(())
    }

    /// Sets the length of the focused sequence, filling with defaults.
    pub fn set_length(&mut self, n: usize) -> Result<()> {
        let f = locate(&self.tc, &self.value, &self.cursor)?;
        let (element, bound) = match resolved(&f.tc) {
            TypeCode::Sequence { element, bound } => ((**element).clone(), *bound),
            TypeCode::Array { length, .. } => {
                return Err(Error {
                    path: f.rendered,
                    message: format!(
                        "an array's length is part of its type: this one holds {length} \
                         element(s) and always will"
                    ),
                });
            }
            other => {
                return Err(Error {
                    path: f.rendered,
                    message: format!("{} has no length to set", describe(other)),
                });
            }
        };
        if bound > 0 && n > bound as usize {
            return Err(Error {
                path: f.rendered,
                message: format!("sequence is bounded at {bound} but {n} were asked for"),
            });
        }
        let existing = match value_at(&self.value, &self.cursor) {
            Some(Value::List(items)) => items.len(),
            _ => {
                return Err(Error {
                    path: f.rendered,
                    message: "the focused value is not a sequence".into(),
                });
            }
        };
        // Built before the slot is borrowed mutably, and against the sequence's
        // own enclosing types, so that an element of a recursive type resolves
        // its marker the same way encoding it would.
        let mut grown = Vec::new();
        for _ in existing..n {
            let mut open = f.open.clone();
            open.push(f.tc.clone());
            grown.push(default_within(&element, &mut open, &f.rendered)?);
        }
        let Some(Value::List(items)) = value_at_mut(&mut self.value, &self.cursor) else {
            return Err(Error {
                path: f.rendered,
                message: "the focused value is not a sequence".into(),
            });
        };
        items.truncate(n);
        items.extend(grown);
        Ok(())
    }

    /// Sets the focused union's discriminator and rebuilds its branch.
    ///
    /// A discriminator that selects no branch and has no default is refused
    /// here, naming the union, rather than encoding as a discriminator with
    /// nothing after it.
    pub fn set_discriminator(&mut self, d: Value) -> Result<()> {
        let f = locate(&self.tc, &self.value, &self.cursor)?;
        let TypeCode::Union { discriminator, cases, default_index, name, .. } = resolved(&f.tc)
        else {
            return Err(Error {
                path: f.rendered,
                message: format!("{} is not a union and has no discriminator", describe(&f.tc)),
            });
        };
        check_within(discriminator, &d, &f.open_refs()).map_err(|e| {
            let mut prefix = f.rendered.clone();
            push_label(&mut prefix, &Label::Discriminator);
            under(&prefix, e)
        })?;
        let case = crate::select_case_public(discriminator, cases, *default_index, &d, name)
            .map_err(|e| under(&f.rendered, e))?;

        let current_branch = match value_at(&self.value, &self.cursor) {
            Some(Value::Union { discriminator: old, value: Some(v) }) => {
                crate::select_case_public(discriminator, cases, *default_index, old, name)
                    .ok()
                    .flatten()
                    .map(|c| (c.name.clone(), (**v).clone()))
            }
            _ => None,
        };

        let branch = match case {
            Some(c) => Some(Box::new(match current_branch {
                // A discriminator change that lands on the same branch keeps
                // the value: the branch did not change, so there is nothing to
                // discard, and discarding it anyway is data loss dressed up as
                // safety.
                Some((old_name, old_value)) if old_name == c.name => old_value,
                _ => {
                    let mut sub = f.rendered.clone();
                    push_label(&mut sub, &Label::Member(c.name.clone()));
                    let mut open = f.open.clone();
                    open.push(f.tc.clone());
                    default_within(&c.tc, &mut open, &sub)?
                }
            })),
            None => None,
        };
        let slot = value_at_mut(&mut self.value, &self.cursor).ok_or_else(|| Error {
            path: f.rendered,
            message: "the cursor resolved against the type but not against the value".into(),
        })?;
        *slot = Value::Union { discriminator: Box::new(d), value: branch };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_giop::typecode::{Member, UnionCase};

    fn line() -> TypeCode {
        TypeCode::Struct {
            id: "IDL:dyn/Line:1.0".into(),
            name: "Line".into(),
            members: vec![
                Member { name: "sku".into(), tc: TypeCode::String(0) },
                Member { name: "quantity".into(), tc: TypeCode::Short },
            ],
        }
    }

    fn order() -> TypeCode {
        TypeCode::Struct {
            id: "IDL:dyn/Order:1.0".into(),
            name: "Order".into(),
            members: vec![Member {
                name: "lines".into(),
                tc: TypeCode::Sequence { element: Box::new(line()), bound: 0 },
            }],
        }
    }

    /// `union Choice switch (long) { case 1: long n; case 2: string s; }`
    fn choice() -> TypeCode {
        TypeCode::Union {
            id: "IDL:dyn/Choice:1.0".into(),
            name: "Choice".into(),
            discriminator: Box::new(TypeCode::Long),
            default_index: -1,
            cases: vec![
                UnionCase { label: vec![0, 0, 0, 1], name: "n".into(), tc: TypeCode::Long },
                UnionCase { label: vec![0, 0, 0, 2], name: "s".into(), tc: TypeCode::String(0) },
            ],
        }
    }

    fn order_with_two_lines() -> DynAny {
        let mut d = DynAny::empty(order()).expect("empty order");
        d.enter().expect("enter lines");
        d.set_length(2).expect("two lines");
        d.reset();
        d
    }

    #[test]
    fn a_default_value_encodes() {
        for tc in [line(), order(), choice()] {
            let v = default_value(&tc).unwrap_or_else(|e| panic!("{tc:?}: {e}"));
            let mut e = orbweaver_cdr::Encoder::new(Endian::Big);
            crate::encode(&mut e, &tc, &v).unwrap_or_else(|err| panic!("{tc:?}: {err}"));
        }
    }

    #[test]
    fn the_path_names_where_the_cursor_is() {
        let mut d = order_with_two_lines();
        d.enter().unwrap();
        assert_eq!(d.path(), "lines");
        d.enter().unwrap();
        assert!(d.next().unwrap());
        assert_eq!(d.path(), "lines[1]");
        d.enter().unwrap();
        assert!(d.next().unwrap());
        assert_eq!(d.path(), "lines[1].quantity");
        assert_eq!(d.current_label().unwrap(), Some(Label::Member("quantity".into())));
    }

    /// The whole point: an index that no longer names anything answers with a
    /// diagnostic, never with a neighbour's value.
    #[test]
    fn an_index_cannot_outlive_the_value_it_indexed_into() {
        let mut d = order_with_two_lines();
        d.enter().unwrap(); // lines
        d.enter().unwrap(); // lines[0]
        assert!(d.next().unwrap()); // lines[1]
        assert_eq!(d.path(), "lines[1]");
        d.leave().unwrap(); // back to lines, which is the only place a length
        d.set_length(1).unwrap(); // can be changed from — and lines[1] is gone
        d.enter().unwrap(); // lines[0]

        assert!(!d.seek(1).unwrap(), "seek must report that there is no lines[1]");
        let e = d.current_value().expect_err("reading a vanished element must fail");
        assert_eq!(e.path, "lines");
        assert!(e.message.contains("no component 1"), "{}", e.message);
        assert!(d.enter().is_err(), "entering a vanished element must fail");
    }

    #[test]
    fn seeking_past_the_end_is_representable_and_not_readable() {
        let mut d = order_with_two_lines();
        d.enter().unwrap();
        d.enter().unwrap();
        assert!(!d.seek(7).unwrap());
        assert!(d.current_value().is_err());
        assert!(d.rewind().is_ok());
        assert_eq!(d.path(), "lines[0]");
    }

    #[test]
    fn next_stops_at_the_end_and_stays_there() {
        let mut d = order_with_two_lines();
        d.enter().unwrap();
        d.enter().unwrap();
        assert!(d.next().unwrap());
        assert!(!d.next().unwrap());
        assert!(!d.next().unwrap(), "past-the-end does not wrap");
    }

    #[test]
    fn a_leaf_has_nothing_to_enter() {
        let mut d = DynAny::empty(line()).unwrap();
        d.enter().unwrap();
        let e = d.enter().expect_err("a string is a leaf");
        assert_eq!(e.path, "sku");
    }

    #[test]
    fn setting_a_member_to_the_wrong_width_is_refused_with_its_path() {
        let mut d = order_with_two_lines();
        d.enter().unwrap();
        d.enter().unwrap();
        assert!(d.next().unwrap());
        d.enter().unwrap();
        assert!(d.next().unwrap()); // lines[1].quantity, a short
        let e = d.set(Value::Long(1)).expect_err("a long is not a short");
        assert_eq!(e.path, "lines[1].quantity");
        assert!(e.message.contains("expected a value of type short"), "{}", e.message);
        d.set(Value::Short(3)).expect("a short is");
    }

    #[test]
    fn a_discriminator_is_not_settable_on_its_own() {
        let mut d = DynAny::empty(choice()).unwrap();
        d.enter().unwrap();
        assert_eq!(d.current_label().unwrap(), Some(Label::Discriminator));
        let e = d.set(Value::Long(2)).expect_err("the branch beside it would be stale");
        assert!(e.message.contains("set_discriminator"), "{}", e.message);
    }

    #[test]
    fn setting_a_discriminator_rebuilds_the_branch() {
        let mut d = DynAny::empty(choice()).unwrap();
        assert_eq!(d.component_count().unwrap(), 2);
        d.set_discriminator(Value::Long(2)).unwrap();
        d.enter().unwrap();
        assert!(d.next().unwrap());
        assert_eq!(d.current_label().unwrap(), Some(Label::Member("s".into())));
        assert_eq!(d.current_value().unwrap(), &Value::String(String::new()));
        // and the branch it produced is the one the discriminator selects
        d.reset();
        crate::encode(&mut orbweaver_cdr::Encoder::new(Endian::Big), &choice(), d.value())
            .expect("the rebuilt union encodes");
    }

    /// The corpus walk cannot see this: it always writes into a union whose
    /// branch it is about to overwrite anyway. Discarding a value because the
    /// discriminator was rewritten to something that selects the same branch
    /// is data loss with nothing to show for it, so it is pinned here.
    #[test]
    fn a_discriminator_that_reselects_the_same_branch_keeps_the_value() {
        let with_default = TypeCode::Union {
            id: "IDL:dyn/Choice2:1.0".into(),
            name: "Choice2".into(),
            discriminator: Box::new(TypeCode::Long),
            default_index: 1,
            cases: vec![
                UnionCase { label: vec![0, 0, 0, 1], name: "n".into(), tc: TypeCode::Long },
                UnionCase { label: vec![0, 0, 0, 2], name: "other".into(), tc: TypeCode::Long },
            ],
        };
        let mut d = DynAny::empty(with_default).unwrap();
        d.set_discriminator(Value::Long(2)).unwrap();
        d.enter().unwrap();
        assert!(d.next().unwrap());
        d.set(Value::Long(41)).unwrap();
        d.leave().unwrap();

        // 7 matches no label, so the default branch — `other` — is selected
        // again. The branch did not change, so neither should its value.
        d.set_discriminator(Value::Long(7)).unwrap();
        d.enter().unwrap();
        assert!(d.next().unwrap());
        assert_eq!(d.current_value().unwrap(), &Value::Long(41));

        // Landing on a different branch does reset it, because the old value
        // is not a value of the new branch's type.
        d.leave().unwrap();
        d.set_discriminator(Value::Long(1)).unwrap();
        d.enter().unwrap();
        assert!(d.next().unwrap());
        assert_eq!(d.current_label().unwrap(), Some(Label::Member("n".into())));
        assert_eq!(d.current_value().unwrap(), &Value::Long(0));
    }

    #[test]
    fn a_discriminator_that_selects_no_branch_is_refused() {
        let mut d = DynAny::empty(choice()).unwrap();
        let e = d.set_discriminator(Value::Long(9)).expect_err("no case 9 and no default");
        assert!(e.message.contains("no branch of Choice matches"), "{}", e.message);
        // and the union is unchanged, rather than half-written
        assert_eq!(d.component_count().unwrap(), 2);
    }

    #[test]
    fn a_discriminator_of_the_wrong_type_is_refused_at_the_discriminator() {
        let mut d = DynAny::empty(choice()).unwrap();
        let e = d.set_discriminator(Value::Short(1)).expect_err("the discriminator is a long");
        assert_eq!(e.path, "_d");
    }

    #[test]
    fn a_bounded_sequence_will_not_be_grown_past_its_bound() {
        let tc = TypeCode::Sequence { element: Box::new(TypeCode::Long), bound: 2 };
        let mut d = DynAny::empty(tc).unwrap();
        d.set_length(2).expect("two fit");
        let e = d.set_length(3).expect_err("three do not");
        assert!(e.message.contains("bounded at 2"), "{}", e.message);
        assert_eq!(d.component_count().unwrap(), 2, "the refusal changed nothing");
    }

    #[test]
    fn an_arrays_length_is_not_settable() {
        let tc = TypeCode::Array { element: Box::new(TypeCode::Long), length: 3 };
        let mut d = DynAny::empty(tc).unwrap();
        assert_eq!(d.component_count().unwrap(), 3);
        let e = d.set_length(4).expect_err("an array does not resize");
        assert!(e.message.contains("part of its type"), "{}", e.message);
    }

    #[test]
    fn an_any_is_navigable_because_its_type_is_in_its_value() {
        let mut d = DynAny::empty(TypeCode::Any).unwrap();
        d.set(Value::Any(Box::new(line()), Box::new(default_value(&line()).unwrap()))).unwrap();
        assert_eq!(d.component_count().unwrap(), 1);
        d.enter().unwrap();
        assert_eq!(d.current_label().unwrap(), Some(Label::Contained));
        assert_eq!(d.path(), "_v");
        d.enter().unwrap();
        d.set(Value::String("sku-1".into())).unwrap();
        d.reset();
        assert_eq!(
            d.value(),
            &Value::Any(
                Box::new(line()),
                Box::new(Value::Struct(vec![
                    ("sku".into(), Value::String("sku-1".into())),
                    ("quantity".into(), Value::Short(0)),
                ]))
            )
        );
    }

    /// A recursive type is navigable because the marker resolves against the
    /// types the cursor is standing on — the same trick the encoder plays.
    #[test]
    fn a_recursive_type_can_be_grown_through_its_marker() {
        let tree = TypeCode::Struct {
            id: "IDL:dyn/Tree:1.0".into(),
            name: "Tree".into(),
            members: vec![
                Member { name: "label".into(), tc: TypeCode::String(0) },
                Member {
                    name: "kids".into(),
                    tc: TypeCode::Sequence {
                        element: Box::new(TypeCode::Recursive("IDL:dyn/Tree:1.0".into())),
                        bound: 0,
                    },
                },
            ],
        };
        let mut d = DynAny::empty(tree.clone()).unwrap();
        d.enter().unwrap();
        assert!(d.next().unwrap()); // kids
        d.set_length(1).unwrap();
        d.enter().unwrap(); // kids[0], a Tree
        d.enter().unwrap();
        d.set(Value::String("child".into())).unwrap();
        assert_eq!(d.path(), "kids[0].label");
        d.reset();
        let mut e = orbweaver_cdr::Encoder::new(Endian::Little);
        crate::encode(&mut e, &tree, d.value()).expect("the grown tree encodes");
    }
}
