//! The lock discipline concurrent dispatch is allowed to have, made
//! structural instead of conventional.
//!
//! [`crate::server::SharedDispatch`] lets two calls into one servant run at
//! once, which is the point of the batch — and it hands every servant the one
//! hazard the concurrency batch already fought: a servant that holds a lock
//! while it calls out. `event_server`'s module docs state the rule
//! ("**no lock may be held across an outbound call**") and its delivery loop
//! obeys it by construction. Nothing enforced it. Documenting a rule does not
//! prevent it — this repository has the receipts.
//!
//! # The discipline
//!
//! **One lock per servant, at most one lock held per thread, and nothing that
//! can block inside it.**
//!
//! *서번트당 락 하나, 스레드당 동시에 하나, 락 안에서 블로킹 금지.*
//!
//! Three consequences, and the reason the rule is shaped this way:
//!
//! 1. **Lock ordering becomes vacuous.** A deadlock cycle needs a thread
//!    holding lock A while it waits for lock B. If a thread never holds two,
//!    there is no order to get wrong and no cycle to draw — so this module
//!    does not define a lock hierarchy, it makes one unnecessary.
//! 2. **Re-entrancy becomes visible.** Taking the same [`Guarded`] twice on
//!    one thread is a deadlock on most `RwLock` implementations and undefined
//!    behaviour on some. It is the same rule violation as (1), so it is caught
//!    by the same check.
//! 3. **"No lock across a blocking call" becomes checkable**, because
//!    "is this thread inside a section" is a question with an answer. The
//!    outbound client path asks it: [`assert_nothing_held`] is called by
//!    [`Connection::connect`](crate::Connection::connect) and by every
//!    `invoke`, which is where every outbound call in this workspace funnels.
//!
//! # How it is enforced
//!
//! - **The guard never escapes.** [`Guarded::read`] and [`Guarded::write`]
//!   take a closure and return its value. There is no `lock()` returning a
//!   guard, so a servant cannot hold one across an `await`-shaped gap by
//!   accident; holding one across a call requires writing the call inside the
//!   closure, where a reader can see it.
//! - **The section is counted**, per thread, and entering one while another is
//!   open is a panic in a debug build and a loud line in a release one. Both
//!   (1) and (2) are that check.
//! - **The outbound path checks the counter.** A servant that calls out from
//!   inside a section fails its own test rather than deadlocking somebody
//!   else's deployment six months later.
//!
//! Panicking in a debug build and complaining in a release one is the deliberate
//! split: a violation is a *bug*, and tests run in debug, so it fails where it
//! can still be fixed — but a live ORB that has already survived the mistake
//! must not be taken down by the diagnostic for it. The count is maintained in
//! both, so the release path really does detect it rather than merely not
//! panicking.
//!
//! # Asserting it in both profiles
//!
//! The split is a hazard for the *tests*, and it was one. They asserted the
//! panic — `catch_unwind(...).is_err()` — so in a release build they asserted
//! something a release build does not do: six of them failed, and
//! `cargo test --release` could not be run clean. A profile nobody can run
//! clean is a profile nobody runs, and that is where a release-only defect
//! lives — `36c8bc0` is an arithmetic overflow that wrapped in release and
//! panicked in debug, found by a fuzzer because no test pass would have.
//!
//! So the *outcome* is counted as well as reported. [`violations`] rises on
//! every complaint in both profiles — before the debug build panics, so the
//! two record the same thing and differ only in what they do next — and
//! [`complaints_about`] asks the question one way for both. The panic becomes
//! the debug build's *reaction* to a violation rather than the only evidence
//! that one happened.
//!
//! Recording the complaint and not just counting it is what lets a test say
//! *which* rule fired, and that turned out to matter: deleting
//! [`assert_nothing_held`]'s body left two of these tests green, because
//! `Pool::acquire` and `Mux::call_on` open sections of their own and the
//! nesting rule caught them instead. A test that cannot tell its own subject
//! apart from a neighbour passes for the wrong reason.
//!
//! The count is **per thread**, like the section count and for the same
//! reason: a violation belongs to the thread that committed it. A
//! process-wide counter would let one test's violation satisfy another test's
//! assertion while the two run in parallel, which is "an unmeasured check
//! reported as a pass" wearing a third hat.
//!
//! *디버그는 패닉, 릴리스는 경고 — 그 차이를 테스트가 떠안지 않도록 위반
//! 자체를 스레드별로 센다. 같은 속성을 두 프로파일에서 검증한다.*
//!
//! # What this is not
//!
//! It is not a lock. It is a *shape* for one, and it does not fit everything:
//! `event_server` needs its state under a `Mutex` a `Condvar` can wait on, and
//! a `Condvar` cannot wait on a closure. That module keeps its own mutex and
//! joins this discipline from the other end — its guard type holds a
//! [`Section`] token, so the same counter sees it and the same tripwire fires.
//! A servant with genuinely independent state may hold several `Guarded`s; it
//! may just not hold two at the same instant, which is the rule, not an
//! accident of this API.

