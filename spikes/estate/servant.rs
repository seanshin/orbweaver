//! One object of the estate, served: `MFS::Tracking::ShipmentTracker`.
//!
//! The estate has twelve exposable interfaces and seventy-six operations. This
//! serves **one** of them, deliberately, because the question the pilot asks at
//! this stage is not "can we serve twelve" — the answer to that is the same
//! answer as for one, and `spikes/service_sweep.sh` already measures a
//! multi-servant process. The question is whether a servant written against a
//! skeleton generated from a *legacy* contract — one nobody wrote for us, with
//! eleven operations, an inherited attribute and two user exceptions — compiles
//! and answers.
//!
//! `ShipmentTracker` is the interface an operator would actually expose to an
//! agent: it is the estate's read-heavy one, it inherits `Describable` (so the
//! skeleton has to dispatch an inherited attribute accessor), and it declares
//! both `UnknownConsignment` and `NotAuthorised`, so the fault enum has more
//! than one arm to get wrong.
//!
//! ```text
//! estate-servant <ior-output-file>
//! ```
//!
//! Serves until killed. The driver captures the PID and kills it by that;
//! nothing here polls for a stop file.

use std::collections::BTreeMap;

use orbweaver_gen::rt::{Dispatch, DispatchBody, Encoder, Request, Server, SystemException};
use orbweaver_genout::f_ESTATE::MFS::Common::{NotAuthorised, Priority};
use orbweaver_genout::f_ESTATE::MFS::Tracking::{
    Consignment, ScanEvent, ScanHistory, ShipmentTrackerFault, ShipmentTrackerRefs,
    ShipmentTrackerServant, ShipmentTrackerSkeleton, ShipmentTrackerTarget, UnknownConsignment,
};
use orbweaver_giop::orb::Orb;
use orbweaver_object::{ObjectId, OrbPoa, Poa, Target};

/// The repository id the estate's own files produce. Not typed from memory:
/// it is the row `exposure.todo.tsv` carries, and the driver greps for it.
const TYPE_ID: &str = "IDL:meridian.com/MFS/Tracking/ShipmentTracker:1.0";

// ── The hand-written half ────────────────────────────────────────────────────

/// A depot's tracking book. Nothing here mentions GIOP, CDR, an operation name
/// or a repository id.
struct Tracking {
    book: BTreeMap<u32, Consignment>,
    scans: BTreeMap<u32, ScanHistory>,
}

impl Tracking {
    fn new() -> Self {
        let mut book = BTreeMap::new();
        book.insert(
            8801,
            Consignment {
                reference: 8801,
                origin: "LHR2".to_owned(),
                destination: "BRU".to_owned(),
                level: Priority::RUSH,
                promised: "2026-08-16T09:00:00Z".to_owned(),
                delivered: false,
            },
        );
        book.insert(
            8802,
            Consignment {
                reference: 8802,
                origin: "MAN".to_owned(),
                destination: "LHR2".to_owned(),
                level: Priority::NORMAL,
                promised: "2026-08-15T17:30:00Z".to_owned(),
                delivered: true,
            },
        );
        let mut scans = BTreeMap::new();
        scans.insert(
            8801u32,
            vec![ScanEvent {
                occurred_at: "2026-08-14T06:12:00Z".to_owned(),
                depot: "LHR2".to_owned(),
                scan_code: "DEP".to_owned(),
                note: "loaded, bay 12".to_owned(),
            }],
        );
        scans.insert(8802u32, Vec::new());
        Self { book, scans }
    }

    fn get(&self, reference: u32) -> Result<&Consignment, ShipmentTrackerFault> {
        self.book
            .get(&reference)
            .ok_or(ShipmentTrackerFault::UnknownConsignment(UnknownConsignment { reference }))
    }

    /// Everything that changes state answers the same way, because this process
    /// holds no caller identity at all. That is not laziness: the estate's IDL
    /// declares `NotAuthorised` on exactly these operations and declares no way
    /// to become authorised, which is what a contract written in 2003 against a
    /// perimeter-security deployment looks like. The pilot record says what
    /// that costs.
    fn refuse(who: &str) -> ShipmentTrackerFault {
        ShipmentTrackerFault::NotAuthorised(NotAuthorised {
            principal: "anonymous".to_owned(),
            required_role: who.to_owned(),
        })
    }
}

