//! A Python servant this process **owns**, mountable as a `Dispatch`.
//!
//! # Why this exists, in one sentence that was already written
//!
//! `spikes/leak_tests.sh`'s language leg has been a counted `SKIPPED` since it
//! was written, and it names its own blocker exactly:
//!
//! > What it waits on is a real Python process reachable from one: the only
//! > route today is `orbweaver-py-bridge --serve`, **which binds its own
//! > listener**, so the Python servant arrives as an endpoint rather than as a
//! > servant and a swap becomes a move.
//!
//! A caller that has to dial a different address has been **moved**, and
//! *location* and *language* are different rows of D029 §6.1. Measuring the
//! language row needs the servant to change behind **one** reference, on a
//! server the test owns — so the Python side has to arrive as a `Dispatch`,
//! not as an endpoint.
//!
//! [`PythonChild`] is that: it spawns `python3`, speaks the seam's line-framed
//! JSON to the child over its own pipes, and is an [`Answerer`] — so
//! [`ForeignServant`] turns it into a `Dispatch` with no new protocol and no
//! second implementation of one.
//!
//! # The direction, and why it is the mirror rather than a second protocol
//!
//! `orbweaver-py-bridge --serve` makes Python the **parent**: Python spawns the
//! bridge, the bridge binds, and the bridge asks Python. Here Rust is the
//! parent: Rust spawns `python3`, and `python3` answers on its own stdin and
//! stdout through `python_rt.serve_on_pipes`. The document on the wire is the
//! same document in both directions, and the loop that answers it is one
//! function in `python_rt` that both modes call — the protocol has one home and
//! keeps it.
//!
//! # Reaping
//!
//! The child is spawned into **its own process group** and the group is
//! signalled in `Drop`. That is not defensive habit: `orbweaver-py-bridge`
//! leaked twelve processes from one harness run and fifty more from the days
//! before, every one `ppid=1` and each holding a loopback port, because three
//! layers each did their job inside their own scope and **nobody owned the
//! span**. This type owns the span.
//!
//! *`Host`의 역이며 새 프로토콜이 아니다. 언어 행을 재려면 서번트가
//! **엔드포인트가 아니라 `Dispatch`로** 도착해야 한다 — 주소를 바꿔 다이얼하면
//! 그것은 이동이지 재구현이 아니기 때문이다. 자식은 자기 프로세스 그룹으로
//! 띄우고 `Drop`에서 그룹에 신호한다: 누수 열두 개가 남긴 교훈이다.*

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

use orbweaver_dynamic::json::Json;

use crate::seam::Answerer;

/// A `python3` child that answers seam calls on its own pipes.
///
/// Construct it with [`PythonChild::spawn`] and hand it to
/// [`crate::seam::ForeignServant::new`].
pub struct PythonChild {
    child: Child,
    /// `Option` so `Drop` can actually **close** it. It was a bare
    /// `ChildStdin` and `Drop` took `self.child.stdin` instead — which spawn
    /// had already emptied — so the line whose comment said *close stdin first*
    /// closed nothing, the child never saw EOF, and the signal below became
    /// load-bearing when it was meant to be a backstop.
    stdin: Option<ChildStdin>,
    stdout: BufReader<std::process::ChildStdout>,
    /// Set once the child has closed its end, so a servant whose Python side
    /// has gone answers the next call with a seam failure instead of blocking.
    gone: bool,
}

