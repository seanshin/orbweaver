//! S3i end to end: ingest through the facade, propose, mark, round-trip, and
//! be refused at the exposure gate.
//!
//! Every deterministic half of the stage is measured here — the ingestion, the
//! marking, the round trip through a real [`Registry`], the refusal, and the
//! promotion a named human performs. The model-facing half is **not** measured
//! here and is not faked here: the producer in these tests is a scripted stand-
//! in exercising the machinery, and the numbers a stand-in produces are numbers
//! about the machinery. A model batch's first-pass rate and unknown rate belong
//! in `docs/pipeline-runs/`, beside the prompt that produced them.
//!
//! The load-bearing assertion of the file is
//! `an_inferred_scope_enforces_nothing_at_the_policy_gate`. It is not a
//! demonstration of a weakness — it is the *proof of the design*, measured
//! against the real `orbweaver-mcp` gate rather than assumed: an inference
//! cannot gate anything, so the refusal has to happen before exposure, and
//! [`exposure_refusal`] is where it happens.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use orbweaver_forge::infer::{
    self, Approval, ApproveError, InferStage, Proposal, Provenance, UNAPPROVED, exposure_refusal,
    provenance, subjects, worksheet,
};
use orbweaver_forge::pipeline::{ItemStatus, Stage, run_batch};
use orbweaver_giop::Ior;
use orbweaver_giop::orb::Orb;
use orbweaver_mcp::policy::{Approval as CallApproval, Denied, Exposure};
use orbweaver_registry::ifr::{RepositoryServer, interface_ids};
use orbweaver_registry::ingest::{Limits, ingest};
use orbweaver_registry::{Entry, Origin, Registry};

const IDL: &str = "
module legacy {
  struct Track { string label; double bearing; };
  interface TrackManager {
    Track get(in string id);
    void update(in string id, in Track replacement);
    void delete_all();
    long process(in string payload);
    oneway void drop(in string id);
  };
};";

fn tmp(name: &str) -> PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("forge-infer-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

const SUBJECT_ID: &str = "IDL:legacy/TrackManager:1.0";

/// The golden path this whole module is about: IDL that exists *somewhere
/// else*, served over a real socket by the IR facade, and ingested with no
/// file on our side. The entries come back `Origin::Ingested` carrying an
/// empty annotation map, because the wire carries no annotations.
fn ingested() -> Registry {
    let spec = orbweaver_idl::check(IDL).expect("the fixture IDL is valid");
    let mut local = Registry::new();
    local.load(&spec).expect("loads");
    let seeds = interface_ids(&local);

    let server = Orb::new().server("127.0.0.1:0", b"IR".to_vec()).expect("binds");
    let port = server.local_addr().expect("has an address").port();
    let mut facade = RepositoryServer::new("127.0.0.1", port, b"IR".to_vec(), local);
    let root: Ior = facade.root_ior();
    std::thread::spawn(move || {
        let _ = server.serve(&mut facade, || false);
    });

    let mut registry = Registry::new();
    let report = ingest(
        &mut registry,
        &root,
        &seeds,
        "ifr://legacy-estate",
        &Limits::default(),
        Duration::from_secs(10),
    )
    .expect("the facade is reachable");
    assert!(report.refused.is_empty(), "{:?}", report.refused);
    assert_eq!(
        registry.origin(SUBJECT_ID),
        Some(Origin::Ingested("ifr://legacy-estate".into())),
        "the entry must carry its provenance or nothing downstream can ask"
    );
    registry
}

/// A proposal a well-behaved producer returns for the fixture.
fn proposal_text(id: &str) -> String {
    format!(
        r#"{{"id":"{id}","source":"ifr://legacy-estate","inferences":[
      {{"operation":"get","desc":"Returns a Track for an identifier.","effect":"unknown",
        "authz":null,"evidence":"the name contains 'get' and it returns a Track"}},
      {{"operation":"update","desc":"Replaces the Track stored under an identifier.",
        "effect":"destructive","authz":"legacy.tracks.write",
        "evidence":"the name contains 'update'"}},
      {{"operation":"delete_all","desc":"Removes every Track.","effect":"destructive",
        "authz":"legacy.tracks.admin","evidence":"the name contains 'delete'"}},
      {{"operation":"process","desc":"Takes a payload string and returns a long.",
        "effect":"unknown","authz":null,
        "evidence":"the name 'process' and the parameter 'payload' say nothing about effect"}},
      {{"operation":"drop","desc":"Asks that a Track be removed, with no reply.",
        "effect":"destructive","authz":"legacy.tracks.write",
        "evidence":"the name contains 'drop' and the operation is oneway"}}
    ]}}"#
    )
}