impl ShipmentTrackerServant for Tracking {
    fn knows(&self, __at: &ShipmentTrackerTarget<'_>) -> bool {
        // One tracking book, one object. `knows` has no default in the
        // generated trait on purpose — a default of `true` is the
        // single-object bug this shape exists to remove.
        __at.oid().is_empty()
    }

    fn lookup(
        &mut self,
        _at: &ShipmentTrackerTarget<'_>,
        reference: u32,
    ) -> Result<Consignment, ShipmentTrackerFault> {
        self.get(reference).cloned()
    }

    fn list_stops(
        &mut self,
        _at: &ShipmentTrackerTarget<'_>,
        reference: u32,
    ) -> Result<Vec<String>, ShipmentTrackerFault> {
        let c = self.get(reference)?;
        Ok(vec![c.origin.clone(), c.destination.clone()])
    }

    fn add_scan(
        &mut self,
        _at: &ShipmentTrackerTarget<'_>,
        reference: u32,
        _scan: ScanEvent,
    ) -> Result<(), ShipmentTrackerFault> {
        self.get(reference)?;
        Err(Tracking::refuse("depot-operator"))
    }

    fn history(
        &mut self,
        _at: &ShipmentTrackerTarget<'_>,
        reference: u32,
        since: String,
    ) -> Result<ScanHistory, ShipmentTrackerFault> {
        self.get(reference)?;
        let all = self.scans.get(&reference).cloned().unwrap_or_default();
        // `since` is a string date, because the contract says so. Comparing it
        // lexically is correct for the ISO-8601 form the typedef's comment
        // gives and wrong for any other, which is the typedef's problem and
        // not this servant's to fix.
        Ok(all.into_iter().filter(|s| s.occurred_at >= since).collect())
    }

    fn estimated_arrival(
        &mut self,
        _at: &ShipmentTrackerTarget<'_>,
        reference: u32,
    ) -> Result<String, ShipmentTrackerFault> {
        Ok(self.get(reference)?.promised.clone())
    }

    fn reroute(
        &mut self,
        _at: &ShipmentTrackerTarget<'_>,
        reference: u32,
        _depot: String,
    ) -> Result<(), ShipmentTrackerFault> {
        self.get(reference)?;
        Err(Tracking::refuse("dispatch-supervisor"))
    }

    fn delivered(
        &mut self,
        _at: &ShipmentTrackerTarget<'_>,
        reference: u32,
    ) -> Result<bool, ShipmentTrackerFault> {
        Ok(self.get(reference)?.delivered)
    }

    fn cancel(
        &mut self,
        _at: &ShipmentTrackerTarget<'_>,
        reference: u32,
        _reason: String,
    ) -> Result<(), ShipmentTrackerFault> {
        self.get(reference)?;
        Err(Tracking::refuse("booking-supervisor"))
    }

    fn backlog(&mut self, _at: &ShipmentTrackerTarget<'_>) -> Result<u32, ShipmentTrackerFault> {
        Ok(self.book.values().filter(|c| !c.delivered).count() as u32)
    }

    fn set_priority(
        &mut self,
        _at: &ShipmentTrackerTarget<'_>,
        reference: u32,
        _level: Priority,
    ) -> Result<(), ShipmentTrackerFault> {
        self.get(reference)?;
        Err(Tracking::refuse("dispatch-supervisor"))
    }

    fn problem_codes(
        &mut self,
        _at: &ShipmentTrackerTarget<'_>,
        reference: u32,
    ) -> Result<Vec<String>, ShipmentTrackerFault> {
        let c = self.get(reference)?;
        Ok(if c.delivered { Vec::new() } else { vec!["LATE-DEPART".to_owned()] })
    }

    /// Inherited from `MFS::Common::Describable`, and the reason this interface
    /// was picked: the skeleton has to dispatch an operation the contract for
    /// `ShipmentTracker` never declares.
    fn describe(&mut self, _at: &ShipmentTrackerTarget<'_>) -> Result<String, ShipmentTrackerFault> {
        Ok(format!("Meridian shipment tracking, {} consignment(s) on the book", self.book.len()))
    }

