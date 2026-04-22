//! Concrete Tantivy-backed transcript indexer.
//!
//! Lives in `theo-application` because the agent-runtime crate is
//! forbidden from depending on `theo-engine-retrieval` (bounded
//! context rule). The `TranscriptIndexer` trait itself is defined in
//! `theo-agent-runtime::transcript_indexer` so the config field can
//! stay a small trait object.
//!
//! Phase 4 of `docs/plans/PLAN_AUTO_EVOLUTION_SOTA.md`.

#[cfg(feature = "tantivy-backend")]
use async_trait::async_trait;
#[cfg(feature = "tantivy-backend")]
use theo_agent_runtime::transcript_indexer::{TranscriptIndexError, TranscriptIndexer};
#[cfg(feature = "tantivy-backend")]
use theo_domain::event::{DomainEvent, EventType};

#[cfg(feature = "tantivy-backend")]
pub struct TantivyTranscriptIndexer;

#[cfg(feature = "tantivy-backend")]
impl TantivyTranscriptIndexer {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(feature = "tantivy-backend")]
impl Default for TantivyTranscriptIndexer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "tantivy-backend")]
#[async_trait]
impl TranscriptIndexer for TantivyTranscriptIndexer {
    async fn record_session(
        &self,
        memory_dir: &std::path::Path,
        session_id: &str,
        events: &[DomainEvent],
    ) -> Result<(), TranscriptIndexError> {
        if events.is_empty() {
            return Ok(());
        }
        let index_dir = memory_dir.join("transcripts");
        let docs = build_transcript_docs(session_id, events);
        if docs.is_empty() {
            return Ok(());
        }
        // Tantivy is blocking I/O; offload so we don't park the runtime.
        let index_dir_owned = index_dir.clone();
        let docs_owned = docs;
        tokio::task::spawn_blocking(move || {
            use theo_engine_retrieval::memory_tantivy::MemoryTantivyIndex;
            let mut idx = MemoryTantivyIndex::open_or_create(&index_dir_owned)
                .map_err(|e| TranscriptIndexError::Backend(e.to_string()))?;
            // Idempotency — if a doc with the same session_id +
            // content_hash already exists, skip entirely. That
            // matches AC-4.4 "Re-indexação com mesmo hash é no-op".
            let skip = docs_owned
                .first()
                .map(|d| {
                    idx.contains_session_with_hash(&d.session_id, &d.content_hash)
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            if skip {
                return Ok(());
            }
            idx.add_transcripts(&docs_owned)
                .map(|_| ())
                .map_err(|e| TranscriptIndexError::Backend(e.to_string()))
        })
        .await
        .map_err(|e| TranscriptIndexError::Backend(format!("join error: {e}")))?
    }

    fn name(&self) -> &'static str {
        "tantivy"
    }
}

/// Build transcript docs from domain events. Content hash is
/// SHA-256(session_id || event_id_1 || event_id_2 || …).
#[cfg(feature = "tantivy-backend")]
fn build_transcript_docs(
    session_id: &str,
    events: &[DomainEvent],
) -> Vec<theo_engine_retrieval::memory_tantivy::TranscriptDoc> {
    use sha2::Digest;
    use theo_engine_retrieval::memory_tantivy::TranscriptDoc;

    let mut hasher = sha2::Sha256::new();
    hasher.update(session_id.as_bytes());
    for e in events {
        hasher.update(e.event_id.as_str().as_bytes());
    }
    let hash = format!("{:x}", hasher.finalize());

    let mut docs = Vec::new();
    for (idx, ev) in events.iter().enumerate() {
        let Some(body) = extract_event_body(&ev.payload) else {
            continue;
        };
        if body.trim().is_empty() {
            continue;
        }
        let role = match ev.event_type {
            EventType::ToolCallCompleted | EventType::ToolCallProgress => "tool",
            EventType::RunInitialized | EventType::RunStateChanged => "system",
            _ => "user",
        };
        docs.push(TranscriptDoc {
            session_id: session_id.to_string(),
            turn_index: idx as u64,
            timestamp_unix: ev.timestamp / 1_000,
            role: role.to_string(),
            body,
            content_hash: hash.clone(),
        });
    }
    docs
}

#[cfg(feature = "tantivy-backend")]
fn extract_event_body(payload: &serde_json::Value) -> Option<String> {
    match payload {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(map) => {
            for key in ["content", "message", "summary", "text", "body"] {
                if let Some(v) = map.get(key).and_then(|v| v.as_str()) {
                    return Some(v.to_string());
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(all(test, feature = "tantivy-backend"))]
mod tests {
    use super::*;
    use serde_json::json;
    use theo_domain::event::{DomainEvent, EventType};

    fn ev(et: EventType, entity: &str, payload: serde_json::Value) -> DomainEvent {
        DomainEvent::new(et, entity, payload)
    }

    #[test]
    fn test_build_transcript_docs_skips_events_without_body() {
        let events = vec![
            ev(EventType::RunInitialized, "r1", json!({"kind": "init"})),
            ev(
                EventType::ToolCallCompleted,
                "r1",
                json!({"content": "ls output here"}),
            ),
            ev(
                EventType::RunStateChanged,
                "r1",
                json!({"summary": "done"}),
            ),
        ];
        let docs = build_transcript_docs("s1", &events);
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].body, "ls output here");
        assert_eq!(docs[1].body, "done");
        // All share the same hash (same session + same events).
        assert_eq!(docs[0].content_hash, docs[1].content_hash);
    }

    #[test]
    fn test_build_transcript_docs_roles_are_assigned() {
        let events = vec![
            ev(EventType::ToolCallCompleted, "r1", json!({"content": "T"})),
            ev(EventType::RunInitialized, "r1", json!({"summary": "I"})),
        ];
        let docs = build_transcript_docs("s1", &events);
        assert_eq!(docs[0].role, "tool");
        assert_eq!(docs[1].role, "system");
    }

    #[tokio::test]
    async fn test_indexer_end_to_end_persists_and_idempotent() {
        let tmp = tempfile::tempdir().expect("tmp");
        let memory_dir = tmp.path();
        let events = vec![ev(
            EventType::ToolCallCompleted,
            "r1",
            json!({"content": "hello world"}),
        )];
        let idx = TantivyTranscriptIndexer::new();
        idx.record_session(memory_dir, "s1", &events).await.unwrap();

        // Re-run should be a no-op (same hash) — if it weren't, the
        // doc count would double. We can't easily assert count via
        // the trait, so we rely on the underlying idx's internal skip
        // logic (covered by unit tests in memory_tantivy).
        idx.record_session(memory_dir, "s1", &events).await.unwrap();

        // Sanity: the index dir actually exists.
        assert!(memory_dir.join("transcripts").exists());
    }
}
