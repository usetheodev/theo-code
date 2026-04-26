//! OTLP exporter — 
//!
//! Env-driven configuration that drives the OpenTelemetry SDK exporter.
//! When `OTLP_ENDPOINT` is unset, `init_otlp_exporter` is a no-op so the
//! CLI works exactly as before — operator opt-in (D2).
//!
//! ## Env vars
//!
//! - `OTLP_ENDPOINT` — required to enable. e.g. `http://localhost:4317`
//!   (gRPC) or `http://localhost:4318` (HTTP).
//! - `OTLP_PROTOCOL` — `grpc` (default) | `http_protobuf` (alias `http`).
//! - `OTLP_TIMEOUT_SECS` — positive `u64`, default `10`. Values `<= 0`
//!   or unparseable fall back to default.
//! - `OTLP_HEADERS` — `"k1=v1,k2=v2"` for auth or tenancy.
//! - `OTLP_SERVICE_NAME` — default `"theo"`.
//! - `OTLP_BATCH_SIZE` — `usize`, default `512`. `0` reserved for sync
//!   exporter (currently same path; kept for forward compat).

#![cfg(feature = "otel")]

use std::time::Duration;

use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::runtime::Tokio as TokioRuntime;
use opentelemetry_sdk::trace::TracerProvider as SdkTracerProvider;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtlpExporterConfig {
    pub endpoint: String,
    pub protocol: OtlpProtocol,
    pub timeout: Duration,
    pub headers: Vec<(String, String)>,
    pub service_name: String,
    pub batch_size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtlpProtocol {
    Grpc,
    HttpProtobuf,
}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_BATCH_SIZE: usize = 512;
const DEFAULT_SERVICE_NAME: &str = "theo";

impl OtlpExporterConfig {
    /// Reads the config from process env vars. Returns `None` when
    /// `OTLP_ENDPOINT` is absent — this is the operator's opt-out path
    /// and the caller (`init_otlp_exporter`) becomes a no-op.
    pub fn from_env() -> Option<Self> {
        use theo_domain::environment::{parse_var, theo_var};
        let endpoint = theo_var("OTLP_ENDPOINT")?;
        let protocol_raw = theo_var("OTLP_PROTOCOL");
        let protocol = parse_protocol(protocol_raw.as_deref());
        let timeout_raw = theo_var("OTLP_TIMEOUT_SECS");
        let timeout = parse_timeout(timeout_raw.as_deref());
        let headers = parse_headers(theo_var("OTLP_HEADERS").as_deref().unwrap_or(""));
        let service_name = theo_var("OTLP_SERVICE_NAME")
            .unwrap_or_else(|| DEFAULT_SERVICE_NAME.into());
        let batch_size = parse_var::<usize>("OTLP_BATCH_SIZE").unwrap_or(DEFAULT_BATCH_SIZE);
        Some(Self {
            endpoint,
            protocol,
            timeout,
            headers,
            service_name,
            batch_size,
        })
    }
}

fn parse_protocol(raw: Option<&str>) -> OtlpProtocol {
    match raw {
        Some("http_protobuf") | Some("http") => OtlpProtocol::HttpProtobuf,
        _ => OtlpProtocol::Grpc,
    }
}

fn parse_timeout(raw: Option<&str>) -> Duration {
    raw.and_then(|v| v.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_TIMEOUT)
}

/// Parse the `OTLP_HEADERS` env var format `"k1=v1,k2=v2"` into pairs.
/// Empty input yields an empty vec; entries without `=` are skipped
/// (best-effort, matches OTel collector behavior).
pub fn parse_headers(raw: &str) -> Vec<(String, String)> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|kv| {
            kv.split_once('=')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum OtelInitError {
    #[error("OTLP exporter build failed: {0}")]
    Exporter(String),
}

/// Initialize the global OTel `TracerProvider` from env vars.
///
/// Returns:
/// - `Ok(Some(provider))` when `OTLP_ENDPOINT` is set; caller should
///   keep the provider alive for the process lifetime and call
///   `provider.shutdown()` at exit (or use `OtlpGuard` for RAII).
/// - `Ok(None)` when `OTLP_ENDPOINT` is absent — operator opt-out.
/// - `Err` only on misconfigured exporter (e.g. invalid endpoint URL).
///
/// MUST be called from within a Tokio runtime context — the
/// `BatchSpanProcessor` uses `tokio::spawn` for its background flush
/// task. Calling outside a runtime panics.
pub fn init_otlp_exporter() -> Result<Option<SdkTracerProvider>, OtelInitError> {
    let cfg = match OtlpExporterConfig::from_env() {
        Some(c) => c,
        None => return Ok(None),
    };
    let exporter = build_exporter(&cfg)?;
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter, TokioRuntime)
        .build();
    opentelemetry::global::set_tracer_provider(provider.clone());
    Ok(Some(provider))
}

/// RAII guard that flushes pending spans on drop. Use at the top of
/// any command that runs inside a Tokio runtime — the guard ensures
/// the provider's background task is given a chance to drain before
/// the runtime is dropped.
///
/// Holds an `Option<SdkTracerProvider>` so `init_otlp_exporter` can
/// be called unconditionally; when the env var is absent the guard
/// is a no-op on drop.
pub struct OtlpGuard {
    provider: Option<SdkTracerProvider>,
}

impl OtlpGuard {
    /// Initialize and stash the provider. `None` when `OTLP_ENDPOINT`
    /// is absent or init fails (the latter logs to stderr; we never
    /// abort the CLI because of telemetry init).
    pub fn install() -> Self {
        let provider = match init_otlp_exporter() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "OTLP init failed; continuing without telemetry");
                None
            }
        };
        Self { provider }
    }

    /// `true` when an OTLP provider is active.
    pub fn is_active(&self) -> bool {
        self.provider.is_some()
    }
}

