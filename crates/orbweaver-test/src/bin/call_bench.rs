//! `call-bench` — the number `docs/PLAN.md` §8's performance row has been
//! citing since v0.2 and nobody could produce.
//!
//! §8 asks for a "LAN echo benchmark, dynamic path vs static stub", judged
//! "within §11 targets". §11 states those targets as *dynamic-path overhead vs
//! static stub (LAN echo, p50) ≤ 5 ms added and ≤ 3× static*. Two numbers, one
//! percentile, and — as the header of the report this prints says out loud —
//! not enough of a specification to pass or fail against: it names no operation
//! shape, no payload size, no sample count, no machine, and no way to tell
//! which of its two clauses binds. This binary measures the thing the row
//! describes and leaves the verdict to a human, because a target that cannot be
//! compared to is not one.
//!
//! # What is measured
//!
//! One process, one server, one **real TCP connection** over loopback, and two
//! clients over that same connection:
//!
//! * the **static** path — the client stub `orbweaver-gen` emitted for
//!   `spikes/bench/bench.idl`, compiled in;
//! * the **dynamic** path — `orbweaver_dynamic::invoke::invoke`, which knows
//!   nothing at compile time and rebuilds every call from the registry.
//!
//! Both call the same operation, on the same servant, through the same socket,
//! interleaved call for call. Nothing is shimmed: where the two paths could not
//! serve the same operation there would be no row, rather than a row built out
//! of two different servers.
//!
//! Three operation shapes, because the interesting question is not the fixed
//! cost of a round trip:
//!
//! | shape | payload |
//! |---|---|
//! | `add` | two `long`s — the scalar floor |
//! | `echo_text` | one string, small and large |
//! | `echo_many` | a sequence of strings — the per-string indirection |
//!
//! `docs/decisions/D009-codeset-reaches-the-marshaller.md` §9 names a benchmark
//! it cannot run as the thing that would falsify its recommendation, and the
//! question it asks is what a **per-string** indirection costs. A benchmark
//! whose widest payload is an integer cannot answer that, which is why
//! `echo_many` exists and why its element count is chosen here rather than in
//! the contract.
//!
//! # Why the generated stub lives in `spikes/`
//!
//! `#[path]` out of the crate is unusual and deliberate. The static path has to
//! be *generated* code or it is not the static path, and generated code that
//! this binary emitted at startup would be measuring rustc, not the stub. So it
//! is checked in beside the contract it comes from, under `spikes/bench/`, with
//! the fixtures — and [`check_stub_is_current`] regenerates it on every run and
//! refuses to measure a stale one. A fossil stub would report the performance of
//! a generator that no longer exists.
//!
//! # Usage
//!
//! ```text
//! call-bench [--samples N] [--warmup N] [--tsv] [--max-ratio F]
//! ```
//!
//! Exit code is about the *measurement*, not the numbers: 0 when every series
//! was measured and the two paths agreed on every answer, 1 when something
//! could not be measured. `--max-ratio` is opt-in and off by default.
//!
//! # In a harness
//!
//! Run it for the *shape* of the answer, not for a threshold:
//!
//! ```text
//! cargo run -q --release -p orbweaver-test --bin call-bench -- --samples 200 --tsv
//! ```
//!
//! That exits non-zero only when a series could not be measured or the two
//! paths disagreed — both of which are defects on any machine, at any speed —
//! and it prints the numbers into the run log where a human can watch them
//! move. A latency threshold in a shared harness fails on the day CI is busy
//! and teaches everyone to re-run it, which costs more than the regression it
//! was meant to catch.
//!
//! `--samples` is the knob for a short run. `--warmup` is not: see
//! [`DEFAULT_WARMUP`] for the measurement that says why.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use orbweaver_dynamic::Value;
use orbweaver_dynamic::invoke::invoke;
use orbweaver_gen::rt::ObjectHome;
use orbweaver_giop::orb::Orb;
use orbweaver_giop::{Connection, Ior};
use orbweaver_registry::Registry;

#[path = "../../../../spikes/bench/stub.rs"]
mod f_bench;