/// A scripted producer. Exercises the machinery and measures nothing about a
/// model, which is why the numbers it yields are never quoted as stage numbers.
struct Scripted {
    reply: Reply,
    calls: usize,
}

/// The scripted producer's body: subject text in, artifact or producer error
/// out — the same signature [`Stage::produce`] has.
type Reply = Box<dyn FnMut(&str) -> Result<String, String> + Send>;

impl Stage for Scripted {
    fn id(&self) -> orbweaver_forge::pipeline::StageId {
        orbweaver_forge::pipeline::StageId::Annotate
    }

    fn produce(&mut self, input: &str, _repair: Option<&str>) -> Result<String, String> {
        self.calls += 1;
        (self.reply)(input)
    }

    fn gate(&self, input: &str, output: &str) -> orbweaver_forge::Report {
        infer::gate(input, output)
    }
}

// ── the batch, over interfaces that came off a socket ────────────────────────

#[test]
fn the_stage_runs_as_a_batch_over_ingested_interfaces() {
    let registry = ingested();
    let subjects = subjects(&registry);
    assert_eq!(subjects.len(), 1, "{subjects:?}");
    let items: Vec<(String, String)> =
        subjects.iter().map(|s| (s.id.clone(), s.to_json().to_string())).collect();

    let mut stage = Scripted {
        reply: Box::new(|input| {
            let s = orbweaver_forge::infer::Subject::parse(input)?;
            Ok(proposal_text(&s.id))
        }),
        calls: 0,
    };
    let batch = run_batch(&mut stage, &items, 3);
    assert!(batch.all_valid(), "{batch}");
    assert_eq!(batch.first_pass_valid, 1);
    assert_eq!(batch.rounds_used, 1);

    let output = batch.items[0].output.as_deref().expect("an artifact");
    let proposal = Proposal::parse(output).expect("parses");
    // Two of five names say nothing the checker can read; the stage said so
    // rather than filling the field in.
    assert!((proposal.unknown_rate() - 0.4).abs() < 1e-9, "{}", proposal.unknown_rate());
}

/// A dishonest proposal is caught, the repair prompt names the cause once, and
/// the second round converges — the §5.1 loop, over this stage.
#[test]
fn a_dishonest_first_round_is_repaired_by_the_gates_own_diagnostics() {
    let registry = ingested();
    let items: Vec<(String, String)> =
        subjects(&registry).iter().map(|s| (s.id.clone(), s.to_json().to_string())).collect();

    // Round 1 claims read_only on `process`, which is the failure mode the
    // whole design exists to prevent: a guess that REMOVES the approval gate.
    let first = proposal_text(SUBJECT_ID).replace(
        r#""desc":"Takes a payload string and returns a long.",
        "effect":"unknown""#,
        r#""desc":"Takes a payload string and returns a long.",
        "effect":"read_only""#,
    );
    assert_ne!(first, proposal_text(SUBJECT_ID), "the fixture edit must apply");
    let mut round = 0usize;
    let mut scripted = Scripted {
        reply: Box::new(move |_| {
            round += 1;
            Ok(if round == 1 { first.clone() } else { proposal_text(SUBJECT_ID) })
        }),
        calls: 0,
    };

    let batch = run_batch(&mut scripted, &items, 3);
    assert_eq!(batch.first_pass_valid, 0, "round 1 must fail: {batch}");
    assert_eq!(batch.affected(0, "si/ungating-claim"), [SUBJECT_ID.to_owned()]);
    assert!(batch.all_valid(), "round 2 must converge: {batch}");
    assert_eq!(batch.rounds_used, 2);
}

/// A producer failure is a producer failure, never "the model was dishonest".
#[test]
fn a_producer_error_is_counted_under_its_own_cause() {
    let registry = ingested();
    let items: Vec<(String, String)> =
        subjects(&registry).iter().map(|s| (s.id.clone(), s.to_json().to_string())).collect();
    let mut stage = Scripted { reply: Box::new(|_| Err("model API unavailable".into())), calls: 0 };
    let batch = run_batch(&mut stage, &items, 2);
    assert_eq!(batch.affected(0, "producer-error"), [SUBJECT_ID.to_owned()]);
    assert!(
        matches!(&batch.items[0].status, ItemStatus::Error { message } if message == "model API unavailable")
    );
}