    /// `_get_label` on the wire — the inherited readonly attribute.
    fn label(&mut self, _at: &ShipmentTrackerTarget<'_>) -> Result<String, ShipmentTrackerFault> {
        Ok("tracking/LHR2".to_owned())
    }
}

// ── The POA in front of it ───────────────────────────────────────────────────

/// Decides whether a request's object key names something this process serves,
/// then hands it to the generated skeleton. Without this the POA would be
/// minting keys nobody checks, and a key from an earlier incarnation would
/// silently reach whatever occupies that id now.
struct PoaFront<D: Dispatch> {
    poa: Poa,
    inner: D,
}

impl<D: Dispatch> Dispatch for PoaFront<D> {
    fn dispatch(&mut self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        self.dispatch_body(request, out).map(|_| ())
    }

    /// The same question `dispatch_body` asks, asked where a §9.4.5 probe
    /// reaches it.
    ///
    /// D036 made this required, and the requirement is what brought a reader
    /// here: before it, this servant checked the key on the request path and
    /// inherited a permissive `knows`, so a `LocateRequest` for a key this
    /// POA calls `Unknown` was answered `ObjectHere`. That is the
    /// request/probe disagreement the `serve_one` reorder closed for a *moved*
    /// key and left open for an *unknown* one.
    ///
    /// [`Poa::serves`] is called rather than the check being written a second
    /// time here: one question, one home, two callers.
    fn knows(&self, object_key: &[u8]) -> bool {
        self.poa.serves(object_key)
    }

    fn dispatch_body(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> Result<DispatchBody, SystemException> {
        match self.poa.dispatch_target(&request.object_key, None) {
            Target::Active(_) => self.inner.dispatch_body(request, out),
            _ => Err(SystemException::object_not_exist()),
        }
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [ior_path] = args.as_slice() else {
        eprintln!("usage: estate-servant <ior-output-file>");
        return std::process::ExitCode::from(2);
    };

    // D019 step 4: `Server::bind` and `Poa::new` are `pub(crate)`; the ORB is
    // the only public way to a listener and a POA. This file is compiled
    // standalone, outside the workspace, so `cargo check --workspace` cannot
    // see it — which is exactly how it survived two sweeps that reported the
    // one-way rule holding.
    let orb = Orb::new();
    let mut poa = orb.create_poa("TrackingPOA", TYPE_ID);
    let oid = ObjectId::from_name("tracker-1");
    poa.activate(oid.clone());
    let key = poa.object_key(&oid);

    let server = match orb.server("127.0.0.1:0", key.clone()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bind: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let ior = match server.ior(TYPE_ID, "127.0.0.1") {
        Ok(i) => i,
        Err(e) => {
            eprintln!("ior: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let text = match ior.to_stringified() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("stringify: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    // Written to a temporary name and renamed, so a reader that sees the file
    // sees a whole IOR. A driver racing a half-written one is the same class of
    // phantom failure as a wait loop that does not wait.
    let tmp = format!("{ior_path}.partial");
    if std::fs::write(&tmp, format!("{text}\n")).is_err()
        || std::fs::rename(&tmp, ior_path).is_err()
    {
        eprintln!("could not publish the IOR to {ior_path}");
        return std::process::ExitCode::from(2);
    }
    eprintln!("serving {TYPE_ID} on {:?}", server.local_addr());

    let addr = server.local_addr().expect("bound");
    let refs = ShipmentTrackerRefs::new(orbweaver_gen::rt::ObjectHome::new(
        addr.ip().to_string(),
        addr.port(),
        key.clone(),
    ));
    let mut front = PoaFront { poa, inner: ShipmentTrackerSkeleton::new(refs, Tracking::new()) };
    // serve_sites: refusal — this process IS the server: serving is its whole
    // remaining job, and `spikes/estate/run.sh` stops it by `kill`ing the PID
    // it captured at launch. No in-process actor is left to raise a stop, so
    // a predicate here would be one nobody can call.
    if let Err(e) = server.serve(&mut front, || false) {
        eprintln!("serve: {e}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
