//! S1 — ingest: natural language in, a **structured brief** out.
//!
//! `docs/PLAN.md` §5 names S1 a stage of its own with an intermediate
//! representation as its output. Until this module it was not one: the model
//! read a requirement sentence and wrote IDL in a single prompt, so S1 existed
//! only as the first half of a sentence S2 was told. Two things are lost that
//! way, and both are the reason the plan separates them.
//!
//! - **Nothing is inspectable before IDL exists.** A requirement in Korean
//!   prose becomes a `.idl` file in one step, and a reader who disagrees with
//!   the reading — the wrong entity, a missing constraint, a unit nobody
//!   noticed — can only argue with the output. A [`Brief`] is the place to
//!   disagree: it is JSON, it is small, a human can edit it, and re-running
//!   from S2 picks the edit up.
//! - **Nothing is attributable.** When a one-prompt pipeline produces bad IDL,
//!   "the model misread the requirement" and "the model mis-wrote the IDL" are
//!   the same failure with the same diagnostic. Split, they are two stages with
//!   two gates, and the stage the failure appears in says which.
//!
//! # The brief is not IDL, on purpose
//!
//! A [`Field`]'s `shape` is free text — `"string"`, `"a list of targets"`,
//! `"금액, 원 단위"` — because S1 is reading a requirement, not choosing a wire
//! type. Choosing `unsigned long long` over `double` is a decision with wire
//! consequences and it belongs to S2, which is the stage whose output the gate
//! rejects when it gets it wrong. An S1 that emitted IDL types would be S2
//! wearing a different name.
//!
//! # What S1 already prevents
//!
//! [`check`] runs the project's dominant root cause one stage earlier than it
//! has ever been caught: a parameter named for the entity it carries
//! (`Position position`) is visible in the brief, before any IDL exists, and is
//! reported there. Phase 0's seven failures were all this cause; catching it at
//! S1 is the codify step applied to the *earliest* stage that can see it.
//!
//! **S1은 요구사항을 읽는 단계이지 IDL을 고르는 단계가 아니다.** 브리프는
//! 사람이 고쳐 넣을 수 있는 중간 표현이며, 여기서 이름 충돌을 먼저 잡는다.

use std::collections::{BTreeMap, BTreeSet};

use orbweaver_dynamic::json::Json;

use crate::{Finding, Report, Severity};

/// What S1 is told to produce, quoted into the ingest prompt verbatim.
///
/// Kept beside the schema it describes so the two cannot drift: the test
/// `the_prompt_describes_every_key_the_parser_requires` fails if a key is added
/// to [`Brief::from_json`] and not to this text.
pub const S1_PROMPT: &str = "\
You are reading one natural-language requirement and producing a STRUCTURED
BRIEF for an IDL synthesis stage that runs after you. You are NOT writing IDL.

Output one JSON object and NOTHING else: no markdown fences, no commentary.

The object has exactly these keys:
  \"requirement\"    string  — the requirement text, copied verbatim
  \"summary\"        string  — one sentence naming what this interface is for
  \"entities\"       array   — the data the requirement talks about
  \"operations\"     array   — the calls the requirement asks for
  \"constraints\"    array of strings — rules the requirement states
                              (limits, ordering, units, error conditions)
  \"open_questions\" array of strings — what the requirement does NOT say and
                              you had to guess or leave open

An entity object has exactly: \"name\" (string), \"description\" (string),
\"fields\" (array).
A field object has exactly: \"name\" (string), \"shape\" (string), and
optionally \"unit\" (string) and \"pii\" (\"none\", \"low\" or \"high\").
An operation object has exactly: \"name\" (string), \"entity\" (string or null,
the entity it acts on), \"inputs\" (array of fields), \"outputs\" (array of
fields), \"effect\" (\"read_only\", \"idempotent\", \"destructive\" or
\"unspecified\"), \"errors\" (array of strings), \"authz\" (string or null, the
permission a caller needs).

