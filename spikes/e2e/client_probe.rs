//! The other generated half, driven: `ParkingControlClient` against the live
//! servant.
//!
//! The agent reaches the facility through the MCP bridge, which marshals
//! dynamically. That path never touches the generated *client stub*, so without
//! this probe the run would have generated a stub and measured nothing about
//! it. Both halves come out of one contract; both halves are called here.
//!
//! ```text
//! client-probe <ior-file>
//! ```
//!
//! Prints one `ok`/`FAIL` line per case and exits non-zero on any failure.

use std::time::Duration;

use orbweaver_gen::rt::{Connection, GiopError, Ior, WString};
use orbweaver_genout::f_parking::ParkingFacility::{GateState, ParkingControlClient, VehicleEntry};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [ior_path] = args.as_slice() else {
        eprintln!("usage: client-probe <ior-file>");
        return std::process::ExitCode::from(2);
    };
    let text = match std::fs::read_to_string(ior_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{ior_path}: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let ior = match Ior::parse(text.trim()) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{ior_path}: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let conn = match Connection::connect(&ior, Duration::from_secs(10)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("connect: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let mut fails = 0u32;
    let mut check = |what: &str, ok: bool| {
        if ok {
            println!("  ok   {what}");
        } else {
            fails += 1;
            println!("  FAIL {what}");
        }
    };

    let mut client = ParkingControlClient::new(conn);

    match client.get_floor_occupancy(-1) {
        Ok(o) => check(
            "stub get_floor_occupancy(-1) -> B1 has 12 free spaces",
            o.level == -1 && o.remaining_spaces == 12,
        ),
        Err(e) => check(&format!("get_floor_occupancy failed: {e}"), false),
    }

    // The declared user exception, through the generated stub: a floor the
    // facility does not have is UnknownFloor, not a sentinel count.
    match client.get_floor_occupancy(-9) {
        Err(GiopError::UserException { id, .. }) => {
            check("stub get_floor_occupancy(-9) raises UnknownFloor", id == UNKNOWN_FLOOR)
        }
        other => check(&format!("an unknown floor was not refused: {other:?}"), false),
    }

    match client.get_gate_status("entry-north".into()) {
        Ok(g) => check(
            "stub get_gate_status(entry-north) -> closed",
            g.gate_id == "entry-north" && g.state == GateState::GATE_CLOSED,
        ),
        Err(e) => check(&format!("get_gate_status failed: {e}"), false),
    }

    // Korean plate text through a `wstring`, which is why the contract chose
    // one: an ASCII-only round trip would prove nothing about this member.
    let plate = "12가 3456";
    let vehicle = VehicleEntry { plate_number: WString(plate.to_owned()) };
    match client.open_entry_gate(vehicle) {
        Ok(()) => check("stub open_entry_gate(Korean plate) -> admitted", true),
        Err(e) => check(&format!("open_entry_gate failed: {e}"), false),
    }

    match client.get_gate_status("entry-north".into()) {
        Ok(g) => check("the gate is open afterwards", g.state == GateState::GATE_OPEN),
        Err(e) => check(&format!("get_gate_status failed: {e}"), false),
    }

    // And the second open is the other declared exception, so the servant's
    // state really did change rather than the call being swallowed.
    let again = VehicleEntry { plate_number: WString(plate.to_owned()) };
    match client.open_entry_gate(again) {
        Err(GiopError::UserException { id, .. }) => {
            check("a second open raises GateAlreadyOpen", id == GATE_ALREADY_OPEN)
        }
        other => check(&format!("the second open was not refused: {other:?}"), false),
    }

    if fails == 0 {
        println!("client stub: PASS");
        std::process::ExitCode::SUCCESS
    } else {
        println!("client stub: FAIL — {fails} case(s)");
        std::process::ExitCode::FAILURE
    }
}

const UNKNOWN_FLOOR: &str = "IDL:ParkingFacility/UnknownFloor:1.0";
const GATE_ALREADY_OPEN: &str = "IDL:ParkingFacility/GateAlreadyOpen:1.0";
