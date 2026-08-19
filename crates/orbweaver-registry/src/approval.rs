//! The approval record behind `idl-diff --approve`: what it writes, what it
//! reads, and what makes a row apply to a diff.
//!
//! # Why a file and not a printed line
//!
//! Until this module existed `--approve <reason>` printed *"that is now a
//! decision on record"* and exited 0, and nothing was on record anywhere: no
//! file, no name, nothing a later run could find. `docs/PLAN.md` §5.3 says the
//! differ blocks an in-place edit *"unless the change carries an explicit
//! approval"* — carries, as in travels with. `forge-pipeline --supersede`
//! already does that half of the pattern for the S4 gate (`superseded.tsv`,
//! one row per waved-through change); this is the same shape for the release
//! gate, plus the two things a release gate additionally needs: **who**, and
//! **which bytes** the approval was given for.
//!
//! # The record
//!
//! A TSV beside the proposed contract, `<proposed>.approvals.tsv` by default,
//! one row per blocking finding:
//!
//! ```text
//! released  proposed  released_sha256  proposed_sha256  id  verdict  what  reason  approver  approved_at
//! ```
//!
//! *Beside the proposed file* because the approval is a property of the
//! proposed revision: it goes into version control in the same change as the
//! contract it approves, and the released side is by rule the thing nobody
//! edits. `superseded.tsv` sits beside the pipeline's output for the same
//! reason.
//!
//! **A row binds to bytes, not to a path.** `released_sha256` and
//! `proposed_sha256` are the SHA-256 of every file in the translation unit —
//! root first, then each included file in first-inclusion order, concatenated
//! — so a single-file contract's fingerprint is what `shasum -a 256` prints
//! for it and a shared header that changes changes the fingerprint of every
//! unit that includes it. Edit either side after approval and the row stops
//! applying; the gate says so and refuses again. An approval given for one
//! set of bytes is not an approval of the next edit.
//!
//! **A row names a person.** `approver` is required by `idl-diff` (its
//! `--approver`, or `ORBWEAVER_APPROVER`) and a store with a blank approver or
//! a blank reason is refused whole, exit 2: a decision with no name on it is
//! not a decision on record, and a store that has one such row cannot be
//! trusted about its others. There is no signature and no identity check —
//! this records who *said* they approved, which is what a chat log used to
//! hold and what a reviewer can now diff.
//!
//! **A re-run is byte-identical apart from `approved_at`.** Findings arrive in
//! [`crate::diff::diff`]'s stable worst-first order, every other column is a
//! function of the inputs, and a finding already covered for the same
//! fingerprints is not written twice. `SOURCE_DATE_EPOCH`, the reproducible-
//! builds convention, pins the timestamp too when a harness needs the whole
//! file identical.
//!
//! *승인은 출력이 아니라 기록이다. 행은 경로가 아니라 바이트에 묶이고, 이름 없는
//! 행이 하나라도 있으면 저장소 전체를 거부한다.*

use std::path::{Path, PathBuf};

use crate::diff::Change;

/// What the default store is called, appended to the proposed file's name:
/// `moe.idl` → `moe.idl.approvals.tsv`.
pub const APPROVALS_SUFFIX: &str = ".approvals.tsv";

/// The comment block a new store starts with. Fixed text, so two stores
/// created for the same diff are identical up to their rows.
const HEADER: &str = "\
# Approvals for breaking changes at the \u{a7}5.3 release gate (docs/PLAN.md \u{a7}5.3).
# Each row is one finding a deployed peer does not survive, accepted under the
# reason and the name beside it. A row binds to the bytes of both translation
# units (sha256 over every file in the unit, root first): edit either side and
# the row stops applying. Written by idl-diff --approve, read by idl-diff and
# the console's diff page. A blank approver or reason refuses the whole store.
# columns: released\tproposed\treleased_sha256\tproposed_sha256\tid\tverdict\twhat\treason\tapprover\tapproved_at
";