Rules:
- \"shape\" is the data shape AS THE REQUIREMENT STATES IT, in plain words
  (\"a list of targets\", \"amount in KRW\", \"UTC timestamp\"). Do NOT write IDL
  types; choosing the IDL type is the next stage's job and its mistakes are
  caught by a compiler.
- Identifier clashes are case-insensitive in IDL. A field or parameter may not
  share a name with an entity or an enclosing scope, ignoring case: an entity
  \"Position\" with a field \"position\", or a parameter \"account\" carrying an
  \"Account\", is illegal downstream. Name the field for its role
  (\"location\", \"the_account\", \"source_account\"), not for its type.
- \"authz\" is the permission AS THE REQUIREMENT STATES IT, copied verbatim
  when the requirement names one (\"gate:operate\"). Do not normalise it, do not
  reword it, do not compose a tidier one — a later stage binds this token to the
  contract's own scope by string equality, and a token you improved here is a
  scope no identity provider issues. Where the requirement only implies a
  permission in prose, say what it implies and put the wording in
  \"open_questions\"; where it names none, use null.
- Do not invent operations the requirement does not ask for. Anything you are
  unsure about belongs in \"open_questions\", which is the field a human reads
  first — leaving it empty when the requirement is genuinely ambiguous is the
  one failure this stage cannot detect.
";

/// Whether a token is shaped like an authorization scope (D005 option C).
///
/// # What "scope-shaped" means here, precisely
///
/// Two or more `[a-z][a-z0-9_-]*` segments joined by `:` or `.`, ASCII only,
/// at most [`MAX_SCOPE_LEN`] bytes. `gate:operate`, `bank.transfer.write`,
/// `echo:blob` and `parkinglot.barrier.open` all pass; so does `api.v2`.
///
/// # What it does **not** claim
///
/// It is a *lexical* filter over one convention, not a definition of "scope":
///
/// - **It does not claim to recognise every scope.** `admin` (no separator),
///   `GATE_OPERATE` (upper case), `gate/operate` (a slash), `읽기권한`
///   (non-ASCII) and a permission described only in prose — "운영자만" — are all
///   real ways to state a permission and all invisible to this predicate. A
///   requirement whose permission is stated any of those ways gets no binding
///   at all, which is silence and not a pass.
/// - **It does not claim every token it accepts is a scope.** `config.yaml`,
///   `api.example.com` and `main.rs` are all scope-shaped by this rule. That is
///   why the predicate is never used alone: [`Brief::stated_scopes`] binds a
///   token only when S1 *also* recorded it as an operation's `authz`, so a
///   filename has to have been read as a permission before it can bind
///   anything.
/// - **It says nothing about whether the scope is the right one.** A model that
///   invents a plausible permission for an operation the requirement never
///   mentioned writes a token nobody stated, and nothing here is a check on
///   that (D005, *What none of these options fix*).
pub fn scope_shaped(token: &str) -> bool {
    if token.is_empty() || token.len() > MAX_SCOPE_LEN || !token.is_ascii() {
        return false;
    }
    if !token.contains([':', '.']) {
        return false;
    }
    token.split([':', '.']).all(|segment| {
        let mut chars = segment.chars();
        matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    })
}

/// The longest token [`scope_shaped`] will consider. A scope is an identifier
/// an identity provider issues, not a sentence.
pub const MAX_SCOPE_LEN: usize = 100;

/// Every scope-shaped token in a piece of text, in order, deduplicated.
///
/// A measurement instrument rather than a gate input: it answers "could this
/// requirement bind anything at all", which is how the false-positive rate over
/// `corpus/requirements/` is measured. The binding itself goes through
/// [`Brief::stated_scopes`], which requires S1 to have read the token as a
/// permission — this function requires only that the characters are there.
pub fn scope_tokens(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // Upper case is part of a candidate even though `scope_shaped` refuses it,
    // so `Gate:operate` is rejected whole rather than split into a token the
    // text does not contain.
    let boundary = |c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'));
    for candidate in text.split(boundary) {
        // Sentence punctuation is not part of the token: "…gate:operate." ends
        // a sentence and states a scope.
        let trimmed = candidate.trim_end_matches([':', '.', '-', '_']);
        if scope_shaped(trimmed) && !out.iter().any(|t| t == trimmed) {
            out.push(trimmed.to_owned());
        }
    }
    out
}

/// What an operation does to the state behind it, as the requirement implies
/// it.
///
/// The same vocabulary `ai_effect` uses (§2.2), so S3 has something to carry
/// across rather than something to re-derive — and [`Effect::Unspecified`] is
/// present because "the requirement does not say" is a real answer that a
/// three-valued enum would force S1 to invent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Effect {
    /// Reads and changes nothing.
    ReadOnly,
    /// Changes something; calling twice lands the same state.
    Idempotent,
    /// Changes something, and repetition is not free.
    Destructive,
    /// The requirement does not say.
    #[default]
    Unspecified,
}

impl Effect {
    /// The JSON spelling.
    pub fn label(self) -> &'static str {
        match self {
            Effect::ReadOnly => "read_only",
            Effect::Idempotent => "idempotent",
            Effect::Destructive => "destructive",
            Effect::Unspecified => "unspecified",
        }
    }

    /// Parses the JSON spelling; `None` for anything else.
    pub fn parse(s: &str) -> Option<Effect> {
        match s.trim() {
            "read_only" => Some(Effect::ReadOnly),
            "idempotent" => Some(Effect::Idempotent),
            "destructive" => Some(Effect::Destructive),
            "unspecified" => Some(Effect::Unspecified),
            _ => None,
        }
    }

    /// Whether the effect says the operation changes state.
    pub fn mutates(self) -> bool {
        matches!(self, Effect::Idempotent | Effect::Destructive)
    }
}

/// One piece of data, named and shaped the way the requirement states it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Field {
    /// What the requirement calls it.
    pub name: String,
    /// The data shape in plain words — never an IDL type (see the module docs).
    pub shape: String,
    /// The unit, where the requirement gives one (`KRW`, `metres`, `seconds`).
    pub unit: Option<String>,
    /// `none`, `low` or `high`, where the requirement implies a sensitivity.
    pub pii: Option<String>,
}

/// A thing the requirement talks about, with the data it carries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entity {
    /// The entity name, as a type would be named.
    pub name: String,
    /// One sentence about what it is.
    pub description: String,
    /// Its data.
    pub fields: Vec<Field>,
}

/// A call the requirement asks for, before anyone has chosen a signature.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OperationSketch {
    /// The operation name.
    pub name: String,
    /// The entity it acts on, where there is one.
    pub entity: Option<String>,
    /// What the caller supplies.
    pub inputs: Vec<Field>,
    /// What the caller gets back.
    pub outputs: Vec<Field>,
    /// What it does to state.
    pub effect: Effect,
    /// The failures the requirement names.
    pub errors: Vec<String>,
    /// The permission a caller needs, where the requirement implies one.
    pub authz: Option<String>,
}

/// S1's output: one requirement, read into structure.
///
/// This is the artifact a human inspects and corrects. It round-trips through
/// [`Brief::to_json`] / [`Brief::from_json`] exactly, which is what makes
/// "edit the brief and re-run from S2" a supported operation rather than a
/// hope.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Brief {
    /// The requirement text, verbatim, so the brief is self-contained.
    pub requirement: String,
    /// One sentence naming what the interface is for.
    pub summary: String,
    /// The data the requirement talks about.
    pub entities: Vec<Entity>,
    /// The calls it asks for.
    pub operations: Vec<OperationSketch>,
    /// Rules the requirement states.
    pub constraints: Vec<String>,
    /// What the requirement does not say. **The field a human reads first.**
    pub open_questions: Vec<String>,
}

/// Why a brief could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefError {
    /// Where in the document, as a JSON path (`$.operations[2].effect`).
    pub path: String,
    /// What was wrong there.
    pub message: String,
}

