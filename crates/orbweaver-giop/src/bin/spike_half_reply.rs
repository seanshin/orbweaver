//! Drives this crate's client against `spikes/half_reply_peer.py` — a peer
//! that closes the connection between two writes of one reply.
//!
//! The in-process version of this measurement is
//! `tests/two_writes_of_one_reply.rs`, and it is the more thorough one. This
//! binary exists for the one thing an in-process peer cannot be: a **separate
//! process, in another language, whose bytes were never produced by this
//! crate's encoders**. Neither omniORB nor JacORB will shut down inside the
//! window between two writes of a reply on command (`docs/decisions/D010` §4
//! B5), so a hand-written socket is the closest thing to an independent peer
//! that exists for this shape.
//!
//! The exit code is the verdict. What it prints is for a reader and for the
//! runner's cross-check of the request id — the peer names the call it cut, and
//! this names the call the client heard about; a claim that they are the same
//! call is worth nothing unless two processes say so separately.
//!
//! Three exit codes, because two of them would lie. `0` is the claim holding,
//! `1` is the claim refuted, and **`3` is nothing measured** — the client never
//! reached the peer, so it has no account of the interruption to be right or
//! wrong about. Both are failures, and a runner that collapsed them would
//! report a refuted claim on a run where nothing was measured, which is a false
//! diagnosis pointed straight at the code under test. Measured once in ~450
//! cases here: a `Connection refused` against a peer that had demonstrably
//! bound, listened and published its port. Not diagnosed — it has not
//! reproduced in 448 subsequent cases — and `SO_REUSEADDR` on the peer's
//! ephemeral bind was the obvious suspect and was measured innocent (0 refusals
//! in 6000 bind/listen/connect cycles with it, 487 failed *binds* in 3000
//! without it, so removing it is strictly worse).
//!
//! ```text
//! spike-half-reply --addr 127.0.0.1:9001 --cut 0 --control close
//! ```
//!
//! *한 응답의 두 번의 쓰기 사이에 끊는 피어 — 다른 언어, 다른 프로세스. 종료
//! 코드가 판정이다.*

use std::process::ExitCode;
use std::time::Duration;

use orbweaver_giop::mux::{Failed, Mux};
use orbweaver_giop::{Error, IiopProfile, Ior, MsgType, Version};

const T: Duration = Duration::from_secs(20);

/// Nothing was measured. Distinct from [`ExitCode::FAILURE`], which means the
/// claim was measured and did not hold.
const UNMEASURED: u8 = 3;

fn usage() -> ExitCode {
    eprintln!(
        "usage: spike-half-reply --addr HOST:PORT --cut <index> --control close|error \
         [--requests N]"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut addr = String::new();
    let mut cut = 0usize;
    let mut requests = 2usize;
    let mut control = MsgType::CloseConnection;
    let mut i = 0;
    while i < args.len() {
        let value = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--addr" => addr = value,
            "--cut" => cut = value.parse().unwrap_or(0),
            "--requests" => requests = value.parse().unwrap_or(2),
            "--control" => {
                control = match value.as_str() {
                    "close" => MsgType::CloseConnection,
                    "error" => MsgType::MessageError,
                    _ => return usage(),
                }
            }
            _ => return usage(),
        }
        i += 2;
    }
    if addr.is_empty() || requests < 2 || cut >= requests {
        return usage();
    }

    let Some((host, port)) = addr.rsplit_once(':') else { return usage() };
    let Ok(port) = port.parse::<u16>() else { return usage() };
    let ior = Ior {
        type_id: "IDL:spike/HalfAnswered:1.0".into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: host.to_string(),
            port,
            object_key: b"half".to_vec(),
            components: Vec::new(),
        }],
    };

    let mux = match Mux::connect(&ior, T) {
        Ok(m) => m,
        Err(e) => {
            println!("verdict=unmeasured reason=connect: {e}");
            return ExitCode::from(UNMEASURED);
        }
    };
    if !mux.multiplexes() {
        println!("verdict=fail reason=this connection would not multiplex");
        return ExitCode::FAILURE;
    }

    // Sent one after the other under the write half, so the order the peer
    // reads them in is the order they went out in — which is what lets the two
    // processes agree on which call `--cut` names.
    let mut pending = Vec::new();
    for n in 0..requests {
        match mux.send(b"half", &format!("call_{n}"), |_| {}) {
            Ok(p) => pending.push(p),
            Err(e) => {
                // The request never reached the peer, so there is no
                // interruption for this run to have an account of.
                println!("verdict=unmeasured reason=request {n} did not go out: {e}");
                return ExitCode::from(UNMEASURED);
            }
        }
    }
    let ids: Vec<u32> = pending.iter().map(|p| p.request_id()).collect();

    let mut outcomes: Vec<Failed> = Vec::new();
    for (n, p) in pending.into_iter().enumerate() {
        match p.wait(T) {
            Err(f) => outcomes.push(f),
            Ok(_) => {
                println!(
                    "verdict=fail reason=caller {n} was answered; half a reply is not an answer"
                );
                return ExitCode::FAILURE;
            }
        }
    }

    let mut wrong = Vec::new();
    for (n, f) in outcomes.iter().enumerate() {
        let expected_unsent = if n == cut {
            match f.error {
                Error::InterruptedMidReassembly { control: c, partial, request_id, received } => {
                    println!(
                        "cut_id={request_id} cut_received={received} cut_partial={partial:?} \
                         cut_control={c:?}"
                    );
                    if c != control {
                        wrong.push(format!("caller {n} named {c:?}, the peer wrote {control:?}"));
                    }
                    if partial != MsgType::Reply {
                        wrong.push(format!("caller {n} says a {partial:?} was cut, not a Reply"));
                    }
                    if request_id != ids[n] {
                        wrong.push(format!(
                            "caller {n} was told about request {request_id}, its own is {}",
                            ids[n]
                        ));
                    }
                }
                ref other => {
                    wrong.push(format!("caller {n} had its reply cut and heard `{other}` instead"));
                }
            }
            // The peer had begun this one's reply, so §13.5.1's "was not
            // processed" is false about it whatever the control message was.
            false
        } else {
            match (control, &f.error) {
                (MsgType::CloseConnection, Error::ConnectionClosed) => {}
                (MsgType::MessageError, Error::UnexpectedMessage(MsgType::MessageError)) => {}
                (_, other) => {
                    wrong.push(format!("caller {n} got nothing back and heard `{other}`"));
                }
            }
            // Only a goodbye frees the untouched caller. §9.4.8's report names
            // no request and so promises nothing about any of them.
            control == MsgType::CloseConnection
        };
        println!(
            "caller{n}_id={} caller{n}_unsent={} caller{n}_error={}",
            ids[n], f.unsent, f.error
        );
        if f.unsent != expected_unsent {
            wrong.push(format!(
                "caller {n} was told unsent={}, and the answer it is owed is {expected_unsent}",
                f.unsent
            ));
        }
    }

    if mux.is_usable() {
        wrong.push("the connection is still being offered after the peer tore it down".into());
    }

    for w in &wrong {
        println!("wrong={w}");
    }
    if wrong.is_empty() {
        println!("verdict=ok");
        ExitCode::SUCCESS
    } else {
        println!("verdict=fail");
        ExitCode::FAILURE
    }
}
