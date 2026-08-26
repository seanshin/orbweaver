//! The parking facility itself — the half of this demonstration a human wrote.
//!
//! Everything around it is generated: the `ParkingControlServant` trait, the
//! `ParkingControlSkeleton` that decodes requests into calls on it, the structs,
//! the enum, the exception enum and its wire encoding. What is *not* generated,
//! and cannot be, is what the facility does — which floors exist, how many
//! spaces are free, what "already open" means. That is the body below, and the
//! run record counts it as real work rather than as free.
//!
//! It is compiled into the crate `gen-corpus` emitted, outside the workspace,
//! so it stands on the published crate surface exactly as an application would.
//!
//! ```text
//! e2e-servant <ior-output-file>
//! ```
//!
//! Serves until it is killed. The harness captures the PID and kills it by
//! that; nothing here polls for a stop file.

use std::collections::BTreeMap;

use orbweaver_gen::rt::{
    Dispatch, DispatchBody, Encoder, Request, Server, SystemException, WString,
};
use orbweaver_genout::f_parking::ParkingFacility::{
    BarrierGate, FloorLevel, FloorOccupancy, GateAlreadyOpen, GateIdentifier, GateState,
    ParkingControlFault, ParkingControlRefs, ParkingControlServant, ParkingControlSkeleton,
    ParkingControlTarget, VehicleEntry,
};
use orbweaver_giop::orb::Orb;
use orbweaver_object::{ObjectId, OrbPoa, Poa, Target};

const TYPE_ID: &str = "IDL:ParkingFacility/ParkingControl:1.0";
const ENTRY_GATE: &str = "entry-north";

// ── The hand-written half ────────────────────────────────────────────────────

/// One parking facility. Nothing here mentions GIOP, CDR, an operation name or
/// a repository id — which is the whole claim a generated skeleton makes.
struct Facility {
    /// Free spaces per floor; basements are negative, as the contract's comment
    /// says the level is signed for exactly that reason.
    floors: BTreeMap<FloorLevel, u32>,
    gates: BTreeMap<GateIdentifier, GateState>,
    admitted: Vec<String>,
}

impl Facility {
    fn new() -> Self {
        Self {
            floors: BTreeMap::from([(-2, 41), (-1, 12), (1, 3), (2, 0)]),
            gates: BTreeMap::from([
                (ENTRY_GATE.to_owned(), GateState::GATE_CLOSED),
                ("exit-south".to_owned(), GateState::GATE_CLOSED),
            ]),
            admitted: Vec::new(),
        }
    }
}

impl ParkingControlServant for Facility {
    fn knows(&self, __at: &ParkingControlTarget<'_>) -> bool {
        // One facility, one object: the default oid and nothing else. `knows`
        // has no default in the generated trait on purpose — a default of
        // `true` is the single-object bug this shape exists to remove.
        __at.oid().is_empty()
    }

    fn get_floor_occupancy(
        &mut self,
        __at: &ParkingControlTarget<'_>,
        level: FloorLevel,
    ) -> Result<FloorOccupancy, ParkingControlFault> {
        match self.floors.get(&level) {
            Some(&remaining_spaces) => Ok(FloorOccupancy { level, remaining_spaces }),
            // The requirement asked for the unknown floor to be its own
            // exception rather than a sentinel count, and it is.
            None => Err(ParkingControlFault::UnknownFloor(
                orbweaver_genout::f_parking::ParkingFacility::UnknownFloor { level },
            )),
        }
    }

    fn get_gate_status(
        &mut self,
        __at: &ParkingControlTarget<'_>,
        target_gate: GateIdentifier,
    ) -> Result<BarrierGate, ParkingControlFault> {
        // The contract declares no `raises` here, so an unknown gate has to be
        // answered inside the return type. GATE_FAULT is the honest value: the
        // facility cannot report a state it cannot read.
        let state = self.gates.get(&target_gate).copied().unwrap_or(GateState::GATE_FAULT);
        Ok(BarrierGate { gate_id: target_gate, state })
    }

    fn open_entry_gate(&mut self, __at: &ParkingControlTarget<'_>, vehicle: VehicleEntry) -> Result<(), ParkingControlFault> {
        let state = self.gates.get(ENTRY_GATE).copied().unwrap_or(GateState::GATE_FAULT);
        if state == GateState::GATE_OPEN {
            return Err(ParkingControlFault::GateAlreadyOpen(GateAlreadyOpen {
                gate_id: ENTRY_GATE.to_owned(),
                state,
            }));
        }
        self.gates.insert(ENTRY_GATE.to_owned(), GateState::GATE_OPEN);
        let WString(plate) = vehicle.plate_number;
        self.admitted.push(plate);
        Ok(())
    }
}

// ── The POA in front of it ───────────────────────────────────────────────────

/// Decides whether a request's object key names something this process serves,
/// then hands it to the generated skeleton.
///
/// `Server` adopts one key and does not itself judge the one a peer addressed,
/// so without this the POA would be minting keys nobody checks. A key from an
/// earlier incarnation is `Target::Unknown` and gets `OBJECT_NOT_EXIST`, which
/// is the answer that makes a stale transient reference recognisably stale
/// instead of silently reaching whatever occupies that id now.
struct PoaFront<D: Dispatch> {
    poa: Poa,
    inner: D,
}

impl<D: Dispatch> Dispatch for PoaFront<D> {
    fn dispatch(&mut self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        self.dispatch_body(request, out).map(|_| ())
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
        eprintln!("usage: e2e-servant <ior-output-file>");
        return std::process::ExitCode::from(2);
    };

    // D019 step 4: `Server::bind` and `Poa::new` are `pub(crate)`; the ORB is
    // the only public way to a listener and a POA. This file is compiled
    // standalone, outside the workspace, so `cargo check --workspace` cannot
    // see it — which is exactly how it survived two sweeps that reported the
    // one-way rule holding.
    let orb = Orb::new();
    let mut poa = orb.create_poa("ParkingPOA", TYPE_ID);
    let oid = ObjectId::from_name("facility-1");
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
    // sees a whole IOR. A harness racing a half-written one is the same class
    // of phantom failure as a wait loop that does not wait.
    let tmp = format!("{ior_path}.partial");
    if std::fs::write(&tmp, format!("{text}\n")).is_err()
        || std::fs::rename(&tmp, ior_path).is_err()
    {
        eprintln!("could not publish the IOR to {ior_path}");
        return std::process::ExitCode::from(2);
    }
    eprintln!("serving {TYPE_ID} on {:?}", server.local_addr());

    // The skeleton now needs the key scheme as well as the servant: it parses
    // the object key itself rather than assuming there is only one object.
    // Rooted at exactly the key the server was bound with, or nothing it
    // parses matches what arrives.
    let addr = server.local_addr().expect("bound");
    let refs = ParkingControlRefs::new(orbweaver_gen::rt::ObjectHome::new(
        addr.ip().to_string(),
        addr.port(),
        key.clone(),
    ));
    let mut front =
        PoaFront { poa, inner: ParkingControlSkeleton::new(refs, Facility::new()) };
    if let Err(e) = server.serve(&mut front, || false) {
        eprintln!("serve: {e}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
