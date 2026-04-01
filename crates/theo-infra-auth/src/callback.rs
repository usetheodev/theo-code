use crate::error::AuthError;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;
use tokio::sync::oneshot;

/// Result of an OAuth callback: the authorization code and state.
#[derive(Debug, Clone)]
pub struct CallbackResult {
    pub code: String,
    pub state: String,
}

const SUCCESS_HTML: &str = r#"<!DOCTYPE html>
<html>
<head><title>Theo Code - Authorization</title></head>
<body style="font-family:system-ui;display:flex;justify-content:center;align-items:center;height:100vh;margin:0;background:#1a1a2e;color:#e0e0e0">
<div style="text-align:center">
<h1 style="color:#4ade80">Authorization Successful</h1>
<p>You can close this window and return to Theo Code.</p>
<script>setTimeout(()=>window.close(),2000)</script>
</div>
</body>
</html>"#;

const ERROR_HTML: &str = r#"<!DOCTYPE html>
<html>
<head><title>Theo Code - Authorization Failed</title></head>
<body style="font-family:system-ui;display:flex;justify-content:center;align-items:center;height:100vh;margin:0;background:#1a1a2e;color:#e0e0e0">
<div style="text-align:center">
<h1 style="color:#f87171">Authorization Failed</h1>
<p>An error occurred during authorization. Please try again.</p>
</div>
</body>
</html>"#;

/// Start a local HTTP server and wait for the OAuth callback.
///
/// Listens on `127.0.0.1:{port}` for a GET request to `/auth/callback`
/// with `?code=...&state=...` query parameters.
///
/// Returns the code and state, or errors on timeout / state mismatch.
pub async fn wait_for_callback(
    port: u16,
    expected_state: &str,
    timeout_secs: u64,
) -> Result<CallbackResult, AuthError> {
    let expected = expected_state.to_string();

    let (tx, rx) = oneshot::channel::<Result<CallbackResult, AuthError>>();

    // Spawn blocking TCP listener in a background thread
    let handle = tokio::task::spawn_blocking(move || {
        let addr = format!("127.0.0.1:{port}");
        let listener = TcpListener::bind(&addr).map_err(|e| {
            AuthError::OAuth(format!("failed to bind {addr}: {e}"))
        })?;
        listener
            .set_nonblocking(false)
            .map_err(|e| AuthError::OAuth(format!("set_nonblocking: {e}")))?;

        // Set a socket timeout so we don't block forever
        let timeout = Duration::from_secs(timeout_secs);
        listener
            .set_nonblocking(false)
            .ok();

        // Accept one connection with timeout via polling
        let start = std::time::Instant::now();
        loop {
            // Non-blocking accept with short timeout
            listener.set_nonblocking(true).ok();
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).ok();
                    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();

                    let reader = BufReader::new(&stream);
                    let request_line = match reader.lines().next() {
                        Some(Ok(line)) => line,
                        _ => {
                            send_response(&mut stream, 400, ERROR_HTML);
                            continue;
                        }
                    };

                    // Parse: GET /auth/callback?code=...&state=... HTTP/1.1
                    let result = parse_callback_request(&request_line, &expected);
                    match &result {
                        Ok(_) => send_response(&mut stream, 200, SUCCESS_HTML),
                        Err(_) => send_response(&mut stream, 400, ERROR_HTML),
                    }

                    let _ = tx.send(result);
                    return Ok::<(), AuthError>(());
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if start.elapsed() >= timeout {
                        let _ = tx.send(Err(AuthError::CallbackTimeout(timeout_secs)));
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(e) => {
                    let _ = tx.send(Err(AuthError::OAuth(format!("accept: {e}"))));
                    return Ok(());
                }
            }
        }
    });

    // Wait for the callback result with timeout
    let result = tokio::time::timeout(
        Duration::from_secs(timeout_secs + 5),
        rx,
    )
    .await
    .map_err(|_| AuthError::CallbackTimeout(timeout_secs))?
    .map_err(|_| AuthError::OAuth("callback channel closed".to_string()))?;

    let _ = handle.await;
    result
}

/// Parse the callback GET request and extract code + state.
fn parse_callback_request(
    request_line: &str,
    expected_state: &str,
) -> Result<CallbackResult, AuthError> {
    // Request line: "GET /auth/callback?code=abc&state=xyz HTTP/1.1"
    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| AuthError::OAuth("invalid request line".to_string()))?;

    let query = path
        .split_once('?')
        .map(|(_, q)| q)
        .ok_or_else(|| AuthError::OAuth("no query parameters".to_string()))?;

    let params: std::collections::HashMap<&str, &str> = query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .collect();

    // Check for error response
    if let Some(error) = params.get("error") {
        let desc = params.get("error_description").unwrap_or(&"unknown error");
        return Err(AuthError::OAuth(format!("OAuth error: {error} — {desc}")));
    }

    let code = params
        .get("code")
        .ok_or_else(|| AuthError::OAuth("missing 'code' parameter".to_string()))?
        .to_string();

    let state = params
        .get("state")
        .ok_or_else(|| AuthError::OAuth("missing 'state' parameter".to_string()))?
        .to_string();

    // CSRF validation
    if state != expected_state {
        return Err(AuthError::StateMismatch);
    }

    Ok(CallbackResult { code, state })
}

/// Send an HTTP response to the browser.
fn send_response(stream: &mut std::net::TcpStream, status: u16, body: &str) {
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_callback_success() {
        let line = "GET /auth/callback?code=abc123&state=xyz789 HTTP/1.1";
        let result = parse_callback_request(line, "xyz789").unwrap();
        assert_eq!(result.code, "abc123");
        assert_eq!(result.state, "xyz789");
    }

    #[test]
    fn test_parse_callback_state_mismatch() {
        let line = "GET /auth/callback?code=abc&state=wrong HTTP/1.1";
        let result = parse_callback_request(line, "expected");
        assert!(matches!(result, Err(AuthError::StateMismatch)));
    }

    #[test]
    fn test_parse_callback_oauth_error() {
        let line = "GET /auth/callback?error=access_denied&error_description=user+denied HTTP/1.1";
        let result = parse_callback_request(line, "s");
        assert!(matches!(result, Err(AuthError::OAuth(_))));
    }

    #[test]
    fn test_parse_callback_missing_code() {
        let line = "GET /auth/callback?state=s HTTP/1.1";
        let result = parse_callback_request(line, "s");
        assert!(matches!(result, Err(AuthError::OAuth(_))));
    }

    #[test]
    fn test_parse_callback_no_query() {
        let line = "GET /auth/callback HTTP/1.1";
        let result = parse_callback_request(line, "s");
        assert!(matches!(result, Err(AuthError::OAuth(_))));
    }
}
