//! Getting an estate into a registry: one translation unit per file, not one
//! file per file.
//!
//! # A file is not a translation unit, and the console used to think it was
//!
//! Every contract in `corpus/` is self-contained, so for as long as the console
//! was only ever pointed at the corpus, reading a file and parsing the string
//! was indistinguishable from resolving it. `spikes/estate/` is thirteen files
//! that include each other, and it is where the difference is a number:
//! nine of its twelve interfaces inherit `::MFS::Common::Describable`, which is
//! declared in `01-common.idl` and nowhere else. Parse each file as a string
//! and the base name resolves against a spec that does not contain it;
//! `Registry` drops a base it cannot resolve, so the interface arrives with no
//! ancestry at all. Measured on the estate: **58 operations catalogued where
//! the resolved surface is 76** — every `describe` and every `_get_label` an
//! agent could call was missing from the operator's page, and nothing on the
//! page said anything was missing.
//!
//! That is the worst shape a viewer can fail in. A page that refuses is a page
//! somebody fixes; a page that renders a smaller surface than the one that
//! exists is one an operator makes a decision on.
//!
//! # Resolving, not gating
//!
//! [`preprocess_file`] resolves `#include` — searching the including file's own
//! directory for the quoted form, then `search` — and injects the
//! `#pragma prefix` resets that keep an included file's prefix from escaping
//! into the includer's declarations. That last part is why this is worth
//! nothing less: a repository id is identity on the wire, and a catalogue of
//! ids no deployed object has would be worse than the missing operations.
//!
//! What this deliberately does **not** do is run semantic analysis. The
//! `orbweaver-mcp-server` loads through [`orbweaver_idl::check`] and says why —
//! a catalog built from IDL that S4 rejects would describe operations nobody
//! can call — but that server is a gate and this is a viewer. The single most
//! useful thing an operator can point this console at is a legacy contract
//! nobody has fixed yet; refusing to draw it until it passes S4 would answer
//! "what is in this estate?" with "fix it first". Syntax is the floor, because
//! text that does not parse has no interfaces to draw.
//!
//! # 파일 하나가 번역 단위 하나가 아니다
//!
//! 코퍼스는 전부 자기완결 파일이라 문자열 파싱과 인클루드 해석이 구분되지
//! 않았다. 13개 파일이 서로를 포함하는 `spikes/estate/`에서는 그 차이가 숫자로
//! 나온다: 상속 기반이 다른 파일에 있으면 `Registry`가 그 기반을 조용히 버리고,
//! 카탈로그는 76개 중 58개 오퍼레이션만 그렸다. 없는 것이 없다고 말하지도
//! 않았다. 여기서는 인클루드를 해석하되 의미 분석은 하지 않는다 — 콘솔은
//! 게이트가 아니라 뷰어이고, 아직 고치지 않은 레거시 계약을 그리는 것이 이
//! 화면의 존재 이유다.

use std::path::Path;

use orbweaver_idl::include::{SearchPath, preprocess_file};
use orbweaver_registry::Registry;