impl std::fmt::Display for BriefError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for BriefError {}

fn err(path: &str, message: impl Into<String>) -> BriefError {
    BriefError { path: path.to_owned(), message: message.into() }
}

/// The object's keys, or an error naming what was found instead.
fn object<'a>(j: &'a Json, path: &str) -> Result<&'a BTreeMap<String, Json>, BriefError> {
    match j {
        Json::Object(m) => Ok(m),
        other => Err(err(path, format!("expected an object, found {}", other.kind()))),
    }
}

fn string(m: &BTreeMap<String, Json>, key: &str, path: &str) -> Result<String, BriefError> {
    match m.get(key) {
        Some(Json::String(s)) => Ok(s.clone()),
        Some(other) => {
            Err(err(&format!("{path}.{key}"), format!("expected a string, found {}", other.kind())))
        }
        None => Err(err(path, format!("missing {key:?}"))),
    }
}

/// A key that may be `null` or absent, and is a string otherwise.
fn optional(
    m: &BTreeMap<String, Json>,
    key: &str,
    path: &str,
) -> Result<Option<String>, BriefError> {
    match m.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(s)) => Ok(Some(s.clone())),
        Some(other) => {
            Err(err(&format!("{path}.{key}"), format!("expected a string, found {}", other.kind())))
        }
    }
}

fn array<'a>(
    m: &'a BTreeMap<String, Json>,
    key: &str,
    path: &str,
) -> Result<&'a [Json], BriefError> {
    match m.get(key) {
        Some(Json::Array(v)) => Ok(v),
        Some(other) => {
            Err(err(&format!("{path}.{key}"), format!("expected an array, found {}", other.kind())))
        }
        None => Err(err(path, format!("missing {key:?}"))),
    }
}

fn strings(m: &BTreeMap<String, Json>, key: &str, path: &str) -> Result<Vec<String>, BriefError> {
    array(m, key, path)?
        .iter()
        .enumerate()
        .map(|(i, j)| match j {
            Json::String(s) => Ok(s.clone()),
            other => Err(err(
                &format!("{path}.{key}[{i}]"),
                format!("expected a string, found {}", other.kind()),
            )),
        })
        .collect()
}

/// Unknown keys are refused rather than ignored.
///
/// The same argument `contract/unknown-annotation` makes about SIDL keys: a key
/// nobody reads is present in the file and absent from every decision, and a
/// model that writes `"questions"` instead of `"open_questions"` has produced a
/// brief whose most important field silently vanished. Refusing turns that into
/// one round with a message.
fn no_unknown_keys(
    m: &BTreeMap<String, Json>,
    allowed: &[&str],
    path: &str,
) -> Result<(), BriefError> {
    match m.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(err(
            path,
            format!("unknown key {k:?}; this object takes exactly {}", allowed.join(", ")),
        )),
        None => Ok(()),
    }
}

fn field_from_json(j: &Json, path: &str) -> Result<Field, BriefError> {
    let m = object(j, path)?;
    no_unknown_keys(m, &["name", "shape", "unit", "pii"], path)?;
    Ok(Field {
        name: string(m, "name", path)?,
        shape: string(m, "shape", path)?,
        unit: optional(m, "unit", path)?,
        pii: optional(m, "pii", path)?,
    })
}

fn fields_from_json(
    m: &BTreeMap<String, Json>,
    key: &str,
    path: &str,
) -> Result<Vec<Field>, BriefError> {
    array(m, key, path)?
        .iter()
        .enumerate()
        .map(|(i, j)| field_from_json(j, &format!("{path}.{key}[{i}]")))
        .collect()
}

fn field_to_json(f: &Field) -> Json {
    let mut m = BTreeMap::from([
        ("name".to_owned(), Json::String(f.name.clone())),
        ("shape".to_owned(), Json::String(f.shape.clone())),
    ]);
    if let Some(u) = &f.unit {
        m.insert("unit".to_owned(), Json::String(u.clone()));
    }
    if let Some(p) = &f.pii {
        m.insert("pii".to_owned(), Json::String(p.clone()));
    }
    Json::Object(m)
}

impl Brief {
    /// The canonical JSON form — the artifact written to disk.
    pub fn to_json(&self) -> Json {
        let fields = |v: &[Field]| Json::Array(v.iter().map(field_to_json).collect());
        let list = |v: &[String]| Json::Array(v.iter().map(|s| Json::String(s.clone())).collect());
        let nullable = |s: &Option<String>| match s {
            Some(v) => Json::String(v.clone()),
            None => Json::Null,
        };
        Json::Object(BTreeMap::from([
            ("requirement".to_owned(), Json::String(self.requirement.clone())),
            ("summary".to_owned(), Json::String(self.summary.clone())),
            (
                "entities".to_owned(),
                Json::Array(
                    self.entities
                        .iter()
                        .map(|e| {
                            Json::Object(BTreeMap::from([
                                ("name".to_owned(), Json::String(e.name.clone())),
                                ("description".to_owned(), Json::String(e.description.clone())),
                                ("fields".to_owned(), fields(&e.fields)),
                            ]))
                        })
                        .collect(),
                ),
            ),
            (
                "operations".to_owned(),
                Json::Array(
                    self.operations
                        .iter()
                        .map(|o| {
                            Json::Object(BTreeMap::from([
                                ("name".to_owned(), Json::String(o.name.clone())),
                                ("entity".to_owned(), nullable(&o.entity)),
                                ("inputs".to_owned(), fields(&o.inputs)),
                                ("outputs".to_owned(), fields(&o.outputs)),
                                ("effect".to_owned(), Json::String(o.effect.label().to_owned())),
                                ("errors".to_owned(), list(&o.errors)),
                                ("authz".to_owned(), nullable(&o.authz)),
                            ]))
                        })
                        .collect(),
                ),
            ),
            ("constraints".to_owned(), list(&self.constraints)),
            ("open_questions".to_owned(), list(&self.open_questions)),
        ]))
    }