/// The command seam, without a model: a wrapper that ignores its input still
/// gets its prompt on disk and its output gated.
#[test]
fn the_command_seam_hands_the_stage_prompt_to_the_wrapper() {
    let dir = tmp("cmd");
    let script = dir.join("producer.sh");
    let out = dir.join("seen-prompt.txt");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\ncp \"$FORGE_PROMPT\" '{}'\nprintf '%s' '{}'\n",
            out.display(),
            proposal_text(SUBJECT_ID).replace('\n', " ")
        ),
    )
    .expect("writes");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    let registry = ingested();
    let items: Vec<(String, String)> =
        subjects(&registry).iter().map(|s| (s.id.clone(), s.to_json().to_string())).collect();
    let mut stage = InferStage::new(script.to_string_lossy().into_owned(), dir.join("scratch"));
    let batch = run_batch(&mut stage, &items, 1);
    assert!(batch.all_valid(), "{batch}");

    let seen = std::fs::read_to_string(&out).expect("the wrapper received a prompt");
    assert_eq!(seen, infer::S3I_PROMPT, "the prompt is the crate's, versioned with the checker");
}

// ── marking, round-tripping, and the gate that reads none of it ──────────────

/// The round trip: proposals become annotations on a real registry entry, and
/// come back out of `resolve_operation` with their marks intact.
#[test]
fn marks_round_trip_through_the_registry() {
    let registry = ingested();
    let proposal = Proposal::parse(&proposal_text(SUBJECT_ID)).expect("parses");
    let annotated = with_proposal(&registry, &proposal);

    let (owner, sig) = annotated.resolve_operation(SUBJECT_ID, "update").expect("resolves");
    assert_eq!(owner, SUBJECT_ID);
    assert_eq!(sig.annotations["inferred_effect"], "destructive");
    assert_eq!(sig.annotations["inferred_authz"], "legacy.tracks.write");
    assert_eq!(sig.annotations[infer::MARK_STATUS], UNAPPROVED);
    assert_eq!(sig.annotations[infer::MARK_SOURCE], "ifr://legacy-estate");
    assert!(sig.annotations[infer::MARK_EVIDENCE].contains("update"));
    assert_eq!(provenance(&sig.annotations), Provenance::InferredUnapproved);

    // And the entry is still an ingested one: annotating did not launder it.
    assert_eq!(annotated.origin(SUBJECT_ID), Some(Origin::Ingested("ifr://legacy-estate".into())));
    assert!(annotated.touches_ingested(SUBJECT_ID));

    // Nothing an inference wrote is in a key a gate reads.
    for sig in annotated.interface(SUBJECT_ID).expect("there").operations.values() {
        for key in sig.annotations.keys() {
            assert!(!key.starts_with("ai_"), "{key} would be read as an authored contract");
        }
    }
}

/// **The proof of the design.** An inferred scope is not a scope: the MCP
/// policy gate reads `ai_authz`, the inference wrote `inferred_authz`, and the
/// call goes through with no caller and no approval.
///
/// This is measured against the real gate rather than assumed, because the
/// entire argument for refusing exposure upstream rests on it. If a future
/// change made `inferred_authz` enforceable, the design would be *worse*, not
/// better: the bridge would be enforcing a permission name a model invented
/// about somebody else's service, and this test would go red and say so.
#[test]
fn an_inferred_scope_enforces_nothing_at_the_policy_gate() {
    let registry = ingested();
    let proposal = Proposal::parse(&proposal_text(SUBJECT_ID)).expect("parses");
    let annotated = with_proposal(&registry, &proposal);

    let exposure = Exposure::nothing().allow_interface(SUBJECT_ID);
    let verdict =
        exposure.check_call(&annotated, SUBJECT_ID, "delete_all", CallApproval::default(), None);
    // An ingested contract carries no SIDL, so the effect gate stops it — which
    // is *not* the inference gating anything, and the distinction is the whole
    // property. The refusal must never be MissingScope or NeedsApproval,
    // because either would mean a model-invented permission name had become
    // enforceable. This used to assert `is_ok()`, which said the same thing
    // more weakly: back then nothing stopped the call at all.
    assert!(
        matches!(verdict, Err(Denied::EffectUnstated { .. })),
        "an inferred annotation must not gate anything; if this is MissingScope or \
         NeedsApproval the marks have leaked into ai_* keys: {verdict:?}"
    );

    // Which is exactly why the refusal has to happen before the allowlist.
    let why = exposure_refusal(&annotated, SUBJECT_ID).expect("refused");
    assert!(why.contains("enforces nothing"), "{why}");
    assert!(why.contains("delete_all"), "{why}");
}