/// The default store for a proposed contract: `<proposed>.approvals.tsv`.
pub fn default_store(proposed: &Path) -> PathBuf {
    let mut name = proposed.file_name().map(|n| n.to_owned()).unwrap_or_default();
    name.push(APPROVALS_SUFFIX);
    proposed.with_file_name(name)
}

/// One approved finding, exactly as one row of the store holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    /// The released path as it was given to `idl-diff`. Informational: the
    /// match is on the fingerprint.
    pub released: String,
    /// The proposed path as given. Informational likewise.
    pub proposed: String,
    /// [`fingerprint`] of the released translation unit, lowercase hex.
    pub released_sha256: String,
    /// [`fingerprint`] of the proposed translation unit, lowercase hex.
    pub proposed_sha256: String,
    /// The finding's repository id.
    pub id: String,
    /// The finding's verdict label — `BREAKING` or `conditionally breaking`.
    pub verdict: String,
    /// The finding's text, [`Change::what`].
    pub what: String,
    /// Why somebody accepted it.
    pub reason: String,
    /// Who said so.
    pub approver: String,
    /// When, ISO 8601 UTC to the second.
    pub approved_at: String,
}

impl Approval {
    /// Whether this row is about `change` — same repository id, verdict and
    /// text — regardless of which bytes it was approved for.
    pub fn is_about(&self, change: &Change) -> bool {
        self.id == change.id && self.verdict == change.verdict.label() && self.what == change.what
    }

    /// Whether this row applies to `change` in a diff between the two units
    /// whose fingerprints these are.
    pub fn covers(&self, released_sha256: &str, proposed_sha256: &str, change: &Change) -> bool {
        self.is_about(change)
            && self.released_sha256 == released_sha256
            && self.proposed_sha256 == proposed_sha256
    }
}

/// The store, read whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    /// Where it was read from.
    pub path: PathBuf,
    /// Every row, in file order.
    pub approvals: Vec<Approval>,
}

impl Store {
    /// The row that approves `change` for exactly these two units, if any.
    pub fn covering(
        &self,
        released_sha256: &str,
        proposed_sha256: &str,
        change: &Change,
    ) -> Option<&Approval> {
        self.approvals.iter().find(|a| a.covers(released_sha256, proposed_sha256, change))
    }

    /// A row about `change` that was given for *other* bytes — the file has
    /// been edited since. The gate reports these so a refusal after an edit
    /// says why the approval that used to be there no longer counts.
    pub fn stale_for(
        &self,
        released_sha256: &str,
        proposed_sha256: &str,
        change: &Change,
    ) -> Option<&Approval> {
        self.approvals
            .iter()
            .find(|a| a.is_about(change) && !a.covers(released_sha256, proposed_sha256, change))
    }
}

/// Why a store could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The file system said no.
    Io {
        /// The store.
        path: PathBuf,
        /// What the OS said.
        message: String,
    },
    /// A row that is not a record: wrong column count, a blank approver, a
    /// blank reason, a fingerprint that is not one. The whole store is
    /// refused, not the row.
    Malformed {
        /// The store.
        path: PathBuf,
        /// 1-based line number.
        line: usize,
        /// What is wrong with it.
        message: String,
    },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io { path, message } => write!(f, "{}: {message}", path.display()),
            StoreError::Malformed { path, line, message } => write!(
                f,
                "{}:{line}: {message}\n  the approval store is refused whole: a row that is not a \
                 decision on record makes the rest unreadable as one",
                path.display()
            ),
        }
    }
}

impl std::error::Error for StoreError {}

