//! Uniform diagnostics for best-effort filesystem operations.
//!
//! REMEDIATION_PLAN T2.3 + T2.4: swaps bare `let _ = tokio::fs::...` and
//! `let _ = std::fs::...` for a consistent `warn_fs_error(site, path, err)`
//! helper. Operations stay best-effort (no caller abort) but every failure
//! is now observable on stderr with enough context to diagnose.
//!
//! When the agent has an `EventBus`, `emit_fs_error` also publishes a
//! `DomainEvent::Error { type: "fs" }` so the observability pipeline can
//! alert on silent persistence failures (previously impossible — the
//! existing `let _ = ...` pattern lost the error entirely).

use std::path::Path;
use std::sync::Arc;

use theo_domain::event::{DomainEvent, EventType};

use crate::event_bus::EventBus;

/// Stderr diagnostic for a failed filesystem operation. Use at every
/// best-effort fs call that does NOT have an `EventBus` in scope
/// (utility modules, path discovery helpers).
///
/// `site` is a static identifier of the call site (e.g., "failure_tracker/save")
/// that operators can grep for when a warning appears.
pub fn warn_fs_error(site: &'static str, path: &Path, err: &impl std::fmt::Display) {
    tracing::warn!(site = site, path = %path.display(), error = %err, "fs error");
}

/// Same as [`warn_fs_error`] but also emits a `DomainEvent::Error` so the
/// observability pipeline can record the failure as part of the run.
///
/// Used by `record_session_exit` and other code paths that own an
/// `EventBus` and where silent persistence failures were previously
/// invisible to the episode log.
pub fn emit_fs_error(
    bus: &Arc<EventBus>,
    entity_id: &str,
    site: &'static str,
    path: &Path,
    err: &impl std::fmt::Display,
) {
    let msg = format!("{err}");
    tracing::warn!(site = site, path = %path.display(), error = %msg, "fs error");
    bus.publish(DomainEvent::new(
        EventType::Error,
        entity_id,
        serde_json::json!({
            "type": "fs",
            "site": site,
            "path": path.display().to_string(),
            "error": msg,
        }),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::CapturingListener;

    #[test]
    fn warn_fs_error_does_not_panic() {
        // Smoke: must never abort the caller even with exotic errors.
        warn_fs_error(
            "unit_test/site",
            Path::new("/nonexistent/xyz"),
            &"disk on fire",
        );
    }

    #[test]
    fn emit_fs_error_publishes_typed_error_event() {
        let bus = Arc::new(EventBus::new());
        let listener = Arc::new(CapturingListener::new());
        bus.subscribe(listener.clone());

        emit_fs_error(
            &bus,
            "run-42",
            "record_session_exit/metrics",
            Path::new("/readonly/path.json"),
            &"permission denied",
        );

        let events = listener.captured();
        assert_eq!(events.len(), 1);
        let ev = &events[0];
        assert_eq!(ev.event_type, EventType::Error);
        assert_eq!(ev.entity_id, "run-42");
        assert_eq!(ev.payload["type"], "fs");
        assert_eq!(ev.payload["site"], "record_session_exit/metrics");
        assert_eq!(ev.payload["error"], "permission denied");
        assert_eq!(ev.payload["path"], "/readonly/path.json");
    }

    #[test]
    fn emit_fs_error_with_no_listeners_still_logs() {
        // Bus with no listeners must not break either.
        let bus = Arc::new(EventBus::new());
        emit_fs_error(
            &bus,
            "r",
            "t",
            Path::new("/tmp/x"),
            &std::io::Error::other("boom"),
        );
        assert_eq!(bus.len(), 1, "event is logged even without listeners");
    }
}
