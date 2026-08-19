//! `corpus/evolution/union-default/` through the §5.3 gate — the IDL-level
//! pair for comparing a union's members by role rather than by position.
//!
//! `v1.0` is the release; `v1.0-default-first` is the same union with the
//! `default:` written first (a different member list and `default_index`,
//! the same encoding of every value — "no change"); `v1.1-retyped-default`
//! inserts a case ahead of the default and changes the default's type. Until
//! 2026-08-19 the differ compared members positionally, so the inserted case
//! shifted the default out from under the comparison and only the added case
//! was reported: the release was refused, for half the reason. The unit tests
//! in `src/diff.rs` hold the frozen-TypeCode half (a folded default against
//! an expanded one); this holds the half a person can produce from IDL.
//!
//! *`v1.0`은 릴리스, `v1.0-default-first`는 default만 앞에 쓴 같은 유니언
//! ("변경 없음"), `v1.1-retyped-default`는 default 앞에 case를 끼워 넣고
//! default의 타입을 바꾼 리비전이다. 위치로 비교하던 차분기는 후자에서
//! 끼워 넣은 case만 보고했다.*

use std::path::{Path, PathBuf};

use orbweaver_idl::SearchPath;
use orbweaver_registry::diff::{Verdict, diff};
use orbweaver_registry::{Registry, Strictness, registry_from_files};

fn pair(rel: &str) -> Registry {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("corpus/evolution/union-default")
        .join(rel)
        .join("payload.idl");
    registry_from_files(&[path], &SearchPath::new(), Strictness::Grammar)
        .unwrap_or_else(|e| panic!("{rel} must load: {e}"))
}

#[test]
fn the_default_written_first_is_the_same_release() {
    let c = diff(&pair("v1.0"), &pair("v1.0-default-first"));
    assert!(c.is_empty(), "member order is not on the wire: {c:#?}");
    let c = diff(&pair("v1.0-default-first"), &pair("v1.0"));
    assert!(c.is_empty(), "and not the other way either: {c:#?}");
}

#[test]
fn a_retyped_default_behind_an_inserted_case_is_named_not_only_the_case() {
    let c = diff(&pair("v1.0"), &pair("v1.1-retyped-default"));
    let text = c.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
    assert!(
        c.iter()
            .any(|x| x.verdict == Verdict::Breaking
                && x.what == "default member \"text\" changed type"),
        "the retyped default must be its own BREAKING finding, got:\n{text}"
    );
    assert!(
        c.iter().any(|x| x.verdict == Verdict::ConditionallyBreaking
            && x.what == "union case(s) added: [\"extra\"]"),
        "the inserted case is still reported, got:\n{text}"
    );
    assert_eq!(c.len(), 2, "two edits, two findings:\n{text}");
}
