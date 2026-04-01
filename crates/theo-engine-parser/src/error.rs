//! Error types for the theo-code-parser crate.

use std::path::PathBuf;

/// All errors produced by the parser crate.
#[derive(Debug, thiserror::Error)]
pub enum ParserError {
    #[error("unsupported language for file: {path}")]
    UnsupportedLanguage { path: PathBuf },

    #[error("parse failed for {path}: {reason}")]
    ParseFailed { path: PathBuf, reason: String },

    #[error("extraction failed for {path}: {reason}")]
    ExtractionFailed { path: PathBuf, reason: String },

    #[error("workspace detection failed for {path}: {reason}")]
    WorkspaceDetection { path: PathBuf, reason: String },

    #[error("IO error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },

    #[error("JSON serialization error: {source}")]
    Json {
        #[from]
        source: serde_json::Error,
    },

    #[error("walkdir error: {source}")]
    WalkDir {
        #[from]
        source: walkdir::Error,
    },
}

pub type Result<T> = std::result::Result<T, ParserError>;