    /// Reads the canonical JSON form, strictly.
    pub fn from_json(j: &Json) -> Result<Brief, BriefError> {
        let path = "$";
        let m = object(j, path)?;
        no_unknown_keys(
            m,
            &["requirement", "summary", "entities", "operations", "constraints", "open_questions"],
            path,
        )?;
        let entities = array(m, "entities", path)?
            .iter()
            .enumerate()
            .map(|(i, j)| {
                let p = format!("$.entities[{i}]");
                let e = object(j, &p)?;
                no_unknown_keys(e, &["name", "description", "fields"], &p)?;
                Ok(Entity {
                    name: string(e, "name", &p)?,
                    description: string(e, "description", &p)?,
                    fields: fields_from_json(e, "fields", &p)?,
                })
            })
            .collect::<Result<Vec<Entity>, BriefError>>()?;
        let operations = array(m, "operations", path)?
            .iter()
            .enumerate()
            .map(|(i, j)| {
                let p = format!("$.operations[{i}]");
                let o = object(j, &p)?;
                no_unknown_keys(
                    o,
                    &["name", "entity", "inputs", "outputs", "effect", "errors", "authz"],
                    &p,
                )?;
                let effect_text = string(o, "effect", &p)?;
                let effect = Effect::parse(&effect_text).ok_or_else(|| {
                    err(
                        &format!("{p}.effect"),
                        format!(
                            "{effect_text:?} is not one of read_only, idempotent, destructive, \
                             unspecified"
                        ),
                    )
                })?;
                Ok(OperationSketch {
                    name: string(o, "name", &p)?,
                    entity: optional(o, "entity", &p)?,
                    inputs: fields_from_json(o, "inputs", &p)?,
                    outputs: fields_from_json(o, "outputs", &p)?,
                    effect,
                    errors: strings(o, "errors", &p)?,
                    authz: optional(o, "authz", &p)?,
                })
            })
            .collect::<Result<Vec<OperationSketch>, BriefError>>()?;
        Ok(Brief {
            requirement: string(m, "requirement", path)?,
            summary: string(m, "summary", path)?,
            entities,
            operations,
            constraints: strings(m, "constraints", path)?,
            open_questions: strings(m, "open_questions", path)?,
        })
    }

    /// The artifact form: the canonical JSON, indented.
    ///
    /// The pipeline writes this rather than the compact form for one reason —
    /// the brief exists to be **read and corrected by a human**, and a
    /// requirement's worth of Korean prose on one line is a file nobody edits.
    /// It parses back through [`Brief::parse`] unchanged, which the round-trip
    /// test pins.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        write_pretty(&self.to_json(), 0, &mut out);
        out.push('\n');
        out
    }

    /// Reads a brief from whatever a model printed.
    ///
    /// Markdown fences are stripped before parsing — the same concession
    /// `spikes/gen_claude.sh` already makes for IDL. Nothing else is repaired:
    /// a brief that is not JSON is an S1 failure with a diagnostic, not
    /// something to guess at.
    pub fn parse(text: &str) -> Result<Brief, BriefError> {
        let stripped: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n");
        let json = Json::parse(stripped.trim())
            .map_err(|e| err("$", format!("not JSON: {}", e.message)))?;
        Brief::from_json(&json)
    }

    /// The scopes this brief **binds**: operation name → the literal token.
    ///
    /// D005 option C, and the one artifact the binding is read from. A pair is
    /// included only when all three hold:
    ///
    /// 1. S1 recorded a token in [`OperationSketch::authz`];
    /// 2. the token is [`scope_shaped`];
    /// 3. the token occurs **verbatim** in [`Brief::requirement`].
    ///
    /// Condition 3 is what makes this a binding on a *stated* token rather than
    /// on a model's opinion: a scope S1 composed for an operation the
    /// requirement scopes only in prose is recorded in the brief, read by a
    /// human, handed to S2 — and binds nothing. The brief is the artifact the
    /// binding lives in rather than the requirement text because it is the
    /// artifact a human can correct *before any IDL exists*: deleting a wrong
    /// `authz` here is a recorded decision, and re-extracting from the
    /// requirement on every run would put the token beyond correction.
    ///
    /// Its own limit, stated where it is implemented: if S1 paraphrases the
    /// requirement instead of copying it verbatim as `S1_PROMPT` requires,
    /// condition 3 fails and the binding silently does not apply. That is the
    /// safe direction — no false demand — and it is still a hole.
    pub fn stated_scopes(&self) -> BTreeMap<String, String> {
        self.operations
            .iter()
            .filter_map(|op| {
                let token = op.authz.as_deref()?.trim();
                (scope_shaped(token) && self.requirement.contains(token))
                    .then(|| (op.name.trim().to_owned(), token.to_owned()))
            })
            .collect()
    }

    /// The brief rendered for a prompt: what S2 is handed instead of prose.
    ///
    /// A rendering, never the source of truth. The JSON is what a human edits
    /// and what the workspace stores; this is regenerated from it on every run,
    /// so an edited brief reaches S2 without anyone re-rendering anything.
    pub fn to_prompt(&self) -> String {
        let mut out = String::new();
        out.push_str("REQUIREMENT (verbatim)\n");
        out.push_str(self.requirement.trim());
        out.push_str("\n\nSUMMARY\n");
        out.push_str(self.summary.trim());
        out.push_str("\n\nENTITIES\n");
        if self.entities.is_empty() {
            out.push_str("  (none identified)\n");
        }
        for e in &self.entities {
            out.push_str(&format!("- {} — {}\n", e.name, e.description));
            for f in &e.fields {
                out.push_str(&format!("    {}\n", render_field(f)));
            }
        }
        out.push_str("\nOPERATIONS\n");
        for o in &self.operations {
            out.push_str(&format!("- {} [effect: {}]", o.name, o.effect.label()));
            if let Some(a) = &o.authz {
                out.push_str(&format!(" [authz: {a}]"));
            }
            out.push('\n');
            if let Some(e) = &o.entity {
                out.push_str(&format!("    on: {e}\n"));
            }
            for f in &o.inputs {
                out.push_str(&format!("    in:  {}\n", render_field(f)));
            }
            for f in &o.outputs {
                out.push_str(&format!("    out: {}\n", render_field(f)));
            }
            if !o.errors.is_empty() {
                out.push_str(&format!("    errors: {}\n", o.errors.join(", ")));
            }
        }
        if !self.constraints.is_empty() {
            out.push_str("\nCONSTRAINTS\n");
            for c in &self.constraints {
                out.push_str(&format!("- {c}\n"));
            }
        }
        if !self.open_questions.is_empty() {
            out.push_str("\nOPEN QUESTIONS (S1 could not settle these from the requirement)\n");
            for q in &self.open_questions {
                out.push_str(&format!("- {q}\n"));
            }
        }
        out
    }
}