use f_bench::obbench::{
    MeterClient, MeterFault, MeterObject, MeterObjects, MeterRefs, MeterSkeleton,
};

/// The interface both paths call.
const TYPE_ID: &str = "IDL:obbench/Meter:1.0";
/// The object key the server is bound with; the default object's oid is empty.
const KEY: &[u8] = b"meter";
/// The module root the checked-in stub was generated under. It appears inside
/// the generated source as `crate::f_bench::…`, so the `mod` above must keep
/// this name and the freshness check must pass the same string.
const STUB_ROOT: &str = "f_bench";

// ── The measurement rules, stated here rather than in a footnote ─────────────

/// Calls made and thrown away before any sample is kept, per series.
///
/// A first call over a fresh connection pays for things no later call does:
/// the allocator's first growth of every buffer, the socket's first write, and
/// on a laptop the CPU still deciding what frequency to run at. Discarding a
/// fixed count is the cheap, stateable rule; the alternative — discarding until
/// some stability criterion holds — is a rule that changes with the machine.
///
/// 300 is not a round number picked for looks. At 20 the first run of this
/// benchmark reported the *scalar* shape at 68µs and the 4 KiB string at 24µs —
/// the payloads ranked backwards, because the whole process was still cold and
/// the first shape measured was paying for all of it. The shapes are measured
/// in order, so an under-warmed run does not look noisy; it looks like a
/// finding. Anything that lowers this number has to explain that inversion
/// first.
///
/// So a short run lowers `--samples`, never this. Measured on the same machine
/// within a minute of each other: `--warmup 50` reported the scalar p50 at
/// 61µs, `--warmup 300` at 21µs, and `--warmup 2000` at 16µs — a run that
/// under-warms is not a noisier version of the real answer, it is a different
/// answer, and it is the wrong one in a consistent direction.
const DEFAULT_WARMUP: usize = 300;

/// Samples kept per series.
const DEFAULT_SAMPLES: usize = 2000;

/// **No sample is discarded after warm-up.** Not a trimmed mean, not an
/// outlier filter, not a "best of N".
///
/// A loopback round trip is a few tens of microseconds and a scheduler
/// preemption is a few hundred, so any trimming rule removes exactly the
/// samples that say what the machine was doing — which is the thing a
/// benchmark taken on a busy laptop most needs to report. The distribution is
/// published instead: p50 for the typical call, p99 and max for what the
/// machine did to it, and [`clock_floor`] for how much of that was
/// never ours to begin with.
const TRIMMING: &str = "none — every post-warm-up sample is kept and reported";

/// The timed region starts where a caller **has the payload and wants the
/// answer**, so argument construction is inside it for both paths.
///
/// This is the one methodological choice that could quietly decide the result.
/// Excluding argument construction would hide the dynamic path's
/// `BTreeMap<String, Value>` — which is a real per-call cost an agent pays —
/// while the static path's `String` clone stayed hidden too, and the two are
/// not the same size. Including it for both keeps the comparison about the
/// path. It does mean neither column is "pure wire time"; the scalar row is
/// the closest thing to that.
const TIMED_REGION: &str = "argument construction through decoded answer, both paths";