impl PythonChild {
    /// Spawns `python3 -c <program>`, with `sys.path` carrying `paths`.
    ///
    /// `program` is expected to end by calling
    /// `orbweaver_rt.serve_on_pipes(servant)`. Nothing here checks that: the
    /// evidence is the first call being answered, and a program that never
    /// serves fails that way rather than by a shape check nobody can read.
    ///
    /// **The child's stdout is the protocol**, so a servant that prints there
    /// corrupts it. `python_rt.serve_on_pipes` says so in its own docstring and
    /// this does not redirect it — silently moving a stream is worse than a
    /// garbled line somebody can see.
    pub fn spawn(program: &str, paths: &[&std::path::Path]) -> Result<Self, String> {
        let prologue: String = paths
            .iter()
            .map(|p| format!("import sys; sys.path.insert(0, {:?})\n", p.display().to_string()))
            .collect();
        let mut cmd = Command::new("python3");
        cmd.arg("-c")
            .arg(format!("{prologue}{program}"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        // Its own group, so `Drop` can reap the tree rather than the leaf.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd.spawn().map_err(|e| format!("python3 did not start: {e}"))?;
        let stdin = child.stdin.take().ok_or("the child has no stdin")?;
        let stdout = child.stdout.take().ok_or("the child has no stdout")?;
        Ok(Self { child, stdin: Some(stdin), stdout: BufReader::new(stdout), gone: false })
    }
}

impl Answerer for PythonChild {
    fn ask(&mut self, call: &Json) -> Result<Json, String> {
        if self.gone {
            return Err("the servant closed its end".to_owned());
        }
        let document = Json::Object([("call".to_owned(), call.clone())].into_iter().collect());
        let pipe = self.stdin.as_mut().ok_or("the servant's input is already closed")?;
        writeln!(pipe, "{document}").map_err(|e| e.to_string())?;
        pipe.flush().map_err(|e| e.to_string())?;
        self.read_answer(&mut |_| {
            // `ask` is `ask_resolving` with nobody to resolve, and a nested
            // request arriving here is the far side using a message this caller
            // did not agree to answer. It is refused as a seam failure rather
            // than ignored, because ignoring it deadlocks both ends: the child
            // waits for an answer that is never written and the parent waits
            // for a reply that is never sent.
            crate::seam::nested_refusal("this caller does not answer nested requests")
        })
    }

    fn ask_resolving(
        &mut self,
        call: &Json,
        resolve: &mut dyn FnMut(&Json) -> Json,
    ) -> Result<Json, String> {
        if self.gone {
            return Err("the servant closed its end".to_owned());
        }
        let document = Json::Object([("call".to_owned(), call.clone())].into_iter().collect());
        let pipe = self.stdin.as_mut().ok_or("the servant's input is already closed")?;
        writeln!(pipe, "{document}").map_err(|e| e.to_string())?;
        pipe.flush().map_err(|e| e.to_string())?;
        self.read_answer(resolve)
    }
}

impl PythonChild {
    /// Read documents until one is the reply, answering nested requests on the
    /// way.
    ///
    /// **This is the loop D038 §2 says every implementation of the protocol
    /// grows.** It used to be *read the reply*; it is now *read the next
    /// document, which may be a reply or may be a request.* The recursion is
    /// bounded by the far side: each nested request is answered before the next
    /// document is read, so the depth is however deep the far side nests and the
    /// pipe stays in step either way.
    fn read_answer(&mut self, resolve: &mut dyn FnMut(&Json) -> Json) -> Result<Json, String> {
        let mut line = String::new();
        loop {
            line.clear();
            let n = self.stdout.read_line(&mut line).map_err(|e| e.to_string())?;
            if n == 0 {
                self.gone = true;
                return Err("the servant closed its end".to_owned());
            }
            if line.trim().is_empty() {
                continue;
            }
            let document = Json::parse(line.trim()).map_err(|e| e.to_string())?;
            let Some(invoke) = document.get(crate::seam::ENVELOPE_INVOKE) else {
                return Ok(document);
            };
            let answered = resolve(invoke);
            let envelope = Json::Object(
                [(crate::seam::ENVELOPE_ANSWER.to_owned(), answered)].into_iter().collect(),
            );
            let pipe = self.stdin.as_mut().ok_or("the servant's input is already closed")?;
            writeln!(pipe, "{envelope}").map_err(|e| e.to_string())?;
            pipe.flush().map_err(|e| e.to_string())?;
        }
    }
}

impl Drop for PythonChild {
    fn drop(&mut self) {
        // **Close the pipe, and mean it.** `serve_on_pipes` blocks on
        // `sys.stdin.readline()`, so an EOF is how it leaves — through its own
        // return, running whatever cleanup it has. This line used to be
        // `self.child.stdin.take()`, which `spawn` had already emptied: it
        // closed nothing, the child never saw EOF, and the group signal that
        // followed became the thing actually doing the work.
        drop(self.stdin.take());

        // **No group signal, and that is a change of position.** This type's
        // first version shelled out to `kill -TERM -<pgid>` on the reasoning
        // that *reaping a child is not reaping its tree*, which is a rule this
        // repository paid twelve leaked processes for. It does not apply here:
        // a `serve_on_pipes` child spawns nothing, so there is no tree, and a
        // group signal buys nothing while being the only thing in this file
        // that reaches beyond the child.
        //
        // It is removed on evidence rather than on taste. CI runs went from
        // green (22–29 minutes) to **cancelled at four minutes**, every one
        // after the commit that added this type, all three dying as
        // `cargo test --workspace` began, and none of it visible in four green
        // harness runs here — which is the exact shape CLAUDE.md records for
        // the `ppid=1` backstop: *it never showed locally, because a Terminal's
        // `zsh` has a live parent.* NOT DIAGNOSED — a Linux runner cannot be
        // reproduced from here — but this was the only mechanism in this code
        // that signals anything it does not own, and the defect above is what
        // made it load-bearing.
        //
        // *그룹 신호를 뺀다. 이 자식은 나무가 없어 그것이 사는 것은 없고, 이
        // 파일에서 자기 것이 아닌 것에 닿는 유일한 기제였다. 진단이 아니라
        // 증거다: 이 타입을 넣은 커밋부터 CI가 4분 만에 취소되기 시작했고
        // 로컬에서는 네 번 초록이었다.*
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