/// Reads a store. `Ok(None)` if there is no file at `path` — nothing on record
/// is a valid state; a file that is there and wrong is not.
pub fn read_store(path: &Path) -> Result<Option<Store>, StoreError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(StoreError::Io { path: path.to_owned(), message: e.to_string() });
        }
    };
    let malformed = |line: usize, message: String| StoreError::Malformed {
        path: path.to_owned(),
        line,
        message,
    };
    let mut approvals = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        if raw.trim().is_empty() || raw.starts_with('#') {
            continue;
        }
        let cells: Vec<String> = raw.split('\t').map(unescape).collect();
        if cells.len() != 10 {
            return Err(malformed(line, format!("{} column(s), a row has 10", cells.len())));
        }
        let mut cells = cells.into_iter();
        let mut next = || cells.next().unwrap_or_default();
        let row = Approval {
            released: next(),
            proposed: next(),
            released_sha256: next(),
            proposed_sha256: next(),
            id: next(),
            verdict: next(),
            what: next(),
            reason: next(),
            approver: next(),
            approved_at: next(),
        };
        for (column, value) in
            [("released_sha256", &row.released_sha256), ("proposed_sha256", &row.proposed_sha256)]
        {
            if !is_sha256_hex(value) {
                return Err(malformed(line, format!("{column} is not a sha256 hex digest")));
            }
        }
        for (column, value) in [
            ("approver", &row.approver),
            ("reason", &row.reason),
            ("id", &row.id),
            ("verdict", &row.verdict),
            ("approved_at", &row.approved_at),
        ] {
            if value.trim().is_empty() {
                return Err(malformed(
                    line,
                    format!("{column} is blank; a decision with no {column} is not on record"),
                ));
            }
        }
        approvals.push(row);
    }
    Ok(Some(Store { path: path.to_owned(), approvals }))
}

/// Appends `rows` to the store at `path`, creating it with the header if it
/// does not exist. Appending nothing touches nothing.
pub fn append(path: &Path, rows: &[Approval]) -> Result<(), StoreError> {
    if rows.is_empty() {
        return Ok(());
    }
    let io = |e: std::io::Error| StoreError::Io { path: path.to_owned(), message: e.to_string() };
    let mut text = if path.exists() { String::new() } else { HEADER.to_owned() };
    for r in rows {
        let cells = [
            &r.released,
            &r.proposed,
            &r.released_sha256,
            &r.proposed_sha256,
            &r.id,
            &r.verdict,
            &r.what,
            &r.reason,
            &r.approver,
            &r.approved_at,
        ];
        let line: Vec<String> = cells.iter().map(|c| escape(c)).collect();
        text.push_str(&line.join("\t"));
        text.push('\n');
    }
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(io)?;
    file.write_all(text.as_bytes()).map_err(io)
}

/// A cell that cannot break a row: backslash, tab, newline and carriage return
/// become two-character escapes. Everything else is written as it is — a
/// repository id or a reason must read back as what was written.
fn escape(cell: &str) -> String {
    let mut out = String::with_capacity(cell.len());
    for c in cell.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

fn unescape(cell: &str) -> String {
    let mut out = String::with_capacity(cell.len());
    let mut chars = cell.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            // Not one of ours: keep both characters rather than drop one.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// The fingerprint of a translation unit: SHA-256 over the bytes of every file
/// in it, concatenated in `files` order (root first, then first-inclusion
/// order — [`orbweaver_idl::include::Unit::files`] as it stands).
///
/// One file: `shasum -a 256 <file>`. Several: `cat <files in order> | shasum
/// -a 256`. A person can check a row against the tree without this crate.
pub fn fingerprint<P: AsRef<Path>>(files: &[P]) -> Result<String, StoreError> {
    let mut sha = Sha256::new();
    for f in files {
        let bytes = std::fs::read(f.as_ref())
            .map_err(|e| StoreError::Io { path: f.as_ref().to_owned(), message: e.to_string() })?;
        sha.update(&bytes);
    }
    Ok(sha.finish_hex())
}

/// The current time as ISO 8601 UTC to the second, or the second named by
/// `SOURCE_DATE_EPOCH` when that is set and is a non-negative integer.
///
/// No clock crate: seconds since the epoch from `std`, the civil date by the
/// proleptic-Gregorian arithmetic every such conversion uses.
pub fn now_iso8601() -> String {
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        });
    iso8601_from_unix(secs)
}