use std::cell::{Cell, RefCell};
use std::sync::RwLock;

thread_local! {
    /// How many lock sections this thread has open, and what the innermost one
    /// is called. Zero for every thread that is not inside one — which is
    /// every thread, almost always, because that is the discipline.
    static OPEN: Cell<Option<&'static str>> = const { Cell::new(None) };

    /// Lock-discipline violations this thread has committed, counted in every
    /// profile. The panic is one build's reaction to a violation; this is the
    /// violation.
    static VIOLATIONS: Cell<u64> = const { Cell::new(0) };

    /// The complaints themselves, recorded only while [`complaints_about`] is
    /// running on this thread. `None` the rest of the time, so a live ORB
    /// that keeps violating does not accumulate a diagnostic it never reads.
    static RECORDING: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

/// An open critical section, counted for the current thread.
///
/// Constructing one asserts the discipline; dropping one closes the section,
/// including the drop an unwinding panic performs — which is why this is a
/// guard type and not a pair of calls.
#[derive(Debug)]
pub struct Section {
    previous: Option<&'static str>,
}

impl Section {
    /// Opens a section named `what`, complaining if one is already open.
    ///
    /// `what` is a static label so the complaint can name *both* sections: a
    /// deadlock report that says only "a lock was held" is one nobody can act
    /// on.
    pub fn enter(what: &'static str) -> Self {
        // Checked *before* the counter moves, so a violation that panics
        // leaves the thread's state exactly as it found it. Marking first and
        // checking after would leave a caught panic having renamed the section
        // its caller is still inside.
        let previous = OPEN.with(|o| o.get());
        if let Some(outer) = previous {
            violation(&format!(
                "entered the lock section {what} while {outer} was already held on this thread; \
                 the discipline is one lock at a time (crates/orbweaver-giop/src/guarded.rs)"
            ));
        }
        OPEN.with(|o| o.set(Some(what)));
        Section { previous }
    }
}

impl Drop for Section {
    fn drop(&mut self) {
        OPEN.with(|o| o.set(self.previous));
    }
}

/// Panics in a debug build, complains in a release one. See the module docs
/// for why the two differ.
fn violation(message: &str) {
    // Counted *before* the panic, deliberately: a debug build must record the
    // violation it is about to die of, or the two profiles would not be
    // answering the same question and a test could only ask one of them.
    VIOLATIONS.with(|v| v.set(v.get().saturating_add(1)));
    RECORDING.with(|r| {
        if let Some(log) = r.borrow_mut().as_mut() {
            log.push(message.to_string());
        }
    });
    if cfg!(debug_assertions) {
        panic!("orbweaver: lock discipline violated: {message}");
    }
    eprintln!("orbweaver: lock discipline violated: {message}");
}

/// Fails if the calling thread is inside a lock section.
///
/// Called from the outbound client path — connecting and invoking — because
/// that is the one place a servant can turn a held lock into a cross-process
/// deadlock. `what` names the operation, so the complaint says which call was
/// about to block.
pub fn assert_nothing_held(what: &str) {
    if let Some(held) = OPEN.with(|o| o.get()) {
        violation(&format!(
            "{what} would block while the lock section {held} is held; \
             copy what you need out of the lock, close it, then call"
        ));
    }
}

/// Whether the calling thread is inside a lock section, and which.
///
/// Exists for the tests that prove the tripwire works: an enforcement
/// mechanism nobody measured is the "unmeasured check reported as a pass"
/// failure wearing a different hat.
pub fn section_held() -> Option<&'static str> {
    OPEN.with(|o| o.get())
}