fn main() -> std::process::ExitCode {
    let mut samples = DEFAULT_SAMPLES;
    let mut warmup = DEFAULT_WARMUP;
    let mut tsv = false;
    let mut max_ratio: Option<f64> = None;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut number = |what: &str| -> Option<String> {
            let v = args.next();
            if v.is_none() {
                eprintln!("{what} needs a value");
            }
            v
        };
        match a.as_str() {
            "--samples" => match number("--samples").and_then(|v| v.parse().ok()) {
                Some(n) => samples = n,
                None => return std::process::ExitCode::from(2),
            },
            "--warmup" => match number("--warmup").and_then(|v| v.parse().ok()) {
                Some(n) => warmup = n,
                None => return std::process::ExitCode::from(2),
            },
            "--max-ratio" => match number("--max-ratio").and_then(|v| v.parse().ok()) {
                Some(f) => max_ratio = Some(f),
                None => return std::process::ExitCode::from(2),
            },
            "--tsv" => tsv = true,
            other => {
                eprintln!(
                    "usage: call-bench [--samples N] [--warmup N] [--tsv] [--max-ratio F]\n\
                     unexpected argument {other:?}"
                );
                return std::process::ExitCode::from(2);
            }
        }
    }
    if samples == 0 {
        eprintln!("--samples 0 measures nothing; an unmeasured check is a failure, not a pass");
        return std::process::ExitCode::from(2);
    }

    match run(samples, warmup, tsv, max_ratio) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("call-bench: not measured: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

// ── The servant. Every operation is an echo, so both paths cost the server the
//    same and the difference between them is the client path ──────────────────

/// The thing being called. It computes nothing on purpose: a servant that did
/// real work would add the same constant to both columns and shrink the
/// difference the benchmark exists to show.
struct Echo;

impl MeterObject for Echo {
    fn add(&mut self, a: i32, b: i32) -> Result<i32, MeterFault> {
        Ok(a.wrapping_add(b))
    }

    fn echo_text(&mut self, msg: String) -> Result<String, MeterFault> {
        Ok(msg)
    }

    fn echo_many(&mut self, items: Vec<String>) -> Result<Vec<String>, MeterFault> {
        Ok(items)
    }
}

// ── Shapes ───────────────────────────────────────────────────────────────────

/// One operation with one payload size: a row of the report.
struct Shape {
    /// How the row is labelled.
    label: &'static str,
    /// The operation name on the wire.
    operation: &'static str,
    /// The payload, prepared once so that neither path is timed building it
    /// from nothing — both build their own arguments *from this* inside the
    /// timed region.
    payload: Payload,
    /// Request body bytes, counted once at warm-up rather than computed, so the
    /// report says what actually travelled.
    request_bytes: usize,
}

/// The source data a call's arguments are built from.
enum Payload {
    /// `add(a, b)`.
    Scalars(i32, i32),
    /// `echo_text(msg)`.
    Text(String),
    /// `echo_many(items)`.
    Many(Vec<String>),
}

impl Payload {
    /// The dynamic path's arguments, built fresh — this allocation is part of
    /// what the dynamic path costs and is inside the timed region.
    fn dynamic_args(&self) -> BTreeMap<String, Value> {
        let mut args = BTreeMap::new();
        match self {
            Payload::Scalars(a, b) => {
                args.insert("a".to_owned(), Value::Long(*a));
                args.insert("b".to_owned(), Value::Long(*b));
            }
            Payload::Text(s) => {
                args.insert("msg".to_owned(), Value::String(s.clone()));
            }
            Payload::Many(v) => {
                args.insert(
                    "items".to_owned(),
                    Value::List(v.iter().map(|s| Value::String(s.clone())).collect()),
                );
            }
        }
        args
    }
}

/// What one call answered, in a form both paths can produce and be compared on.
///
/// The comparison is not decoration: a path that returned early, or returned a
/// truncated sequence, would look faster. Every sample of every series is
/// checked against the payload it was given.
#[derive(PartialEq, Debug)]
enum Answer {
    Scalar(i32),
    Text(String),
    Many(Vec<String>),
}

impl Answer {
    fn expected(p: &Payload) -> Answer {
        match p {
            Payload::Scalars(a, b) => Answer::Scalar(a.wrapping_add(*b)),
            Payload::Text(s) => Answer::Text(s.clone()),
            Payload::Many(v) => Answer::Many(v.clone()),
        }
    }
}

fn shapes() -> Vec<Shape> {
    // Sizes are round numbers with a reason, not tuning. 16 bytes is a label or
    // a key — the string a real contract carries most of. 4 KiB is a document
    // field, big enough that the copy is visible over the round trip. 64 × 24 is
    // the shape D009 asks about: many small strings, where the per-element
    // indirection is paid 64 times and the total bytes stay modest, so a
    // difference between the paths cannot be blamed on volume.
    vec![
        Shape {
            label: "add (2 longs)",
            operation: "add",
            payload: Payload::Scalars(1_000_000, 337),
            request_bytes: 0,
        },
        Shape {
            label: "echo_text 16 B",
            operation: "echo_text",
            payload: Payload::Text("s".repeat(16)),
            request_bytes: 0,
        },
        Shape {
            label: "echo_text 4 KiB",
            operation: "echo_text",
            payload: Payload::Text("s".repeat(4096)),
            request_bytes: 0,
        },
        Shape {
            label: "echo_many 64x24 B",
            operation: "echo_many",
            payload: Payload::Many((0..64).map(|i| format!("{i:024}")).collect()),
            request_bytes: 0,
        },
    ]
}

// ── The two paths ────────────────────────────────────────────────────────────

/// One call through the generated stub.
fn call_static(client: &mut MeterClient<Connection>, p: &Payload) -> Result<Answer, String> {
    match p {
        Payload::Scalars(a, b) => client.add(*a, *b).map(Answer::Scalar).map_err(|e| e.to_string()),
        Payload::Text(s) => {
            client.echo_text(s.clone()).map(Answer::Text).map_err(|e| e.to_string())
        }
        Payload::Many(v) => {
            client.echo_many(v.clone()).map(Answer::Many).map_err(|e| e.to_string())
        }
    }
}

/// One call through `invoke`, which knows the operation only by name.
fn call_dynamic(
    conn: &mut Connection,
    registry: &Registry,
    operation: &str,
    p: &Payload,
) -> Result<Answer, String> {
    let args = p.dynamic_args();
    let out = invoke(conn, registry, TYPE_ID, operation, &args).map_err(|e| e.to_string())?;
    match out.returns {
        Value::Long(v) => Ok(Answer::Scalar(v)),
        Value::String(s) => Ok(Answer::Text(s)),
        Value::List(items) => {
            let mut v = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(s) => v.push(s),
                    other => return Err(format!("element is not a string: {other:?}")),
                }
            }
            Ok(Answer::Many(v))
        }
        other => Err(format!("unexpected return {other:?}")),
    }
}

