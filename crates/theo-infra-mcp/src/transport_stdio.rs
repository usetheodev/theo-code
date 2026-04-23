//! stdio transport: subprocess + JSON-RPC over stdin/stdout (line-delimited).
//!
//! Subprocess is killed when the transport is dropped (subprocess kill on drop).

use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};

use crate::error::McpError;
use crate::protocol::{McpRequest, McpResponse};

#[derive(Debug)]
pub struct StdioTransport {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
}

impl StdioTransport {
    /// Spawn the server subprocess.
    pub async fn spawn(
        command: &str,
        args: &[String],
        env: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self, McpError> {
        let mut cmd = Command::new(command);
        cmd.args(args);
        cmd.envs(env);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        // Subprocess killed when handle is dropped
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Io(std::io::Error::other("no stdin")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Io(std::io::Error::other("no stdout")))?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    /// Send a request and wait for the matching response.
    pub async fn request(&mut self, req: McpRequest) -> Result<McpResponse, McpError> {
        let line = serde_json::to_string(&req)?;
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        // Read until we get the response with matching id
        loop {
            let mut buf = String::new();
            let n = self.stdout.read_line(&mut buf).await?;
            if n == 0 {
                return Err(McpError::TransportClosed);
            }
            let line = buf.trim();
            if line.is_empty() {
                continue;
            }
            // Skip notifications (no id field or id is null)
            let v: serde_json::Value = serde_json::from_str(line)?;
            if v.get("id").is_none() || v.get("id") == Some(&serde_json::Value::Null) {
                continue;
            }
            let resp: McpResponse = serde_json::from_value(v)?;
            if resp.id == req.id {
                return Ok(resp);
            }
            // Mismatched id — could be from a different request; for simplicity
            // we ignore it (proper impl would queue by id)
        }
    }

    /// Check whether the subprocess is still running.
    pub fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(None) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo_command_available() -> bool {
        std::process::Command::new("sh")
            .args(["-c", "echo hi"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[tokio::test]
    async fn stdio_transport_spawns_subprocess() {
        if !echo_command_available() {
            return;
        }
        let transport = StdioTransport::spawn(
            "sh",
            &["-c".into(), "cat".into()],
            std::iter::empty(),
        )
        .await
        .unwrap();
        // process is alive
        let mut t = transport;
        assert!(t.is_alive());
        // dropping the transport kills the child via kill_on_drop
    }

    #[tokio::test]
    async fn stdio_transport_request_reads_response_with_matching_id() {
        if !echo_command_available() {
            return;
        }
        // A trivial mock server: jq-like that echoes back as JSON-RPC response
        // We use `cat` and write a pre-cooked response to stdin → it will be
        // echoed to stdout. Since the request is also written to stdin, cat
        // outputs both lines. We want the response line. Skip the request line.
        let mut transport = StdioTransport::spawn(
            "sh",
            &[
                "-c".into(),
                // Read input, ignore it, emit a response with id=1
                "cat > /dev/null & echo '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}'; wait".into(),
            ],
            std::iter::empty(),
        )
        .await
        .unwrap();
        let req = McpRequest::new(1, "ping");
        let resp = transport.request(req).await.unwrap();
        assert_eq!(resp.id, serde_json::json!(1));
        assert!(resp.result.is_some());
    }

    #[tokio::test]
    async fn stdio_transport_spawn_invalid_command_returns_error() {
        let res = StdioTransport::spawn(
            "/nonexistent/command/xyz",
            &[],
            std::iter::empty(),
        )
        .await;
        assert!(res.is_err());
    }
}
