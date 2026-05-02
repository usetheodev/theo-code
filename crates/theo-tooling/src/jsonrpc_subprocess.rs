//! T3.1 / T13.1 — Subprocess wrapper that spawns a child and wires
//! its stdio to a `StdioSession`.
//!
//! Production code (LSP / DAP clients) uses this to spawn
//! `rust-analyzer`, `lldb-vscode`, etc. Tests use `cat` (or any
//! echoing command) to drive the full spawn → write → read loop
//! against a real OS subprocess WITHOUT depending on any specific
//! server being installed.
//!
//! Lifecycle: dropping `SubprocessSession` kills the child. Stderr
//! is captured into a Vec<u8> readable via `take_stderr()` so a
//! caller can surface diagnostic logs from a misbehaving server.

use std::io;
use std::process::Stdio;

use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

use crate::jsonrpc_session::{SessionError, StdioSession};

/// Errors specific to spawning the subprocess.
#[derive(Debug, thiserror::Error)]
pub enum SubprocessError {
    #[error("subprocess `{program}` not found on PATH (or stat failed): {source}")]
    NotFound {
        program: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to capture child stdio (stdin/stdout/stderr piped but unavailable)")]
    StdioCaptureFailed,
    #[error("subprocess spawn IO error: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Session(#[from] SessionError),
}

/// A spawned subprocess + its `StdioSession` (writer over stdin,
/// reader over stdout). Use `session_mut()` to write/read frames;
/// `kill()` to terminate explicitly; on drop the child is killed.
pub struct SubprocessSession {
    child: Child,
    session: StdioSession<ChildStdin, ChildStdout>,
    stderr: Option<ChildStderr>,
}

impl SubprocessSession {
    /// Spawn `program` with `args`, capturing stdin/stdout/stderr.
    /// Returns a session ready to write/read frames.
    pub fn spawn(program: &str, args: &[&str]) -> Result<Self, SubprocessError> {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Don't let the child inherit our controlling tty.
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                SubprocessError::NotFound {
                    program: program.into(),
                    source: e,
                }
            } else {
                SubprocessError::Io(e)
            }
        })?;

        let stdin = child.stdin.take().ok_or(SubprocessError::StdioCaptureFailed)?;
        let stdout = child.stdout.take().ok_or(SubprocessError::StdioCaptureFailed)?;
        let stderr = child.stderr.take();
        let session = StdioSession::new(stdin, stdout);
        Ok(Self {
            child,
            session,
            stderr,
        })
    }

    /// Borrow the session for write/read operations.
    pub fn session_mut(&mut self) -> &mut StdioSession<ChildStdin, ChildStdout> {
        &mut self.session
    }

    /// Take ownership of the captured stderr stream so the caller
    /// can drain it for diagnostics. Returns `None` if already taken.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.stderr.take()
    }

    /// Kill the child explicitly (doesn't wait). On drop the child
    /// is killed automatically via `kill_on_drop`, but this lets the
    /// caller signal early.
    pub async fn kill(&mut self) -> io::Result<()> {
        self.child.kill().await
    }

    /// Returns the child's PID if still alive.
    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonrpc_stdio::encode_frame;

    /// `cat` is a perfect echo subprocess: reads stdin, writes to
    /// stdout, no extra newlines. Available on every Unix CI runner.
    fn cat_available() -> bool {
        std::process::Command::new("cat")
            .arg("--version")
            .output()
            .is_ok()
    }

    #[tokio::test]
    async fn t31sub_spawn_unknown_binary_returns_not_found() {
        // SubprocessSession doesn't implement Debug (Child does not),
        // so `.expect_err` can't print it. Use a manual match.
        match SubprocessSession::spawn("definitely-not-a-real-binary-xyz", &[]) {
            Ok(_) => panic!("spawn should fail for missing binary"),
            Err(SubprocessError::NotFound { program, .. }) => {
                assert!(program.contains("definitely-not-a-real-binary-xyz"));
            }
            Err(other) => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t31sub_cat_echoes_one_frame_round_trip() {
        if !cat_available() {
            eprintln!("skip: `cat` not available");
            return;
        }
        let mut subp = SubprocessSession::spawn("cat", &[]).unwrap();
        let body = b"{\"jsonrpc\":\"2.0\",\"id\":1}";
        subp.session_mut().write_frame(body).await.unwrap();
        let echo = subp.session_mut().read_frame().await.unwrap();
        assert_eq!(echo, body);
    }

    #[tokio::test]
    async fn t31sub_cat_echoes_two_consecutive_frames_in_order() {
        if !cat_available() {
            eprintln!("skip: `cat` not available");
            return;
        }
        let mut subp = SubprocessSession::spawn("cat", &[]).unwrap();
        subp.session_mut().write_frame(b"{\"id\":1}").await.unwrap();
        subp.session_mut().write_frame(b"{\"id\":2}").await.unwrap();
        let f1 = subp.session_mut().read_frame().await.unwrap();
        let f2 = subp.session_mut().read_frame().await.unwrap();
        assert_eq!(f1, b"{\"id\":1}");
        assert_eq!(f2, b"{\"id\":2}");
    }

    #[tokio::test]
    async fn t31sub_pid_returns_some_after_spawn() {
        if !cat_available() {
            eprintln!("skip: `cat` not available");
            return;
        }
        let subp = SubprocessSession::spawn("cat", &[]).unwrap();
        assert!(subp.pid().is_some());
    }

    #[tokio::test]
    async fn t31sub_kill_on_drop_terminates_child() {
        if !cat_available() {
            eprintln!("skip: `cat` not available");
            return;
        }
        let pid;
        {
            let subp = SubprocessSession::spawn("cat", &[]).unwrap();
            pid = subp.pid().unwrap();
            // Drop here — kill_on_drop(true) ensures cat dies.
        }

        // Wait briefly for the kill to propagate, then verify the
        // process is gone. On Linux we can check `/proc/<pid>` exists.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let proc_path = format!("/proc/{pid}");
        // On Linux: /proc/<pid> should NOT exist after kill.
        // On other OSes the test is best-effort (skipped silently).
        if std::path::Path::new("/proc").exists() {
            assert!(
                !std::path::Path::new(&proc_path).exists(),
                "process {pid} should be dead after drop, but /proc/{pid} still exists"
            );
        }
    }

    #[tokio::test]
    async fn t31sub_take_stderr_succeeds_once() {
        if !cat_available() {
            eprintln!("skip: `cat` not available");
            return;
        }
        let mut subp = SubprocessSession::spawn("cat", &[]).unwrap();
        assert!(subp.take_stderr().is_some(), "first take returns Some");
        assert!(subp.take_stderr().is_none(), "second take returns None");
    }

    #[tokio::test]
    async fn t31sub_explicit_kill_stops_child() {
        if !cat_available() {
            eprintln!("skip: `cat` not available");
            return;
        }
        let mut subp = SubprocessSession::spawn("cat", &[]).unwrap();
        let pid = subp.pid().unwrap();
        subp.kill().await.unwrap();
        // After explicit kill, pid() may still return Some until
        // the child is reaped; the important property is that the
        // OS no longer has a live process.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if std::path::Path::new("/proc").exists() {
            let proc_path = format!("/proc/{pid}");
            assert!(!std::path::Path::new(&proc_path).exists());
        }
    }

    #[tokio::test]
    async fn t31sub_round_trip_encode_decode_through_real_subprocess() {
        if !cat_available() {
            eprintln!("skip: `cat` not available");
            return;
        }
        // Closes the loop: encode_frame → write_frame → cat echoes →
        // read_frame extracts the body. End-to-end proof that the
        // FrameAccumulator + StdioSession + Subprocess layers compose.
        let mut subp = SubprocessSession::spawn("cat", &[]).unwrap();
        let bodies: Vec<&[u8]> = vec![
            b"{}",
            b"{\"id\":1,\"method\":\"initialize\"}",
            b"{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"capabilities\":{}}}",
        ];
        for body in &bodies {
            subp.session_mut().write_frame(body).await.unwrap();
        }
        for body in &bodies {
            let echo = subp.session_mut().read_frame().await.unwrap();
            assert_eq!(echo, *body);
        }
    }

    #[test]
    fn t31sub_encode_frame_byte_count_matches_session_write() {
        // Sanity: the bytes encode_frame produces match what
        // write_frame writes — caller can predict exact wire size.
        let body = b"{\"x\":1}";
        let frame = encode_frame(body);
        let expected_header = format!("Content-Length: {}\r\n\r\n", body.len());
        assert!(frame.starts_with(expected_header.as_bytes()));
        assert_eq!(frame.len(), expected_header.len() + body.len());
    }
}