// ── Statistics ───────────────────────────────────────────────────────────────

/// One measured series: a path over a shape.
struct Series {
    /// Every kept sample, in nanoseconds, sorted ascending.
    sorted: Vec<u64>,
}

impl Series {
    fn new(mut ns: Vec<u64>) -> Self {
        ns.sort_unstable();
        Series { sorted: ns }
    }

    fn n(&self) -> usize {
        self.sorted.len()
    }

    /// Nearest-rank percentile: the smallest sample at or above the rank, which
    /// is an observation rather than an interpolation between two of them.
    fn pct(&self, p: f64) -> f64 {
        if self.sorted.is_empty() {
            return f64::NAN;
        }
        let rank = ((p / 100.0) * self.sorted.len() as f64).ceil().max(1.0) as usize;
        self.sorted[rank.min(self.sorted.len()) - 1] as f64 / 1000.0
    }

    fn min_us(&self) -> f64 {
        self.sorted.first().copied().unwrap_or(0) as f64 / 1000.0
    }

    fn max_us(&self) -> f64 {
        self.sorted.last().copied().unwrap_or(0) as f64 / 1000.0
    }
}

/// How much of the measurement is the measurement.
///
/// `Instant::now()` twice around nothing at all, sampled the same way as a
/// call. If this floor's p99 is a large fraction of a series' p99, the tail
/// being reported is the machine's scheduler, not the ORB — and saying so is
/// the difference between a benchmark and a number.
fn clock_floor(n: usize) -> Series {
    let mut ns = Vec::with_capacity(n);
    for _ in 0..n {
        let t = Instant::now();
        std::hint::black_box(());
        ns.push(t.elapsed().as_nanos() as u64);
    }
    Series::new(ns)
}