impl Drop for OtlpGuard {
    fn drop(&mut self) {
        if let Some(p) = self.provider.take() {
            // Best-effort flush — never propagate errors at drop time.
            let _ = p.shutdown();
        }
    }
}

fn build_exporter(cfg: &OtlpExporterConfig) -> Result<SpanExporter, OtelInitError> {
    let result = match cfg.protocol {
        OtlpProtocol::Grpc => SpanExporter::builder()
            .with_tonic()
            .with_endpoint(&cfg.endpoint)
            .with_timeout(cfg.timeout)
            .build(),
        OtlpProtocol::HttpProtobuf => SpanExporter::builder()
            .with_http()
            .with_endpoint(&cfg.endpoint)
            .with_timeout(cfg.timeout)
            .build(),
    };
    result.map_err(|e| OtelInitError::Exporter(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Process-wide env var lock so tests don't race.
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static M: OnceLock<Mutex<()>> = OnceLock::new();
        M.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn clear_env() {
        for k in [
            "OTLP_ENDPOINT",
            "OTLP_PROTOCOL",
            "OTLP_TIMEOUT_SECS",
            "OTLP_HEADERS",
            "OTLP_SERVICE_NAME",
            "OTLP_BATCH_SIZE",
        ] {
            unsafe { std::env::remove_var(k); }
        }
    }

    #[test]
    fn config_from_env_returns_none_when_endpoint_absent() {
        let _g = lock();
        clear_env();
        assert!(OtlpExporterConfig::from_env().is_none());
    }

    #[test]
    fn config_from_env_returns_none_when_endpoint_blank() {
        let _g = lock();
        clear_env();
        unsafe { std::env::set_var("OTLP_ENDPOINT", "   "); }
        let got = OtlpExporterConfig::from_env();
        clear_env();
        assert!(got.is_none(), "blank endpoint must be treated as absent");
    }

    #[test]
    fn config_from_env_returns_some_when_endpoint_set() {
        let _g = lock();
        clear_env();
        unsafe { std::env::set_var("OTLP_ENDPOINT", "http://localhost:4317"); }
        let got = OtlpExporterConfig::from_env().expect("endpoint set");
        clear_env();
        assert_eq!(got.endpoint, "http://localhost:4317");
    }

    #[test]
    fn config_from_env_defaults_protocol_to_grpc() {
        let _g = lock();
        clear_env();
        unsafe { std::env::set_var("OTLP_ENDPOINT", "http://x"); }
        let got = OtlpExporterConfig::from_env().unwrap();
        clear_env();
        assert_eq!(got.protocol, OtlpProtocol::Grpc);
    }

    #[test]
    fn config_from_env_parses_protocol_http_protobuf() {
        let _g = lock();
        clear_env();
        unsafe { std::env::set_var("OTLP_ENDPOINT", "http://x"); }
        unsafe { std::env::set_var("OTLP_PROTOCOL", "http_protobuf"); }
        let got = OtlpExporterConfig::from_env().unwrap();
        clear_env();
        assert_eq!(got.protocol, OtlpProtocol::HttpProtobuf);
    }

    #[test]
    fn config_from_env_parses_protocol_http_alias() {
        let _g = lock();
        clear_env();
        unsafe { std::env::set_var("OTLP_ENDPOINT", "http://x"); }
        unsafe { std::env::set_var("OTLP_PROTOCOL", "http"); }
        let got = OtlpExporterConfig::from_env().unwrap();
        clear_env();
        assert_eq!(got.protocol, OtlpProtocol::HttpProtobuf);
    }

    #[test]
    fn config_from_env_defaults_timeout_to_10s() {
        let _g = lock();
        clear_env();
        unsafe { std::env::set_var("OTLP_ENDPOINT", "http://x"); }
        let got = OtlpExporterConfig::from_env().unwrap();
        clear_env();
        assert_eq!(got.timeout, Duration::from_secs(10));
    }

    #[test]
    fn config_from_env_parses_timeout_seconds() {
        let _g = lock();
        clear_env();
        unsafe { std::env::set_var("OTLP_ENDPOINT", "http://x"); }
        unsafe { std::env::set_var("OTLP_TIMEOUT_SECS", "42"); }
        let got = OtlpExporterConfig::from_env().unwrap();
        clear_env();
        assert_eq!(got.timeout, Duration::from_secs(42));
    }

    #[test]
    fn config_from_env_falls_back_to_default_when_timeout_zero() {
        let _g = lock();
        clear_env();
        unsafe { std::env::set_var("OTLP_ENDPOINT", "http://x"); }
        unsafe { std::env::set_var("OTLP_TIMEOUT_SECS", "0"); }
        let got = OtlpExporterConfig::from_env().unwrap();
        clear_env();
        assert_eq!(got.timeout, Duration::from_secs(10));
    }

    #[test]
    fn config_from_env_falls_back_when_timeout_unparseable() {
        let _g = lock();
        clear_env();
        unsafe { std::env::set_var("OTLP_ENDPOINT", "http://x"); }
        unsafe { std::env::set_var("OTLP_TIMEOUT_SECS", "not-a-number"); }
        let got = OtlpExporterConfig::from_env().unwrap();
        clear_env();
        assert_eq!(got.timeout, Duration::from_secs(10));
    }

    #[test]
    fn config_from_env_parses_multiple_headers() {
        let _g = lock();
        clear_env();
        unsafe { std::env::set_var("OTLP_ENDPOINT", "http://x"); }
        unsafe { std::env::set_var("OTLP_HEADERS", "Authorization=Bearer abc,X-Tenant=t1"); }
        let got = OtlpExporterConfig::from_env().unwrap();
        clear_env();
        assert_eq!(got.headers.len(), 2);
        assert_eq!(got.headers[0], ("Authorization".into(), "Bearer abc".into()));
        assert_eq!(got.headers[1], ("X-Tenant".into(), "t1".into()));
    }

    #[test]
    fn config_from_env_defaults_service_name_to_theo() {
        let _g = lock();
        clear_env();
        unsafe { std::env::set_var("OTLP_ENDPOINT", "http://x"); }
        let got = OtlpExporterConfig::from_env().unwrap();
        clear_env();
        assert_eq!(got.service_name, "theo");
    }

    #[test]
    fn config_from_env_uses_custom_service_name_when_set() {
        let _g = lock();
        clear_env();
        unsafe { std::env::set_var("OTLP_ENDPOINT", "http://x"); }
        unsafe { std::env::set_var("OTLP_SERVICE_NAME", "theo-cli-prod"); }
        let got = OtlpExporterConfig::from_env().unwrap();
        clear_env();
        assert_eq!(got.service_name, "theo-cli-prod");
    }

    #[test]
    fn config_from_env_defaults_batch_size_to_512() {
        let _g = lock();
        clear_env();
        unsafe { std::env::set_var("OTLP_ENDPOINT", "http://x"); }
        let got = OtlpExporterConfig::from_env().unwrap();
        clear_env();
        assert_eq!(got.batch_size, 512);
    }

    #[test]
    fn config_from_env_parses_custom_batch_size() {
        let _g = lock();
        clear_env();
        unsafe { std::env::set_var("OTLP_ENDPOINT", "http://x"); }
        unsafe { std::env::set_var("OTLP_BATCH_SIZE", "100"); }
        let got = OtlpExporterConfig::from_env().unwrap();
        clear_env();
        assert_eq!(got.batch_size, 100);
    }

    #[test]
    fn parse_headers_returns_empty_for_empty_string() {
        assert!(parse_headers("").is_empty());
    }

    #[test]
    fn parse_headers_skips_malformed_entries() {
        // 'no_equals' has no '=', dropped.
        let got = parse_headers("k1=v1,no_equals,k2=v2");
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn parse_headers_trims_whitespace_around_pairs_and_kv() {
        let got = parse_headers(" k1 = v1 , k2 = v2 ");
        assert_eq!(got, vec![
            ("k1".to_string(), "v1".to_string()),
            ("k2".to_string(), "v2".to_string()),
        ]);
    }

    #[test]
    fn init_otlp_exporter_returns_none_when_env_absent() {
        let _g = lock();
        clear_env();
        let result = init_otlp_exporter().expect("must not error when no env set");
        assert!(result.is_none(), "absent endpoint → None (no-op)");
    }

    #[test]
    fn otlp_guard_install_is_inactive_when_env_absent() {
        let _g = lock();
        clear_env();
        let guard = OtlpGuard::install();
        assert!(!guard.is_active(), "no env → guard is inactive");
        drop(guard); // must not panic on no-op drop
    }
}
