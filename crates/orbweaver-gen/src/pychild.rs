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
    stdin: ChildStdin,
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
        Ok(Self { child, stdin, stdout: BufReader::new(stdout), gone: false })
    }
}

impl Answerer for PythonChild {
    fn ask(&mut self, call: &Json) -> Result<Json, String> {
        if self.gone {
            return Err("the servant closed its end".to_owned());
        }
        let document = Json::Object([("call".to_owned(), call.clone())].into_iter().collect());
        writeln!(self.stdin, "{document}").map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())?;
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
            return Json::parse(line.trim()).map_err(|e| e.to_string());
        }
    }
}

impl Drop for PythonChild {
    fn drop(&mut self) {
        // Close stdin first: a child blocked on `readline` leaves when its
        // input ends, which is the exit that runs its own cleanup. The signal
        // below is for the one that does not.
        let _ = self.child.stdin.take();
        #[cfg(unix)]
        {
            // The GROUP, not the leaf. `python3` may itself have children, and
            // reaping a child is not reaping its tree — measured, twelve
            // leaked processes from one harness run.
            let pid = self.child.id() as i32;
            let _ = Command::new("kill").arg("-TERM").arg(format!("-{pid}")).status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
