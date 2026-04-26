//! Transcript indexer trait — wiring of
//! `PLAN_AUTO_EVOLUTION_SOTA`.
//!
//! The concrete implementation lives in `theo-application` so this
//! crate can stay inside its bounded context (`theo-agent-runtime`
//! must NOT depend on `theo-engine-retrieval`).

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use theo_domain::event::DomainEvent;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TranscriptIndexError {
    #[error("transcript indexer I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("transcript indexer backend failure: {0}")]
    Backend(String),
}

#[async_trait]
pub trait TranscriptIndexer: Send + Sync {
    /// Index the events produced during `session_id`. Callers treat
    /// the call as fire-and-forget — errors are logged, not bubbled
    /// back into the main loop. Implementations must be idempotent
    /// (same session + same events should not create duplicate docs).
    async fn record_session(
        &self,
        memory_dir: &Path,
        session_id: &str,
        events: &[DomainEvent],
    ) -> Result<(), TranscriptIndexError>;

    fn name(&self) -> &'static str;
}

/// No-op indexer. Default value of the config field so existing tests
/// and headless runs need no changes.
#[derive(Debug, Default, Clone)]
pub struct NullTranscriptIndexer;

#[async_trait]
impl TranscriptIndexer for NullTranscriptIndexer {
    async fn record_session(
        &self,
        _: &Path,
        _: &str,
        _: &[DomainEvent],
    ) -> Result<(), TranscriptIndexError> {
        Ok(())
    }
    fn name(&self) -> &'static str {
        "null"
    }
}

#[derive(Clone)]
pub struct TranscriptIndexerHandle(pub Arc<dyn TranscriptIndexer>);

impl TranscriptIndexerHandle {
    pub fn new(i: Arc<dyn TranscriptIndexer>) -> Self {
        Self(i)
    }
    pub fn as_indexer(&self) -> &dyn TranscriptIndexer {
        self.0.as_ref()
    }
}

impl std::fmt::Debug for TranscriptIndexerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("TranscriptIndexerHandle")
            .field(&self.0.name())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_null_indexer_is_ok() {
        let n = NullTranscriptIndexer;
        assert!(n.record_session(Path::new("/tmp"), "s1", &[]).await.is_ok());
    }
}