/// Indented JSON, deterministic, and parseable by [`Json::parse`].
///
/// A local printer rather than a dependency: the compact form
/// `orbweaver-dynamic` emits is the right default for the wire and the wrong
/// one for a file a person edits.
fn write_pretty(j: &Json, indent: usize, out: &mut String) {
    let pad = |n: usize| " ".repeat(n * 2);
    match j {
        Json::Object(m) if !m.is_empty() => {
            out.push_str("{\n");
            for (i, (k, v)) in m.iter().enumerate() {
                out.push_str(&pad(indent + 1));
                out.push_str(&Json::String(k.clone()).to_string());
                out.push_str(": ");
                write_pretty(v, indent + 1, out);
                out.push_str(if i + 1 == m.len() { "\n" } else { ",\n" });
            }
            out.push_str(&pad(indent));
            out.push('}');
        }
        Json::Array(v) if !v.is_empty() => {
            out.push_str("[\n");
            for (i, item) in v.iter().enumerate() {
                out.push_str(&pad(indent + 1));
                write_pretty(item, indent + 1, out);
                out.push_str(if i + 1 == v.len() { "\n" } else { ",\n" });
            }
            out.push_str(&pad(indent));
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

fn render_field(f: &Field) -> String {
    let mut s = format!("{}: {}", f.name, f.shape);
    if let Some(u) = &f.unit {
        s.push_str(&format!(" (unit: {u})"));
    }
    if let Some(p) = &f.pii {
        s.push_str(&format!(" (pii: {p})"));
    }
    s
}

fn finding(rule: &str, severity: Severity, message: String, source: String) -> Finding {
    Finding { rule: rule.to_owned(), severity, message, line: 0, column: 0, source, fix: None }
}

fn with_fix(mut f: Finding, fix: impl Into<String>) -> Finding {
    f.fix = Some(fix.into());
    f
}

/// S1's own gate: is this text a brief, and is it a brief S2 can work from?
///
/// Runnable and measurable alone, which is the point of the stage split. The
/// input is whatever S1 printed; the output is a [`Report`] in exactly S4's
/// shape, so [`Report::repair_prompt`] feeds the ingest stage's own repair
/// round the same way it feeds S2's.
pub fn gate(text: &str) -> Report {
    match Brief::parse(text) {
        Ok(b) => check(&b),
        Err(e) => Report {
            findings: vec![with_fix(
                finding(
                    "s1/not-a-brief",
                    Severity::Error,
                    format!("the output is not a valid brief — {e}"),
                    e.path.clone(),
                ),
                "return one JSON object with exactly the keys the prompt lists, and nothing else \
                 — no prose, no fences",
            )],
        },
    }
}

/// Every structural check over a parsed brief.
///
/// Separate from [`gate`] so a caller holding a [`Brief`] — a human who has
/// just edited one, say — can check it without serialising it first.
pub fn check(brief: &Brief) -> Report {
    let mut findings = Vec::new();

    if brief.summary.trim().is_empty() {
        findings.push(with_fix(
            finding(
                "s1/no-summary",
                Severity::Warning,
                "the brief has no summary, so S2 has entity names and no statement of purpose"
                    .into(),
                String::new(),
            ),
            "one sentence naming what the interface is for",
        ));
    }

    if brief.operations.is_empty() {
        findings.push(with_fix(
            finding(
                "s1/no-operations",
                Severity::Error,
                "the brief lists no operations, so S2 would synthesize an interface nobody can \
                 call"
                    .into(),
                String::new(),
            ),
            "every requirement asks for at least one call; name it, even if its shape is still \
             open — the uncertainty belongs in open_questions",
        ));
    }

    let mut seen: BTreeSet<String> = BTreeSet::new();
    for op in &brief.operations {
        if op.name.trim().is_empty() {
            findings.push(with_fix(
                finding(
                    "s1/unnamed",
                    Severity::Error,
                    "an operation has no name".into(),
                    String::new(),
                ),
                "name it after what it does",
            ));
            continue;
        }
        if !seen.insert(op.name.to_ascii_lowercase()) {
            findings.push(with_fix(
                finding(
                    "s1/duplicate-operation",
                    Severity::Error,
                    format!(
                        "{:?} appears twice; IDL compares identifiers case-insensitively, so two \
                         operations differing only in case are one operation declared twice",
                        op.name
                    ),
                    op.name.clone(),
                ),
                "merge them, or give the second one a name saying how it differs",
            ));
        }
        if op.effect == Effect::Unspecified {
            findings.push(with_fix(
                finding(
                    "s1/effect-unspecified",
                    Severity::Warning,
                    format!(
                        "{:?} does not say what it does to state, so S3 will have to infer \
                         ai_effect from the name alone",
                        op.name
                    ),
                    op.name.clone(),
                ),
                "set effect to read_only, idempotent or destructive — or record the ambiguity in \
                 open_questions so a human settles it",
            ));
        }
    }

    for e in &brief.entities {
        if e.name.trim().is_empty() {
            findings.push(with_fix(
                finding(
                    "s1/unnamed",
                    Severity::Error,
                    "an entity has no name".into(),
                    String::new(),
                ),
                "name it after what it is",
            ));
        }
    }

    findings.extend(shapeless_fields(brief));
    findings.extend(name_clashes(brief));

    // Advice, and deliberately present in the report: the open questions are
    // what a human is being asked to look at, and a brief whose questions are
    // never surfaced is a brief nobody reads.
    for q in &brief.open_questions {
        findings.push(finding(
            "s1/open-question",
            Severity::Advice,
            format!("S1 could not settle this from the requirement: {q}"),
            q.clone(),
        ));
    }

    findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.rule.cmp(&b.rule)));
    Report { findings }
}

fn shapeless_fields(brief: &Brief) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut check_one = |where_: &str, f: &Field| {
        if f.name.trim().is_empty() {
            out.push(with_fix(
                finding(
                    "s1/unnamed",
                    Severity::Error,
                    format!("a field of {where_} has no name"),
                    where_.to_owned(),
                ),
                "name it after the role it plays",
            ));
        } else if f.shape.trim().is_empty() {
            out.push(with_fix(
                finding(
                    "s1/no-shape",
                    Severity::Error,
                    format!(
                        "{where_}.{} has no shape, so S2 has a name and nothing to type it with",
                        f.name
                    ),
                    f.name.clone(),
                ),
                "describe the data in plain words — \"a list of ids\", \"amount in KRW\" — and \
                 leave the IDL type to S2",
            ));
        }
    };
    for e in &brief.entities {
        for f in &e.fields {
            check_one(&e.name, f);
        }
    }
    for o in &brief.operations {
        for f in o.inputs.iter().chain(&o.outputs) {
            check_one(&o.name, f);
        }
    }
    out
}