/// `secs` since 1970-01-01T00:00:00Z as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn iso8601_from_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// SHA-256 as FIPS 180-4 writes it. First-party for the same reason the ORB
/// is: a published specification, ~80 lines, and no dependency for a
/// governance record to inherit a licence from.
struct Sha256 {
    state: [u32; 8],
    buffer: Vec<u8>,
    length: u64,
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Sha256 {
    fn new() -> Self {
        Sha256 {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: Vec::with_capacity(64),
            length: 0,
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        self.length = self.length.wrapping_add(bytes.len() as u64);
        self.buffer.extend_from_slice(bytes);
        let whole = self.buffer.len() / 64 * 64;
        let mut consumed = Vec::new();
        std::mem::swap(&mut consumed, &mut self.buffer);
        for block in consumed[..whole].chunks_exact(64) {
            self.compress(block);
        }
        self.buffer.extend_from_slice(&consumed[whole..]);
    }

    fn finish_hex(mut self) -> String {
        let bit_length = self.length.wrapping_mul(8);
        let mut tail = vec![0x80u8];
        while (self.buffer.len() + tail.len()) % 64 != 56 {
            tail.push(0);
        }
        tail.extend_from_slice(&bit_length.to_be_bytes());
        // `update` would count these bytes into `length`; feed them past it.
        self.buffer.extend_from_slice(&tail);
        let blocks = std::mem::take(&mut self.buffer);
        for block in blocks.chunks_exact(64) {
            self.compress(block);
        }
        let mut hex = String::with_capacity(64);
        for word in self.state {
            hex.push_str(&format!("{word:08x}"));
        }
        hex
    }

    fn compress(&mut self, block: &[u8]) {
        let mut w = [0u32; 64];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (s, v) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }
}

/// SHA-256 of `bytes`, lowercase hex. Exposed for tests and for a caller that
/// already holds the bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut s = Sha256::new();
    s.update(bytes);
    s.finish_hex()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::Verdict;

    /// FIPS 180-4 / RFC 6234 vectors: the empty string, "abc", and the
    /// 56-byte two-block case that catches padding mistakes.
    #[test]
    fn sha256_matches_the_published_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // A million 'a's, fed in uneven pieces: the streaming path.
        let mut s = Sha256::new();
        let a = vec![b'a'; 1_000_000];
        for chunk in a.chunks(7919) {
            s.update(chunk);
        }
        assert_eq!(
            s.finish_hex(),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn iso8601_dates_are_civil() {
        assert_eq!(iso8601_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_from_unix(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso8601_from_unix(1_787_097_600), "2026-08-19T00:00:00Z");
        assert_eq!(iso8601_from_unix(1_787_097_600 + 3661), "2026-08-19T01:01:01Z");
    }

    #[test]
    fn a_cell_round_trips_through_the_escape() {
        for s in ["plain", "tab\there", "line\nbreak", "back\\slash", "\\t literal", "trail\\"] {
            assert_eq!(unescape(&escape(s)), s, "{s:?}");
            assert!(!escape(s).contains('\t'), "{s:?}");
            assert!(!escape(s).contains('\n'), "{s:?}");
        }
    }

    fn change() -> Change {
        Change {
            id: "IDL:m/S:1.0".into(),
            what: "member \"a\" changed type".into(),
            why: "positional",
            verdict: Verdict::Breaking,
        }
    }

    fn row(released_sha: &str, proposed_sha: &str) -> Approval {
        Approval {
            released: "a.idl".into(),
            proposed: "b.idl".into(),
            released_sha256: released_sha.into(),
            proposed_sha256: proposed_sha.into(),
            id: "IDL:m/S:1.0".into(),
            verdict: "BREAKING".into(),
            what: "member \"a\" changed type".into(),
            reason: "v2 rollout, all peers rebuilt".into(),
            approver: "harness".into(),
            approved_at: "2026-08-19T00:00:00Z".into(),
        }
    }

    #[test]
    fn a_row_covers_only_the_bytes_it_was_given_for() {
        let r = sha256_hex(b"released");
        let p = sha256_hex(b"proposed");
        let store = Store { path: PathBuf::from("x"), approvals: vec![row(&r, &p)] };
        assert!(store.covering(&r, &p, &change()).is_some());
        assert!(store.stale_for(&r, &p, &change()).is_none());
        let edited = sha256_hex(b"proposed, edited");
        assert!(store.covering(&r, &edited, &change()).is_none());
        assert!(store.stale_for(&r, &edited, &change()).is_some());
        // A different finding, same bytes: not covered, not stale — unrelated.
        let mut other = change();
        other.what = "removed".into();
        assert!(store.covering(&r, &p, &other).is_none());
        assert!(store.stale_for(&r, &p, &other).is_none());
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orbweaver-approvals-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir.join(name)
    }

    #[test]
    fn a_store_round_trips_and_appends_without_a_second_header() {
        let path = scratch("round-trip.tsv");
        let _ = std::fs::remove_file(&path);
        assert_eq!(read_store(&path).expect("absent is fine"), None);
        let r = sha256_hex(b"r");
        let p = sha256_hex(b"p");
        let mut a = row(&r, &p);
        a.reason = "tab\tin the reason".into();
        append(&path, &[a.clone()]).expect("write");
        let mut b = row(&r, &p);
        b.what = "operation \"x\" removed".into();
        append(&path, &[b.clone()]).expect("append");
        let text = std::fs::read_to_string(&path).expect("read");
        assert_eq!(text.matches("# columns:").count(), 1, "{text}");
        let store = read_store(&path).expect("reads").expect("exists");
        assert_eq!(store.approvals, vec![a, b]);
        let _ = std::fs::remove_file(&path);
    }

    /// The negative control: blank the approver in one row and the whole store
    /// is refused, naming the line.
    #[test]
    fn a_blank_approver_refuses_the_whole_store() {
        let path = scratch("blank-approver.tsv");
        let r = sha256_hex(b"r");
        let p = sha256_hex(b"p");
        append(&path, &[row(&r, &p)]).expect("write");
        let text = std::fs::read_to_string(&path).expect("read").replace("\tharness\t", "\t\t");
        std::fs::write(&path, text).expect("rewrite");
        match read_store(&path) {
            Err(StoreError::Malformed { line, message, .. }) => {
                assert!(message.contains("approver is blank"), "{message}");
                assert!(line > 7, "row lines follow the header: {line}");
            }
            other => panic!("a blank approver must refuse the store, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_short_row_or_a_bad_digest_refuses_the_store() {
        let path = scratch("bad-row.tsv");
        std::fs::write(&path, "# c\na\tb\tc\n").expect("write");
        assert!(matches!(read_store(&path), Err(StoreError::Malformed { line: 2, .. })));
        let mut r = row("nothex", &sha256_hex(b"p"));
        r.released_sha256 = "NOTHEX".repeat(11)[..64].to_owned();
        let _ = std::fs::remove_file(&path);
        append(&path, &[r]).expect("write");
        match read_store(&path) {
            Err(StoreError::Malformed { message, .. }) => {
                assert!(message.contains("released_sha256"), "{message}");
            }
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_default_store_sits_beside_the_proposed_file() {
        assert_eq!(
            default_store(Path::new("corpus/evolution/moe/v1.1-in-place/moe.idl")),
            PathBuf::from("corpus/evolution/moe/v1.1-in-place/moe.idl.approvals.tsv")
        );
    }

    #[test]
    fn a_fingerprint_is_the_shasum_of_the_concatenation() {
        let a = scratch("fp-a.idl");
        let b = scratch("fp-b.idl");
        std::fs::write(&a, b"module a {};").expect("a");
        std::fs::write(&b, b"module b {};").expect("b");
        assert_eq!(fingerprint(&[&a]).unwrap(), sha256_hex(b"module a {};"));
        assert_eq!(fingerprint(&[&a, &b]).unwrap(), sha256_hex(b"module a {};module b {};"));
    }
}
