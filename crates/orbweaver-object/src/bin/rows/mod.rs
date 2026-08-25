//! `ROW` — the machine-readable channel a harness script keys its verdict on.
//!
//! A shell script cannot import a Rust constant, so for four releases the
//! harness told the classes apart by `grep -c`ing an exact `println!` sentence
//! out of a spike binary's log: `spikes/wide_rust.sh` matched
//! `"wchar is not legal at this version -> MARSHAL"` and
//! `"received bytes are not valid UTF-16"` — the second of which is not even
//! this binary's sentence, it is `orbweaver-giop`'s `NegotiationError`
//! `Display` two crates away — and `spikes/perm_fallback.sh` matched
//! `"forwarded ping()"` / `"served ping()"`. Reword any of them for clarity
//! and the count goes to zero and the gate goes red about nothing, or, worse,
//! a *new* sentence starts matching an old pattern and the gate goes green
//! about nothing.
//!
//! The fix is not a shared constant — there is no way to share one. It is the
//! shape `spikes/service_sweep.py` already uses: **one `#ROWS` header line and
//! then `ROW`-tagged tab-separated rows on the same stdout, mixed in with the
//! prose**, whose every column is a token from a closed set or a
//! fixed-format value, and never a sentence. The prose stays exactly as it
//! was — it is what a human reads when a gate goes red — but the *verdict*
//! stops depending on its wording.
//!
//! Ten columns after the tag, one tab between each (shown here as `·`, since
//! a real tab in a doc comment is a clippy lint):
//!
//! ```text
//! #ROWS·seat·event·op·giop·endian·codeset·n·sent·got·note
//! ROW·serve·served·echo_wchar·1.1·BE·0x00010109·7·-·U+D55C·-
//! ROW·call·skipped·echo_wchar·1.0·-·-·-·U+0077·-·version-illegal
//! ```
//!
//! * `seat` — [`SERVE`] or [`CALL`], which side of the wire printed it.
//! * `event` — one of the [`event`] tokens.
//! * `op` — the IDL operation, or `-`.
//! * `giop` — `1.0`, `1.1`, `1.2`, or `-`. Bare digits: `run_checks.sh` still
//!   greps the prose for `"first request at GIOP"`, and a row saying `GIOP`
//!   would silently join that count.
//! * `endian` — `BE`, `LE`, or `-`.
//! * `codeset` — the OSF registry id as `0x00010109`, or `-`.
//! * `n` — a decimal count: the request number on a `serve` row, the UTF-16
//!   unit count of a `wstring`, the failure count on a `verdict` row.
//! * `sent`, `got` — `U+XXXX` for one `wchar`, else `-`. An `ok` row is one
//!   where the two were equal, which is what the emitting code has just
//!   checked; a difference is a `fail` row.
//! * `note` — one of the [`note`] tokens, or `-`.
//!
//! A column is never free text. `op` is the one field a *peer* controls, so
//! it is sanitised at emit rather than trusted: a tab in an operation name
//! would otherwise shift every column after it.
//!
//! *하네스의 판정은 문장의 표현에 의존해서는 안 된다. 셸은 Rust 상수를 가져올 수
//! 없으므로 공유 상수가 아니라 기계가 읽는 채널이 답이다 —
//! `spikes/service_sweep.py`가 이미 쓰는 `ROW` 행 형식을 그대로 따른다. 산문은
//! 그대로 남는다; 판정만 산문에서 떨어져 나온다.*

// Each binary emits its own subset of this vocabulary; the point of the
// vocabulary is that it is complete and lives in one file, so an unused token
// here is the module working, not dead weight.
#![allow(dead_code)]

use std::borrow::Cow;

use orbweaver_cdr::Endian;
use orbweaver_giop::Version;
use orbweaver_giop::codeset::CodeSetId;

/// The column names, in order, without the leading tag.
pub const COLUMNS: &str = "seat\tevent\top\tgiop\tendian\tcodeset\tn\tsent\tgot\tnote";

/// The `seat` column: this process is serving.
pub const SERVE: &str = "serve";
/// The `seat` column: this process is calling.
pub const CALL: &str = "call";