/// How many lock-discipline violations the calling thread has committed.
///
/// Monotonic for the life of the thread, and maintained identically in both
/// build profiles — they differ in what they *do* about a violation, never in
/// whether they saw one.
pub fn violations() -> u64 {
    VIOLATIONS.with(|v| v.get())
}

/// Runs `f` and returns what the lock discipline complained about, in the
/// order it complained — the same answer in a debug build and a release one.
///
/// This is how a test asserts the discipline. Catching the panic instead
/// asserts nothing in a release build, where there is no panic to catch; see
/// the module docs for what that cost.
///
/// A panic raised by `f` is absorbed, because in a debug build the violation
/// *is* one. A panic with no violation behind it therefore yields an empty
/// list rather than passing: `catch_unwind(...).is_err()` could not tell the
/// tripwire firing apart from an `unwrap` failing on the way to it.
///
/// **A debug build stops at the first complaint** and a release build carries
/// on, so only `first()` is a both-profile observation. Assert on it, not on
/// the length — a call that violates twice reports twice in release and once
/// in debug, and a test that pinned the count would be pinning the profile.
pub fn complaints_about(f: impl FnOnce()) -> Vec<String> {
    let outer = RECORDING.with(|r| r.replace(Some(Vec::new())));
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    RECORDING.with(|r| r.replace(outer)).unwrap_or_default()
}

/// Whether the lock discipline caught `f` violating it, for the tests that do
/// not care which rule was broken. [`complaints_about`] is the one that does.
pub fn catches_a_violation(f: impl FnOnce()) -> bool {
    !complaints_about(f).is_empty()
}

/// State shared by every thread serving a servant, reachable only inside a
/// counted section.
///
/// An `RwLock` and not a `Mutex`: the servants ported in this batch each have
/// a read-only half of their wire surface — `resolve`, `list`,
/// `get_manifest`, `describe` — and a read that blocks a read is exactly the
/// serialization this batch exists to remove. A servant whose every operation
/// writes still uses this type and simply never calls [`Guarded::read`]; the
/// choice is per servant and argued in each module.
#[derive(Default)]
pub struct Guarded<T> {
    what: &'static str,
    inner: RwLock<T>,
}

impl<T> Guarded<T> {
    /// State named `what`, which is what a discipline violation will be
    /// reported as. Name it after the servant, not after the type.
    pub fn new(what: &'static str, value: T) -> Self {
        Guarded { what, inner: RwLock::new(value) }
    }