/// [`infer::UNGATING`] is a copy of a set that lives in another crate, and this
/// runs the gate rather than reading it.
///
/// The set's home is `orbweaver-mcp`'s `policy::is_harmless`, which is
/// **private**, so no constant can be shared and no comment can be compiled.
/// Until 2026-08-25 the copy's only pin was against `annotate`'s copy — two
/// literals in one crate agreeing with each other — while the doc beside both
/// of them named a `policy::destructive_effect` that had been gone since
/// 2026-08-14. A pin whose scope is narrower than its fact's stays green over
/// the drift, and the fact here is workspace-scoped, so the pin has to cross
/// the boundary the only way a private predicate leaves one: through what the
/// gate *does*.
///
/// The asymmetry S3i is built on is exactly this — an ungating value removes
/// the approval requirement — so if a value ever left the set, an inference
/// would still be forbidden to propose it for a reason that had stopped being
/// true, and nothing else would notice.
#[test]
fn the_ungating_set_is_what_the_gate_lets_through() {
    let exposure = Exposure::nothing().allow_interface("IDL:gate/I:1.0");
    let with_effect = |effect: &str| {
        let idl =
            format!("module gate {{ interface I {{ //@ ai_effect: {effect}\n void f(); }}; }};");
        let spec = orbweaver_idl::check(&idl).expect("the fixture IDL is valid");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        exposure.check_call(&r, "IDL:gate/I:1.0", "f", CallApproval::default(), None)
    };

    for value in infer::UNGATING {
        assert!(
            with_effect(value).is_ok(),
            "`{value}` is in UNGATING because the gate needs no approval for it — and the gate \
             now asks for one, so the asymmetry S3i refuses to propose it under has changed"
        );
    }
    // The other half, or the assertion above would pass over a gate that had
    // stopped refusing anything at all.
    for value in ["destructive", "moves_money"] {
        assert!(
            matches!(with_effect(value), Err(Denied::NeedsApproval { .. })),
            "`{value}` is not ungating and must still need a human"
        );
    }
}

/// An approval is a human act with a name on it, and it is the only thing that
/// moves a value into a key the gate reads.
#[test]
fn approval_is_the_only_transition_and_it_leaves_the_mark_behind() {
    let registry = ingested();
    let proposal = Proposal::parse(&proposal_text(SUBJECT_ID)).expect("parses");
    let annotated = with_proposal(&registry, &proposal);

    // Approve one operation, the way an operator working the worksheet would.
    let mut iface = annotated.interface(SUBJECT_ID).expect("there").clone();
    let sig = iface.operations.get_mut("delete_all").expect("there");
    infer::approve(
        &mut sig.annotations,
        &Approval { by: "ops-lead@example".into(), at: "2026-08-14".into() },
    )
    .expect("approved");

    let mut approved = Registry::new();
    approved
        .define_ingested(SUBJECT_ID.to_owned(), Entry::Interface(iface), "ifr://legacy-estate")
        .expect("registers");

    // Now the gate sees it — and it sees `destructive`, so it demands a human.
    let exposure = Exposure::nothing().allow_interface(SUBJECT_ID);
    let verdict =
        exposure.check_call(&approved, SUBJECT_ID, "delete_all", CallApproval::default(), None);
    assert!(verdict.is_err(), "an approved destructive claim must reach the gate: {verdict:?}");

    let sig = &approved.interface(SUBJECT_ID).expect("there").operations["delete_all"];
    assert_eq!(sig.annotations["ai_effect"], "destructive");
    assert_eq!(sig.annotations["ai_authz"], "legacy.tracks.admin");
    // The mark survives the promotion, everywhere it travels.
    assert_eq!(sig.annotations["inferred_effect"], "destructive");
    assert!(sig.annotations[infer::MARK_EVIDENCE].contains("delete"));
    match provenance(&sig.annotations) {
        Provenance::InferredApproved(who) => {
            assert!(who.contains("ops-lead@example") && who.contains("2026-08-14"), "{who}");
        }
        other => panic!("{other:?}"),
    }

    // The un-approved operations are still listed, so a partial approval does
    // not read as a finished one.
    let still = infer::unapproved(&approved);
    assert!(still.iter().all(|b| b.operation != "delete_all"), "{still:?}");
    assert!(still.iter().any(|b| b.operation == "process"), "{still:?}");
}