/// The closed set of `event` tokens.
pub mod event {
    /// The first request seen at a given version and byte order.
    pub const FIRST: &str = "first";
    /// A request served here.
    pub const SERVED: &str = "served";
    /// A request refused here; `note` says why.
    pub const REFUSED: &str = "refused";
    /// A request answered with a `LOCATION_FORWARD`; `note` says which kind.
    pub const FORWARDED: &str = "forwarded";
    /// A call that round-tripped as sent.
    pub const OK: &str = "ok";
    /// A call that did not; `note` says how.
    pub const FAIL: &str = "fail";
    /// A case this seat declined to put on the wire; `note` says why.
    pub const SKIPPED: &str = "skipped";
    /// A case sent as raw octets past our own writer, and refused by the peer
    /// as it should be.
    pub const RAW_REFUSED: &str = "raw-refused";
    /// What the reference being dialled says about itself.
    pub const TARGET: &str = "target";
    /// The run's own verdict; `n` is the failure count.
    pub const VERDICT: &str = "verdict";
}

/// The closed set of `note` tokens.
pub mod note {
    /// The GIOP version forbids this type (§9.3.1.6: `wchar` under 1.0).
    pub const VERSION_ILLEGAL: &str = "version-illegal";
    /// The octets received are not a value in the negotiated codeset.
    pub const BAD_ENCODING: &str = "bad-encoding";
    /// The operation is not one this servant has.
    pub const BAD_OPERATION: &str = "bad-operation";
    /// The code unit asked for is not a character — a lone surrogate.
    pub const NOT_A_CHARACTER: &str = "not-a-character";
    /// The peer refused it with `MARSHAL` and OMG minor code 6, which is what
    /// §9.3.1.6 prescribes.
    pub const MARSHAL_OMG_6: &str = "marshal-omg-6";
    /// The peer refused it, but not with the status that was expected.
    pub const WRONG_EXCEPTION: &str = "wrong-exception";
    /// The peer answered where it should have refused.
    pub const NOT_REFUSED: &str = "not-refused";
    /// The reply decoded, but octets were left over.
    pub const OCTETS_LEFT: &str = "octets-left";
    /// The reply would not decode at all.
    pub const UNDECODABLE: &str = "undecodable";
    /// The call itself did not complete.
    pub const CALL_FAILED: &str = "call-failed";
    /// The reply decoded to a different value than was sent.
    pub const VALUE_DIFFERS: &str = "value-differs";
    /// An IIOP profile's advertised version.
    pub const PROFILE: &str = "profile";
    /// A `LOCATION_FORWARD` carrying the temporary status.
    pub const TEMPORARY: &str = "temporary";
    /// A `LOCATION_FORWARD_PERM` carrying the permanent status.
    pub const PERMANENT: &str = "permanent";
    /// The one-shot forward: emitted once, then the object is served here.
    pub const ONCE: &str = "once";
    /// Every case passed.
    pub const PASS: &str = "pass";
    /// At least one case did not.
    pub const FAILED: &str = "failed";
}

/// The header, printed once per process before anything else.
pub fn header() {
    println!("#ROWS\t{COLUMNS}");
}

/// One row. Build it with `..Row::default()` and fill only the columns that
/// apply; the rest are `-`.
pub struct Row<'a> {
    pub seat: &'a str,
    pub event: &'a str,
    pub op: &'a str,
    pub giop: &'a str,
    pub endian: &'a str,
    pub codeset: &'a str,
    pub n: &'a str,
    pub sent: &'a str,
    pub got: &'a str,
    pub note: &'a str,
}

impl Default for Row<'_> {
    fn default() -> Self {
        Row {
            seat: "-",
            event: "-",
            op: "-",
            giop: "-",
            endian: "-",
            codeset: "-",
            n: "-",
            sent: "-",
            got: "-",
            note: "-",
        }
    }
}

