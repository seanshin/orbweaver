//! The fixture `spikes/orb_shutdown_peer.py` dials: a server that is stopped by
//! its **ORB** while a request is inside its servant.
//!
//! D029 §5 O1's oracle is a peer mid-call. The peer is the Python program; this
//! is the thing being measured, and its only job is to make *"the shutdown
//! landed while a request was inside the servant"* a fact rather than a race:
//!
//! 1. bind through [`Orb::server`] and publish the port;
//! 2. serve on a thread, with **`|| false`** as the caller's own predicate —
//!    the shape 17 of this workspace's 63 serve sites use, and the shape that
//!    before D032 could not be stopped without killing the process;
//! 3. the servant signals when it is entered, and blocks;
//! 4. the main thread calls [`Orb::shutdown`] on that signal, then releases;
//! 5. report, and exit on the report.
//!
//! The rendezvous is a channel, never a sleep. A sleep here would make the
//! measurement a statement about this machine's scheduler.
//!
//! *피어는 파이썬 쪽이고, 이쪽은 측정 대상이다. 이 프로그램의 유일한 일은 "종료가
//! 서번트 안에 요청이 있는 동안 착지했다"를 경합이 아니라 사실로 만드는 것이다.*

use std::sync::mpsc;
use std::time::Duration;

use orbweaver_cdr::Encoder;
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{Dispatch, Request, SystemException};

/// Nothing measured — the same spelling `spike_half_reply` uses, and kept apart
/// from 1 (refuted) because a fixture that could not run is a failure that
/// needs a different fix from a claim that was refuted.
const UNMEASURED: i32 = 3;
const DEADLINE: Duration = Duration::from_secs(30);
const ANSWER: i32 = 42;

/// Holds inside the first `held` call until the main thread has shut the ORB
/// down.
struct Held {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    held: bool,
}

impl Dispatch for Held {
    fn dispatch(&mut self, req: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        match req.operation.as_str() {
            "held" => {
                if !self.held {
                    self.held = true;
                    let _ = self.entered.send(());
                    // Bounded, so a wedged run ends rather than hangs.
                    let _ = self.release.recv_timeout(DEADLINE);
                }
                out.put_i32(ANSWER);
                Ok(())
            }
            "ping" => {
                out.put_i32(ANSWER);
                Ok(())
            }
            _ => Err(SystemException::bad_operation()),
        }
    }
}

fn main() -> std::process::ExitCode {
    let mut args = std::env::args().skip(1);
    let mut port_file = None;
    let mut key = b"StopProbe".to_vec();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port-file" => port_file = args.next(),
            "--object-key" => key = args.next().unwrap_or_default().into_bytes(),
            other => {
                eprintln!("spike-orb-shutdown: unknown argument {other:?}");
                return std::process::ExitCode::from(UNMEASURED as u8);
            }
        }
    }
    let Some(port_file) = port_file else {
        eprintln!("spike-orb-shutdown: --port-file is required");
        return std::process::ExitCode::from(UNMEASURED as u8);
    };

    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut servant = Held { entered: entered_tx, release: release_rx, held: false };

    let orb = Orb::new();
    let server = match orb.server("127.0.0.1:0", key) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("spike-orb-shutdown: could not bind: {e}");
            return std::process::ExitCode::from(UNMEASURED as u8);
        }
    };
    let port = match server.local_addr() {
        Ok(a) => a.port(),
        Err(e) => {
            eprintln!("spike-orb-shutdown: no bound address: {e}");
            return std::process::ExitCode::from(UNMEASURED as u8);
        }
    };

    // Published atomically: a runner's wait loop that can read a half-written
    // file is a wait loop that reports a phantom failure.
    let tmp = format!("{port_file}.partial");
    if std::fs::write(&tmp, format!("{port}\n")).is_err()
        || std::fs::rename(&tmp, &port_file).is_err()
    {
        eprintln!("spike-orb-shutdown: could not publish the port to {port_file}");
        return std::process::ExitCode::from(UNMEASURED as u8);
    }

    let serving = std::thread::spawn(move || server.serve(&mut servant, || false));

    // Not a sleep. The servant says when it is inside.
    if entered_rx.recv_timeout(DEADLINE).is_err() {
        eprintln!("spike-orb-shutdown: no peer reached the servant within {DEADLINE:?}");
        return std::process::ExitCode::from(UNMEASURED as u8);
    }
    let report = orb.shutdown();
    let _ = release_tx.send(());

    let quiet = orb.wait_until_stopped(DEADLINE);
    let served = serving.join();
    println!(
        "{{\"servers_stopped\":{},\"pools_closed\":{},\"already_gone\":{},\
          \"went_quiet\":{},\"serve_returned_ok\":{}}}",
        report.servers(),
        report.pools(),
        report.already_gone(),
        quiet,
        matches!(served, Ok(Ok(()))),
    );

    // Every one of these is a property of *this* side; the verdict about what a
    // peer saw belongs to the peer and is its exit code, not ours.
    if report.servers() == 1 && quiet && matches!(served, Ok(Ok(()))) {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::from(1)
    }
}