/// What the machine was doing, as far as this process can tell.
///
/// Load average is the one figure available without a dependency, and it is
/// reported rather than acted on: a benchmark that silently refused to run
/// under load would produce no number at all on CI, which is worse.
fn load_average() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(s) = std::fs::read_to_string("/proc/loadavg") {
            let f: Vec<&str> = s.split_whitespace().take(3).collect();
            if f.len() == 3 {
                return f.join(" ");
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        // Captured to a variable, never piped into a matcher — the harness rule
        // about `grep -q` is about producers that matter, and this one does.
        if let Ok(out) =
            std::process::Command::new("/usr/sbin/sysctl").args(["-n", "vm.loadavg"]).output()
        {
            let s = String::from_utf8_lossy(&out.stdout);
            let t = s.trim().trim_matches(|c| c == '{' || c == '}').trim();
            if !t.is_empty() {
                return t.to_owned();
            }
        }
    }
    "not measured on this platform".to_owned()
}

// ── The run ──────────────────────────────────────────────────────────────────

/// Regenerates the stub from the contract and refuses to measure a stale one.
///
/// The same discipline as `crates/orbweaver-gen/tests/emitted_current.rs`, for
/// the same reason: a checked-in generated file with no freshness check is a
/// fossil, and a benchmark over a fossil reports the speed of a generator that
/// no longer exists. `ORBWEAVER_BLESS=1` rewrites it, which is the recovery
/// path after a template change.
fn check_stub_is_current(registry: &Registry, root: &std::path::Path) -> Result<(), String> {
    let path = root.join("spikes/bench/stub.rs");
    let want = orbweaver_gen::emit(registry, STUB_ROOT).source;
    if std::env::var_os("ORBWEAVER_BLESS").is_some() {
        std::fs::write(&path, &want).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(());
    }
    let have = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if have != want {
        return Err(format!(
            "{} is stale — re-bless it in the same commit as the template change:\n  \
             ORBWEAVER_BLESS=1 cargo run -q -p orbweaver-test --bin call-bench -- --samples 1",
            path.display()
        ));
    }
    Ok(())
}

fn run(
    samples: usize,
    warmup: usize,
    tsv: bool,
    max_ratio: Option<f64>,
) -> Result<std::process::ExitCode, String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let idl = root.join("spikes/bench/bench.idl");
    let src = std::fs::read_to_string(&idl).map_err(|e| format!("{}: {e}", idl.display()))?;
    let spec = orbweaver_idl::parse(&src).map_err(|e| format!("{}: {e}", idl.display()))?;
    let mut registry = Registry::new();
    registry.load(&spec).map_err(|e| e.to_string())?;
    check_stub_is_current(&registry, &root)?;

    let server = Orb::new().server("127.0.0.1:0", KEY.to_vec()).map_err(|e| e.to_string())?;
    let addr = server.local_addr().map_err(|e| e.to_string())?;
    let ior = server.ior(TYPE_ID, "127.0.0.1").map_err(|e| e.to_string())?;
    let home = ObjectHome::of(&server, "127.0.0.1").map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let serving = std::thread::spawn(move || {
        let mut objects = MeterObjects::new();
        objects.insert("", Echo);
        let mut skeleton = MeterSkeleton::new(MeterRefs::new(home), objects);
        server.serve(&mut skeleton, || flag.load(Ordering::SeqCst))
    });

    let outcome = measure(&ior, &registry, samples, warmup, tsv, max_ratio);

    // Ending the server the same way the wire tests do: raise the flag, then
    // hand the accept loop one throwaway connection so it wakes without a
    // spin. The join is what proves the server thread did not die mid-run.
    stop.store(true, Ordering::SeqCst);
    let _ = std::net::TcpStream::connect(addr);
    match serving.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(format!("the server ended badly: {e}")),
        Err(_) => return Err("the server thread panicked".to_owned()),
    }
    outcome
}