impl Row<'_> {
    /// The line, without its newline. Separated from [`Row::emit`] so a test
    /// can assert the exact bytes a harness will match.
    pub fn line(&self) -> String {
        format!(
            "ROW\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            tok(self.seat),
            tok(self.event),
            tok(self.op),
            tok(self.giop),
            tok(self.endian),
            tok(self.codeset),
            tok(self.n),
            tok(self.sent),
            tok(self.got),
            tok(self.note),
        )
    }

    /// Print the row. Flushed with the rest of stdout; a harness reads the
    /// file after the process has been stopped.
    pub fn emit(&self) {
        let line = self.line();
        #[cfg(test)]
        captured::push(&line);
        println!("{line}");
    }
}

/// Under `cfg(test)` every emitted row is also kept, so a test can call the
/// real `Dispatch` and assert the rows it produced rather than assert that a
/// row it built itself formats the way it built it. Nothing here exists in a
/// release binary.
#[cfg(test)]
pub mod captured {
    use std::cell::RefCell;

    thread_local! {
        static ROWS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn push(line: &str) {
        ROWS.with(|r| r.borrow_mut().push(line.to_owned()));
    }

    /// Every row emitted on this thread since the last call, and clear.
    pub fn drain() -> Vec<String> {
        ROWS.with(|r| std::mem::take(&mut *r.borrow_mut()))
    }
}

/// A column value with nothing in it that could be mistaken for a column
/// break. Borrows when there is nothing to replace, which is every row we
/// build ourselves; only a peer-supplied operation name ever allocates.
fn tok(s: &str) -> Cow<'_, str> {
    if s.is_empty() {
        return Cow::Borrowed("-");
    }
    if s.bytes().all(|b| b.is_ascii_graphic()) {
        return Cow::Borrowed(s);
    }
    Cow::Owned(s.chars().map(|c| if c.is_ascii_graphic() { c } else { '_' }).collect())
}

/// The `giop` column: bare digits, never the word `GIOP`.
pub fn giop(v: Version) -> String {
    format!("{}.{}", v.major, v.minor)
}

/// The `endian` column.
pub fn endian(e: Endian) -> &'static str {
    match e {
        Endian::Big => "BE",
        Endian::Little => "LE",
    }
}

/// The `codeset` column: the OSF registry id, no name — the name is prose and
/// prose is what this channel exists to stop depending on.
pub fn codeset(c: CodeSetId) -> String {
    format!("0x{:08X}", c.0)
}

/// The `sent` and `got` columns for one code unit.
pub fn unit(u: u32) -> String {
    format!("U+{u:04X}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header names as many columns as a row has fields, which is the one
    /// way the two can drift apart without anything failing to compile.
    #[test]
    fn the_header_and_a_row_have_the_same_column_count() {
        let row = Row::default().line();
        assert_eq!(
            COLUMNS.split('\t').count() + 1,
            row.split('\t').count(),
            "header {COLUMNS:?} vs row {row:?}"
        );
        assert_eq!(row, "ROW\t-\t-\t-\t-\t-\t-\t-\t-\t-\t-");
    }

    /// `op` is the one column a peer controls. A tab in an operation name
    /// would shift every column after it, which is a way to make a harness
    /// count the wrong thing by sending a request.
    #[test]
    fn a_peer_cannot_shift_the_columns() {
        let row = Row { op: "echo\twchar\nping", ..Default::default() }.line();
        assert_eq!(row.split('\t').count(), COLUMNS.split('\t').count() + 1);
        assert!(row.contains("echo_wchar_ping"), "{row}");
    }

    /// The `giop` column never says `GIOP`: `run_checks.sh` counts the prose
    /// line "first request at GIOP" and a row saying it would join that count.
    #[test]
    fn the_giop_column_is_bare_digits() {
        assert_eq!(giop(Version::V1_1), "1.1");
        assert!(!giop(Version::V1_2).contains("GIOP"));
    }

    #[test]
    fn the_codeset_column_is_the_registry_id_without_its_name() {
        assert_eq!(codeset(CodeSetId::UTF_16), "0x00010109");
        assert_eq!(endian(Endian::Big), "BE");
        assert_eq!(endian(Endian::Little), "LE");
        assert_eq!(unit(0xD55C), "U+D55C");
    }
}
