//! Wiki engine errors — typed, contextual, never generic strings.

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WikiError {
    #[error("wiki store I/O failed for path `{path}`: {source}")]
    StoreFailed {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("skeleton extraction failed: {reason}")]
    SkeletonFailed { reason: String },

    #[error("enrichment failed for page `{slug}`: {reason}")]
    EnrichmentFailed { slug: String, reason: String },

    #[error("lint violation: {rule} on page `{slug}`")]
    LintViolation { rule: String, slug: String },

    #[error("hash manifest corrupted: {reason}")]
    HashCorrupted { reason: String },

    #[error("page not found: `{slug}`")]
    PageNotFound { slug: String },
}

pub type WikiResult<T> = Result<T, WikiError>;