/// Resolves `path`'s includes and loads the whole unit into `registry`.
///
/// Returns the advice the resolver had about the unit — an include cycle, or a
/// re-inclusion a C preprocessor would have handled differently — already
/// formatted against the files they were written in. Advice is returned rather
/// than swallowed and rather than failed over: it is a fact about the estate,
/// and the caller decides where an operator reads it.
///
/// `Err` is a unit that cannot mean anything: the root file could not be read,
/// an `#include` resolved to nothing, or the spliced text does not parse. Every
/// diagnostic is rendered through [`orbweaver_idl::include::Unit::render`], so
/// it names the file and line somebody wrote and the `#include` chain that
/// reached it — not an offset into a spliced buffer nobody has.
pub fn load_into(
    registry: &mut Registry,
    path: &Path,
    search: &SearchPath,
) -> Result<Vec<String>, String> {
    let unit = preprocess_file(path, search).map_err(|e| format!("{}: {e}", path.display()))?;
    if !unit.errors.is_empty() {
        // Every error, not the first: one missing header is one cause, and a
        // batch of thirteen files wants the causes rather than the first file
        // that hit one.
        let all: Vec<String> = unit.errors.iter().map(|d| unit.render(d)).collect();
        return Err(all.join("\n"));
    }
    let advice = unit.advice.iter().map(|d| unit.render(d)).collect();
    let spec = orbweaver_idl::parse(&unit.text).map_err(|e| {
        unit.render(&orbweaver_idl::Diagnostic { message: e.message, span: e.span, rule: e.rule })
    })?;
    registry.load(&spec).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(advice)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/estate").join(name)
    }

    /// The estate's shape in two files: a shared header everybody includes, and
    /// a contract whose only ancestry is in that header.
    #[test]
    fn an_inherited_operation_declared_in_another_file_reaches_the_registry() {
        let mut registry = Registry::new();
        load_into(&mut registry, &fixture("depot.idl"), &SearchPath::new()).expect("resolves");
        let ops = orbweaver_mcp::resolved_operations(&registry, "IDL:DepotOps/StockControl:1.0");
        let names: Vec<&str> = ops.iter().map(|(n, _, _)| n.as_str()).collect();
        assert!(
            names.contains(&"describe"),
            "the base declared in common.idl is part of the callable surface: {names:?}"
        );
        assert!(names.contains(&"reconcile"), "{names:?}");
    }

    /// The regression this module exists for, stated as the difference it
    /// makes rather than as its own answer: parsing the file as a string sees
    /// strictly fewer operations than resolving it.
    #[test]
    fn parsing_the_file_alone_sees_a_smaller_surface_than_resolving_it() {
        let path = fixture("depot.idl");
        let text = std::fs::read_to_string(&path).expect("readable");
        let mut alone = Registry::new();
        // `parse` skips what it cannot see, which is exactly the old
        // behaviour: it succeeds, and it succeeds with the ancestry missing.
        alone.load(&orbweaver_idl::parse(&text).expect("parses")).expect("loads");
        let mut resolved = Registry::new();
        load_into(&mut resolved, &path, &SearchPath::new()).expect("resolves");

        let count = |r: &Registry| {
            orbweaver_mcp::resolved_operations(r, "IDL:DepotOps/StockControl:1.0").len()
        };
        assert!(
            count(&alone) < count(&resolved),
            "the string path saw {} of {} operations",
            count(&alone),
            count(&resolved)
        );
    }

    /// A file-scope `#pragma prefix` ends with its file. If it escaped into the
    /// includer, the ids on the page would be ids no deployed object has, and
    /// an operator allowlisting one would get a refusal that looks like a
    /// policy mistake.
    #[test]
    fn an_included_files_prefix_does_not_reach_the_includers_ids() {
        let mut registry = Registry::new();
        load_into(&mut registry, &fixture("depot.idl"), &SearchPath::new()).expect("resolves");
        let ids: Vec<&str> = registry.ids().map(String::as_str).collect();
        assert!(ids.contains(&"IDL:DepotOps/StockControl:1.0"), "{ids:?}");
        assert!(
            ids.contains(&"IDL:meridian.example/Common/Describable:1.0"),
            "the header keeps its own prefix: {ids:?}"
        );
        assert!(
            !ids.iter().any(|id| id.starts_with("IDL:meridian.example/DepotOps/")),
            "the header's prefix escaped into the includer: {ids:?}"
        );
    }

    /// An include that resolves to nothing is the cause, and the cause is what
    /// comes back — with the file and line it was written on.
    #[test]
    fn an_unresolvable_include_is_reported_against_the_line_that_wrote_it() {
        let mut registry = Registry::new();
        let err = load_into(&mut registry, &fixture("dangling.idl"), &SearchPath::new())
            .expect_err("the include resolves to nothing");
        assert!(err.contains("dangling.idl"), "{err}");
        assert!(err.contains("nowhere.idl"), "{err}");
    }
}