    /// Reads the state. Concurrent readers proceed together; this is the half
    /// of the surface concurrent dispatch actually buys.
    ///
    /// A poisoned lock is recovered rather than propagated, the policy
    /// [`crate::server::Server`] already applies to its own: one panicking
    /// request must not become a permanently dead service for everybody else.
    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let _section = Section::enter(self.what);
        let guard = self.inner.read().unwrap_or_else(|e| e.into_inner());
        f(&guard)
    }

    /// Mutates the state, excluding every other reader and writer.
    pub fn write<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let _section = Section::enter(self.what);
        let mut guard = self.inner.write().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }

    /// The state, consumed. No section: nothing else can hold a reference to
    /// a value being moved out.
    pub fn into_inner(self) -> T {
        self.inner.into_inner().unwrap_or_else(|e| e.into_inner())
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Guarded<T> {
    /// Deliberately non-blocking: a `Debug` that waits for a lock turns a
    /// diagnostic print into a deadlock, and `Debug` is exactly what somebody
    /// reaches for while diagnosing one.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.inner.try_read() {
            Ok(v) => {
                f.debug_struct("Guarded").field("what", &self.what).field("state", &*v).finish()
            }
            Err(_) => f
                .debug_struct("Guarded")
                .field("what", &self.what)
                .field("state", &"<held>")
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_section_leaves_nothing_held() {
        let g = Guarded::new("test", 1u32);
        assert_eq!(g.read(|v| *v), 1);
        assert_eq!(section_held(), None, "the section must close when the closure returns");
        g.write(|v| *v += 1);
        assert_eq!(g.read(|v| *v), 2);
        assert_eq!(section_held(), None);
    }

    #[test]
    fn a_section_is_visible_from_inside_it() {
        let g = Guarded::new("visible", ());
        g.read(|_| assert_eq!(section_held(), Some("visible")));
    }

    /// The tripwire's whole job, exercised directly: the outbound path's
    /// assertion must fire from inside a section and stay silent outside one.
    ///
    /// Both halves go through [`catches_a_violation`], so both are asserted in
    /// a release build too — the negative control especially. "Did not panic"
    /// is not an observation a release build makes, so a control written that
    /// way was vacuous in exactly the profile that ships.
    #[test]
    fn the_outbound_tripwire_fires_from_inside_a_section() {
        assert!(
            !catches_a_violation(|| assert_nothing_held("a call outside any section")),
            "a call outside any section must not be reported as one"
        );
        let g = Guarded::new("tripwire", ());
        let before = violations();
        let said = complaints_about(|| {
            g.read(|_| assert_nothing_held("an outbound call from inside a section"));
        });
        assert!(
            said.first()
                .is_some_and(|c| c.contains("an outbound call from inside a section")
                    && c.contains("tripwire")),
            "the complaint must name both the call and the section it was made from, got {said:?}"
        );
        assert_eq!(violations(), before + 1, "one crossing, one complaint");
        assert_eq!(section_held(), None, "the section must have closed either way");
    }

    /// Re-entering — the same lock twice, or two locks at once — is the same
    /// violation, because it is the same rule.
    ///
    /// "Caught" and not "refused": a release build does not refuse it, it
    /// takes the second lock and says so. What holds in both profiles is that
    /// the discipline *sees* it, and that is what the name should claim.
    #[test]
    fn a_second_section_on_one_thread_is_caught() {
        let a = Guarded::new("outer", ());
        let b = Guarded::new("inner", ());
        let said = complaints_about(|| {
            a.read(|_| b.read(|_| ()));
        });
        assert!(
            said.first().is_some_and(|c| c.contains("inner") && c.contains("outer")),
            "the complaint must name both sections, got {said:?}"
        );
        assert_eq!(section_held(), None, "both sections must have closed behind it");
    }

    /// A panic inside a section must not leave the thread permanently marked
    /// as holding one: the next request on that thread would be refused for a
    /// lock nobody holds.
    #[test]
    fn a_panicking_closure_still_closes_its_section() {
        let g = Guarded::new("panicky", ());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            g.read(|_| panic!("boom"));
        }));
        assert_eq!(section_held(), None);
        g.read(|_| ()); // and the next one is admitted
    }

    /// Two claims at once, because they are the same claim from two sides:
    /// the section count is **per thread**, and readers really do overlap.
    ///
    /// Every reader waits inside its own section until all four have arrived.
    /// A per-process count would refuse the second reader; a `Mutex` behind
    /// `read` would let only one in and the rest would wait out the deadline.
    /// The deadline is what makes either a failed test rather than a hung one.
    #[test]
    fn readers_overlap_and_the_count_is_per_thread() {
        use std::sync::atomic::{AtomicU32, Ordering};
        const N: u32 = 4;
        let g = Guarded::new("shared", 0u32);
        let inside = AtomicU32::new(0);
        let all_here = std::thread::scope(|scope| {
            let readers: Vec<_> = (0..N)
                .map(|_| {
                    scope.spawn(|| {
                        g.read(|_| {
                            assert_eq!(section_held(), Some("shared"));
                            inside.fetch_add(1, Ordering::SeqCst);
                            let deadline =
                                std::time::Instant::now() + std::time::Duration::from_secs(10);
                            while inside.load(Ordering::SeqCst) < N {
                                if std::time::Instant::now() >= deadline {
                                    return false;
                                }
                                std::thread::sleep(std::time::Duration::from_millis(2));
                            }
                            true
                        })
                    })
                })
                .collect();
            readers.into_iter().all(|r| r.join().unwrap())
        });
        assert!(all_here, "four readers must be inside the same Guarded at once");
        assert_eq!(section_held(), None);
    }
}