/// The project's dominant root cause, caught before any IDL exists.
///
/// CORBA 3.4 §7.2.3 compares identifiers case-insensitively, so a field named
/// for the entity it carries is illegal downstream. Phase 0 measured this as
/// 7 failures out of 7; S4 catches it on IDL and this catches it one stage
/// earlier, on the reading that produced the IDL. Attribution is why both
/// exist: a clash reported here is S1's, and the same clash reported by S4
/// after a clean brief is S2's.
fn name_clashes(brief: &Brief) -> Vec<Finding> {
    let entities: Vec<&str> = brief.entities.iter().map(|e| e.name.trim()).collect();
    let clashes = |name: &str| -> Option<String> {
        entities
            .iter()
            .find(|e| !e.is_empty() && e.eq_ignore_ascii_case(name.trim()))
            .map(|e| (*e).to_owned())
    };
    let mut out = Vec::new();

    for e in &brief.entities {
        for f in &e.fields {
            if let Some(t) = clashes(&f.name) {
                out.push(with_fix(
                    finding(
                        "s1/name-clash",
                        Severity::Error,
                        format!(
                            "{}.{} is named for the entity {t:?} it carries; IDL compares \
                             identifiers case-insensitively (CORBA 3.4 §7.2.3), so this is \
                             illegal in the IDL S2 will write from it",
                            e.name, f.name
                        ),
                        f.name.clone(),
                    ),
                    format!(
                        "name the field for its role, not its type: \"the_{0}\", \"{0}_value\" or \
                         a domain word all work. Renaming the entity is usually wrong — other \
                         fields refer to it.",
                        f.name.trim()
                    ),
                ));
            }
        }
    }

    for o in &brief.operations {
        for f in o.inputs.iter().chain(&o.outputs) {
            if let Some(t) = clashes(&f.name) {
                out.push(with_fix(
                    finding(
                        "s1/name-clash",
                        Severity::Error,
                        format!(
                            "the parameter {0:?} of {1} is named for the entity {t:?} it carries; \
                             IDL compares identifiers case-insensitively, so `{t} {0}` will not \
                             compile",
                            f.name, o.name
                        ),
                        f.name.clone(),
                    ),
                    format!("name it for its role: \"the_{0}\" or \"source_{0}\"", f.name.trim()),
                ));
            }
        }
        if let Some(entity) = &o.entity
            && entity.trim().eq_ignore_ascii_case(o.name.trim())
        {
            out.push(with_fix(
                finding(
                    "s1/name-clash",
                    Severity::Error,
                    format!(
                        "the operation {:?} has the same name as the entity {entity:?} it acts \
                         on, ignoring case; an operation may not share a name with a type or an \
                         enclosing scope",
                        o.name
                    ),
                    o.name.clone(),
                ),
                "name the operation for the action and the entity for the thing",
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brief() -> Brief {
        Brief {
            requirement: "사용자 계정을 생성·조회한다. 이메일로도 조회할 수 있어야 한다.".into(),
            summary: "Account lifecycle over a user directory".into(),
            entities: vec![Entity {
                name: "Account".into(),
                description: "one user account".into(),
                fields: vec![
                    Field { name: "id".into(), shape: "opaque string".into(), ..Field::default() },
                    Field {
                        name: "email".into(),
                        shape: "an email address".into(),
                        unit: None,
                        pii: Some("high".into()),
                    },
                    Field {
                        name: "balance".into(),
                        shape: "amount".into(),
                        unit: Some("KRW".into()),
                        pii: None,
                    },
                ],
            }],
            operations: vec![
                OperationSketch {
                    name: "create".into(),
                    entity: Some("Account".into()),
                    inputs: vec![Field {
                        name: "the_email".into(),
                        shape: "an email address".into(),
                        unit: None,
                        pii: Some("high".into()),
                    }],
                    outputs: vec![Field {
                        name: "created".into(),
                        shape: "the new account".into(),
                        ..Field::default()
                    }],
                    effect: Effect::Destructive,
                    errors: vec!["AlreadyExists".into()],
                    authz: Some("accounts.write".into()),
                },
                OperationSketch {
                    name: "find_by_email".into(),
                    entity: Some("Account".into()),
                    inputs: vec![Field {
                        name: "the_email".into(),
                        shape: "an email address".into(),
                        ..Field::default()
                    }],
                    outputs: vec![Field {
                        name: "found".into(),
                        shape: "the account".into(),
                        ..Field::default()
                    }],
                    effect: Effect::ReadOnly,
                    errors: vec!["NotFound".into()],
                    authz: Some("accounts.read".into()),
                },
            ],
            constraints: vec!["조회 실패 시 예외".into()],
            open_questions: vec!["삭제 권한을 누가 갖는지 명시되지 않음".into()],
        }
    }

    /// The property that makes "edit the brief, re-run from S2" real: the
    /// structure survives a trip through the artifact on disk unchanged.
    #[test]
    fn a_brief_round_trips_through_its_json_exactly() {
        let b = brief();
        let text = b.to_json().to_string();
        assert_eq!(Brief::parse(&text).expect("parses"), b);
        // And through the Json value itself, so the text form is not doing
        // any of the work.
        assert_eq!(Brief::from_json(&b.to_json()).expect("reads"), b);
        // And through the artifact form a human edits, which is the one the
        // workspace actually writes.
        let artifact = b.to_text();
        assert!(artifact.contains("\n  \"requirement\""), "indented:\n{artifact}");
        assert_eq!(Brief::parse(&artifact).expect("parses"), b);
    }

    /// Korean is the language the requirements are written in, so it goes
    /// through the JSON layer in every direction or the pipeline cannot ingest
    /// its own benchmark.
    #[test]
    fn korean_requirement_text_survives_the_round_trip() {
        let b = brief();
        let round = Brief::parse(&b.to_json().to_string()).expect("parses");
        assert_eq!(round.requirement, b.requirement);
        assert!(round.open_questions[0].contains("삭제 권한"));
    }

    #[test]
    fn a_clean_brief_passes_its_own_gate() {
        let r = check(&brief());
        assert!(r.is_ok(), "{:?}", r.findings);
        // The open question is still surfaced, as advice.
        assert_eq!(
            r.findings.iter().filter(|f| f.rule == "s1/open-question").count(),
            1,
            "{:?}",
            r.findings
        );
    }

    #[test]
    fn prose_instead_of_json_is_one_actionable_finding() {
        let r = gate("Sure! Here is the brief you asked for.");
        assert!(!r.is_ok());
        assert_eq!(r.findings.len(), 1);
        assert_eq!(r.findings[0].rule, "s1/not-a-brief");
        assert!(r.findings[0].fix.as_deref().unwrap().contains("no fences"));
    }

    /// Fences are the one formatting concession, matching what the IDL path
    /// already does.
    #[test]
    fn markdown_fences_are_stripped_before_parsing() {
        let b = brief();
        let fenced = format!("```json\n{}\n```", b.to_json());
        assert_eq!(Brief::parse(&fenced).expect("parses"), b);
    }

    /// A misspelled key is refused rather than silently dropped — losing
    /// `open_questions` to a typo would lose the field a human reads first.
    #[test]
    fn an_unknown_key_is_refused_and_named() {
        let mut j = brief().to_json();
        let Json::Object(m) = &mut j else { panic!("an object") };
        m.remove("open_questions");
        m.insert("questions".into(), Json::Array(vec![]));
        let e = Brief::from_json(&j).expect_err("refused");
        assert!(e.message.contains("questions"), "{e}");
        assert_eq!(e.path, "$");
    }

    #[test]
    fn a_bad_effect_value_names_the_vocabulary() {
        let text = brief().to_json().to_string().replace("\"destructive\"", "\"probably_fine\"");
        let e = Brief::parse(&text).expect_err("refused");
        assert_eq!(e.path, "$.operations[0].effect");
        assert!(e.message.contains("destructive"), "{e}");
    }

    /// The dominant root cause, one stage earlier than it has ever been caught.
    #[test]
    fn a_field_named_for_its_entity_is_the_phase_0_failure_caught_at_s1() {
        let mut b = brief();
        b.entities.push(Entity {
            name: "Position".into(),
            description: "where a thing is".into(),
            fields: vec![Field {
                name: "position".into(),
                shape: "lat/long".into(),
                ..Field::default()
            }],
        });
        let r = check(&b);
        assert!(!r.is_ok());
        let f = r.findings.iter().find(|f| f.rule == "s1/name-clash").expect("caught");
        assert_eq!(f.source, "position");
        assert!(f.message.contains("case-insensitively"), "{}", f.message);
        assert!(f.fix.as_deref().unwrap().contains("the_position"), "{f:?}");
    }

    #[test]
    fn a_parameter_named_for_the_entity_it_carries_is_caught_too() {
        let mut b = brief();
        b.operations[0].inputs[0].name = "account".into();
        let r = check(&b);
        let f = r.findings.iter().find(|f| f.rule == "s1/name-clash").expect("caught");
        assert!(f.message.contains("`Account account`"), "{}", f.message);
    }

    #[test]
    fn an_operation_named_for_its_entity_is_caught() {
        let mut b = brief();
        b.operations[0].name = "account".into();
        let r = check(&b);
        assert!(r.findings.iter().any(|f| f.rule == "s1/name-clash"), "{:?}", r.findings);
    }

    #[test]
    fn a_brief_with_no_operations_is_refused() {
        let mut b = brief();
        b.operations.clear();
        let r = check(&b);
        assert!(!r.is_ok());
        assert!(r.findings.iter().any(|f| f.rule == "s1/no-operations"));
    }

    #[test]
    fn a_shapeless_field_is_refused_because_s2_cannot_type_it() {
        let mut b = brief();
        b.entities[0].fields[0].shape = "  ".into();
        let r = check(&b);
        let f = r.findings.iter().find(|f| f.rule == "s1/no-shape").expect("refused");
        assert!(f.message.contains("Account.id"), "{}", f.message);
    }

    #[test]
    fn two_operations_differing_only_in_case_are_one_operation_twice() {
        let mut b = brief();
        b.operations[1].name = "CREATE".into();
        let r = check(&b);
        assert!(r.findings.iter().any(|f| f.rule == "s1/duplicate-operation"), "{:?}", r.findings);
    }

    #[test]
    fn an_unspecified_effect_is_a_warning_not_a_refusal() {
        let mut b = brief();
        b.operations[0].effect = Effect::Unspecified;
        let r = check(&b);
        assert!(r.is_ok(), "a brief that admits it does not know still passes");
        let f = r.findings.iter().find(|f| f.rule == "s1/effect-unspecified").expect("warned");
        assert_eq!(f.severity, Severity::Warning);
    }

    /// The rendering S2 is handed must carry everything S2 needs to choose
    /// types — including the two things prose loses first, units and PII.
    #[test]
    fn the_prompt_rendering_carries_the_shapes_units_and_open_questions() {
        let text = brief().to_prompt();
        assert!(text.contains("REQUIREMENT (verbatim)"), "{text}");
        assert!(text.contains("사용자 계정"), "{text}");
        assert!(text.contains("balance: amount (unit: KRW)"), "{text}");
        assert!(text.contains("(pii: high)"), "{text}");
        assert!(text.contains("- create [effect: destructive] [authz: accounts.write]"), "{text}");
        assert!(text.contains("errors: AlreadyExists"), "{text}");
        assert!(text.contains("OPEN QUESTIONS"), "{text}");
    }

    /// What "scope-shaped" is, as a table rather than as a paragraph. The
    /// rejections are the interesting half: a rule that fires on the wrong
    /// tokens is a rule people learn to skim.
    #[test]
    fn scope_shaped_accepts_the_conventions_the_corpus_uses_and_little_else() {
        for token in [
            "gate:operate",        // the measured requirement
            "bank.transfer.write", // the corpus's dotted convention
            "echo:blob",           // a spike fixture
            "accounts.write",
            "parkinglot.barrier.open", // the drift, which is also scope-shaped
            "api.v2",
            "a:b",
            "gate-control:operate_now",
        ] {
            assert!(scope_shaped(token), "{token} should be scope-shaped");
        }
        for token in [
            "",
            "admin",        // no separator: a bare word is not distinguishable
            "GATE:OPERATE", // upper case is a different convention we do not claim
            "Gate:operate",
            "gate/operate", // a slash is a path, and paths are everywhere
            "gate:",        // trailing separator
            ":operate",
            "gate::operate",  // an empty segment
            "1.0",            // a version
            "v1.0",           // …and a version with a prefix
            "읽기:권한",      // non-ASCII: matching it would fire on prose
            "gate:operate x", // a phrase is not a token
        ] {
            assert!(!scope_shaped(token), "{token:?} should not be scope-shaped");
        }
        assert!(
            !scope_shaped(&format!("a.{}", "b".repeat(MAX_SCOPE_LEN))),
            "a scope is not a text"
        );
    }

    /// The scan is what the false-positive rate over `corpus/requirements/` is
    /// measured with, so its own behaviour on sentence punctuation and on
    /// mixed-case neighbours is pinned here.
    #[test]
    fn the_scan_finds_a_stated_token_and_leaves_prose_alone() {
        assert_eq!(scope_tokens("차단기 개방은 gate:operate 권한이 필요하다."), ["gate:operate"]);
        assert_eq!(scope_tokens("The scope is gate:operate."), ["gate:operate"]);
        assert_eq!(scope_tokens("운영자 권한을 가진 사람만 열 수 있다."), Vec::<String>::new());
        // A capitalised neighbour must not be shaved into a token the text does
        // not contain.
        assert_eq!(scope_tokens("Gate:operate"), Vec::<String>::new());
        assert_eq!(scope_tokens("a.b a.b"), ["a.b"], "deduplicated");
    }

    /// The binding, and the two conditions that keep it honest.
    #[test]
    fn a_brief_binds_only_a_scope_shaped_token_the_requirement_states() {
        let mut b = brief();
        // The default brief's tokens are S1's own words: scope-shaped, but the
        // requirement never states them, so nothing binds.
        assert!(b.stated_scopes().is_empty(), "{:?}", b.stated_scopes());

        b.requirement.push_str(" 계정 생성은 accounts.write 권한이 필요하다.");
        assert_eq!(
            b.stated_scopes(),
            BTreeMap::from([("create".to_owned(), "accounts.write".to_owned())]),
            "the token the requirement states binds; the one it does not, does not"
        );

        // A permission stated only in prose binds nothing at all, which is the
        // limit rather than a defect.
        b.operations[0].authz = Some("운영자 권한".into());
        assert!(b.stated_scopes().is_empty());
    }

    /// The codify rule for this stage: the prompt must describe every key the
    /// parser insists on, or S1 fails on a schema the model was never told.
    #[test]
    fn the_prompt_describes_every_key_the_parser_requires() {
        for key in [
            "requirement",
            "summary",
            "entities",
            "operations",
            "constraints",
            "open_questions",
            "shape",
            "effect",
            "errors",
            "authz",
            "pii",
            "unit",
        ] {
            assert!(S1_PROMPT.contains(&format!("\"{key}\"")), "the prompt never names {key}");
        }
        // And the dominant root cause is a prompt constraint, not only a check.
        assert!(S1_PROMPT.contains("case-insensitive"), "{S1_PROMPT}");
        // So is the one thing S1 records that a later stage binds by string
        // equality: a token S1 tidies here is a token nothing can bind.
        assert!(S1_PROMPT.contains("copied verbatim"), "{S1_PROMPT}");
    }
}