fn measure(
    ior: &Ior,
    registry: &Registry,
    samples: usize,
    warmup: usize,
    tsv: bool,
    max_ratio: Option<f64>,
) -> Result<std::process::ExitCode, String> {
    // One connection for both paths. The stub owns its invoker, so the dynamic
    // path borrows it back out of `client.conn` — which is the point: a second
    // socket would put a second set of TCP state under one of the two columns.
    let conn = Connection::connect(ior, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    let endian = orbweaver_giop::Invoker::endian(&conn);
    let mut client = MeterClient::new(conn);

    let mut shapes = shapes();
    let mut rows: Vec<(String, Series, Series)> = Vec::new();

    for shape in &mut shapes {
        let want = Answer::expected(&shape.payload);

        // Warm-up, and the correctness check that makes the timings mean
        // something: both paths answer, and both answer the same thing.
        for _ in 0..warmup.max(1) {
            let s = call_static(&mut client, &shape.payload)?;
            if s != want {
                return Err(format!("{}: the static path answered {s:?}", shape.label));
            }
            let d = call_dynamic(&mut client.conn, registry, shape.operation, &shape.payload)?;
            if d != want {
                return Err(format!("{}: the dynamic path answered {d:?}", shape.label));
            }
        }
        shape.request_bytes = request_body_bytes(&shape.payload, endian);

        let mut stat = Vec::with_capacity(samples);
        let mut dyna = Vec::with_capacity(samples);
        for i in 0..samples {
            // Interleaved, and the order swapped every iteration. Whatever the
            // machine does to one path during a run it does to the other, and
            // neither path is permanently the one that goes first — the first
            // of a pair pays for anything the second finds already warm.
            if i % 2 == 0 {
                stat.push(time_static(&mut client, &shape.payload, &want)?);
                dyna.push(time_dynamic(
                    &mut client,
                    registry,
                    shape.operation,
                    &shape.payload,
                    &want,
                )?);
            } else {
                dyna.push(time_dynamic(
                    &mut client,
                    registry,
                    shape.operation,
                    &shape.payload,
                    &want,
                )?);
                stat.push(time_static(&mut client, &shape.payload, &want)?);
            }
        }
        rows.push((shape.label.to_owned(), Series::new(stat), Series::new(dyna)));
    }

    let floor = clock_floor(samples.min(10_000));
    report(&shapes, &rows, &floor, samples, warmup, endian, tsv);

    if let Some(limit) = max_ratio {
        let mut over = Vec::new();
        for (label, s, d) in &rows {
            let ratio = d.pct(50.0) / s.pct(50.0);
            if ratio > limit {
                over.push(format!("{label}: {ratio:.2}x"));
            }
        }
        if !over.is_empty() {
            eprintln!("call-bench: over --max-ratio {limit}: {}", over.join(", "));
            return Ok(std::process::ExitCode::FAILURE);
        }
    }
    Ok(std::process::ExitCode::SUCCESS)
}

/// One timed static call. The timer opens before the argument exists, per
/// [`TIMED_REGION`].
fn time_static(
    client: &mut MeterClient<Connection>,
    p: &Payload,
    want: &Answer,
) -> Result<u64, String> {
    let t = Instant::now();
    let got = call_static(client, p)?;
    let ns = t.elapsed().as_nanos() as u64;
    if &got != want {
        return Err(format!("the static path answered {got:?}"));
    }
    Ok(ns)
}

/// One timed dynamic call, including the argument map it has to build.
fn time_dynamic(
    client: &mut MeterClient<Connection>,
    registry: &Registry,
    operation: &str,
    p: &Payload,
    want: &Answer,
) -> Result<u64, String> {
    let t = Instant::now();
    let got = call_dynamic(&mut client.conn, registry, operation, p)?;
    let ns = t.elapsed().as_nanos() as u64;
    if &got != want {
        return Err(format!("the dynamic path answered {got:?}"));
    }
    Ok(ns)
}

/// The request body a shape puts on the wire, so the report states volume
/// rather than implying it. Body only — the 12-byte GIOP header, the service
/// contexts and the object key are the same for every row here.
fn request_body_bytes(p: &Payload, endian: orbweaver_cdr::Endian) -> usize {
    use orbweaver_cdr::Encoder;
    let mut e = Encoder::new(endian);
    match p {
        Payload::Scalars(a, b) => {
            e.put_i32(*a);
            e.put_i32(*b);
        }
        Payload::Text(s) => e.put_str(s),
        Payload::Many(v) => {
            e.put_u32(v.len() as u32);
            for s in v {
                e.put_str(s);
            }
        }
    }
    e.finish().map(|b| b.len()).unwrap_or(0)
}

fn report(
    shapes: &[Shape],
    rows: &[(String, Series, Series)],
    floor: &Series,
    samples: usize,
    warmup: usize,
    endian: orbweaver_cdr::Endian,
    tsv: bool,
) {
    if tsv {
        println!("shape\tpath\tn\tmin_us\tp50_us\tp90_us\tp99_us\tmax_us\treq_bytes");
        for (shape, (label, s, d)) in shapes.iter().zip(rows) {
            for (path, series) in [("static", s), ("dynamic", d)] {
                println!(
                    "{label}\t{path}\t{}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{}",
                    series.n(),
                    series.min_us(),
                    series.pct(50.0),
                    series.pct(90.0),
                    series.pct(99.0),
                    series.max_us(),
                    shape.request_bytes,
                );
            }
        }
        println!(
            "clock floor\t—\t{}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t0",
            floor.n(),
            floor.min_us(),
            floor.pct(50.0),
            floor.pct(90.0),
            floor.pct(99.0),
            floor.max_us(),
        );
        return;
    }

    println!("call-bench — PLAN §8, dynamic path vs static stub");
    println!();
    println!("  transport      loopback TCP, one connection shared by both paths, TCP_NODELAY on");
    println!("  peer           our server, our generated skeleton, in this process");
    println!("  byte order     {endian:?} (native; a benchmark is not the endianness test)");
    println!("  samples        {samples} per path per shape, interleaved, order swapped each pair");
    println!("  warm-up        {warmup} discarded pairs per shape");
    println!("  trimming       {TRIMMING}");
    println!("  timed region   {TIMED_REGION}");
    println!("  load average   {}", load_average());
    println!();
    println!(
        "  {:<20} {:<8} {:>7} {:>9} {:>9} {:>9} {:>9} {:>8}",
        "shape", "path", "n", "min", "p50", "p90", "p99", "req B"
    );
    for (shape, (label, s, d)) in shapes.iter().zip(rows) {
        for (path, series) in [("static", s), ("dynamic", d)] {
            println!(
                "  {:<20} {:<8} {:>7} {:>8.1}µ {:>8.1}µ {:>8.1}µ {:>8.1}µ {:>8}",
                if path == "static" { label.as_str() } else { "" },
                path,
                series.n(),
                series.min_us(),
                series.pct(50.0),
                series.pct(90.0),
                series.pct(99.0),
                if path == "static" { shape.request_bytes.to_string() } else { String::new() },
            );
        }
    }
    println!();
    println!("  {:<20} {:>12} {:>12} {:>10}", "shape", "p50 added", "p50 ratio", "p99 added");
    for (label, s, d) in rows {
        println!(
            "  {:<20} {:>11.1}µ {:>11.2}x {:>9.1}µ",
            label,
            d.pct(50.0) - s.pct(50.0),
            d.pct(50.0) / s.pct(50.0),
            d.pct(99.0) - s.pct(99.0),
        );
    }
    println!();
    println!(
        "  clock floor    Instant::now() twice around nothing: p50 {:.3}µ, p99 {:.3}µ, max {:.3}µ",
        floor.pct(50.0),
        floor.pct(99.0),
        floor.max_us(),
    );
    println!();
    println!("  Not measured here, and worth saying so:");
    println!("    · a real LAN. This is loopback, so every figure omits a NIC, a switch and");
    println!("      a wire. §8 says LAN; the fixed cost of a LAN hop is added to *both*");
    println!("      columns equally, so the difference between the paths survives the move");
    println!("      and the absolute numbers do not.");
    println!("    · a foreign ORB. Both ends are ours. An omniORB peer would change the");
    println!("      absolute figures and not the comparison, which is between two clients.");
    println!("    · concurrency. One call at a time, one connection. Nothing here says what");
    println!("      either path does under load.");
    println!("    · §11's targets, as stated, cannot be compared to these. See the crate doc:");
    println!("      \"≤ 5 ms added and ≤ 3× static\" names no shape, no payload and no machine,");
    println!("      and on loopback its two clauses disagree by three orders of magnitude.");
}