#[test]
fn an_anonymous_approval_moves_nothing() {
    let mut ann = BTreeMap::from([
        ("inferred_effect".to_owned(), "destructive".to_owned()),
        (infer::MARK_STATUS.to_owned(), UNAPPROVED.to_owned()),
    ]);
    assert_eq!(
        infer::approve(&mut ann, &Approval { by: String::new(), at: "2026-08-14".into() }),
        Err(ApproveError::NoApprover)
    );
    assert!(!ann.contains_key("ai_effect"));
}

/// The visibility requirement, as a measurement: an ingested operation nobody
/// has proposed anything for still gets a row. Default-off would show nothing
/// here, which is the difference this file is about.
#[test]
fn the_un_approved_state_is_a_row_and_not_an_absent_field() {
    let registry = ingested();
    let sheet = worksheet(&registry);
    for op in ["get", "update", "delete_all", "process", "drop"] {
        let line = sheet
            .lines()
            .find(|l| l.starts_with(SUBJECT_ID) && l.contains(&format!("\t{op}\t")))
            .unwrap_or_else(|| panic!("{op} has no row:\n{sheet}"));
        assert!(line.contains("approved=no"), "{line}");
        assert!(line.contains("no annotation at all"), "{line}");
    }

    // With proposals, the rows carry them — still `approved=no`.
    let proposal = Proposal::parse(&proposal_text(SUBJECT_ID)).expect("parses");
    let sheet = worksheet(&with_proposal(&registry, &proposal));
    let line = sheet.lines().find(|l| l.contains("\tdelete_all\t")).expect("a row");
    assert!(line.contains("destructive"), "{line}");
    assert!(line.contains("legacy.tracks.admin"), "{line}");
    assert!(line.contains("approved=no"), "{line}");
}

/// An ingested entry carrying `ai_*` keys with no provenance mark is the one
/// shape this design forbids, and it must be reported rather than trusted —
/// a remote description cannot be allowed to wear a reviewed contract's
/// clothes just because somebody wrote the keys by hand.
#[test]
fn unmarked_gate_keys_on_an_ingested_entry_are_reported() {
    let registry = ingested();
    let mut iface = registry.interface(SUBJECT_ID).expect("there").clone();
    iface
        .operations
        .get_mut("delete_all")
        .expect("there")
        .annotations
        .insert("ai_effect".to_owned(), "read_only".to_owned());
    let mut forged = Registry::new();
    forged
        .define_ingested(SUBJECT_ID.to_owned(), Entry::Interface(iface), "ifr://legacy-estate")
        .expect("registers");

    let blocker = infer::unapproved(&forged)
        .into_iter()
        .find(|b| b.operation == "delete_all")
        .expect("still listed");
    assert!(blocker.why.contains("no provenance mark"), "{}", blocker.why);
    assert!(exposure_refusal(&forged, SUBJECT_ID).is_some(), "still refused for exposure");
}

/// Registering the annotated entry rebuilds rather than patches, because
/// `define_ingested` refuses to replace anything — and that refusal is
/// load-bearing, so this asserts it rather than working around it silently.
fn with_proposal(registry: &Registry, proposal: &Proposal) -> Registry {
    let iface = registry.interface(&proposal.id).expect("an ingested interface");
    let annotated = infer::apply(iface, proposal);

    let mut fresh = Registry::new();
    for id in registry.ids().cloned().collect::<Vec<_>>() {
        let entry = match registry.get(&id) {
            Some(Entry::Interface(_)) if id == proposal.id => Entry::Interface(annotated.clone()),
            Some(other) => other.clone(),
            None => continue,
        };
        fresh.define_ingested(id, entry, "ifr://legacy-estate").expect("a fresh registry is empty");
    }
    assert!(
        fresh
            .define_ingested(
                SUBJECT_ID.to_owned(),
                Entry::Interface(annotated),
                "ifr://legacy-estate"
            )
            .is_err(),
        "the registry must refuse to have an ingested entry replaced"
    );
    fresh
}
