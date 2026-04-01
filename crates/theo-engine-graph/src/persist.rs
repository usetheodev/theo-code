/// Serialization / deserialization of `CodeGraph` to disk.
///
/// Uses `bincode` (v1) for compact binary encoding. The canonical location is
/// `.theo/graph.bin` relative to the repository root, but callers supply the
/// path explicitly.
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use crate::model::CodeGraph;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum PersistError {
    Io(io::Error),
    Encode(bincode::Error),
    Decode(bincode::Error),
}

impl std::fmt::Display for PersistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PersistError::Io(e) => write!(f, "I/O error: {e}"),
            PersistError::Encode(e) => write!(f, "encode error: {e}"),
            PersistError::Decode(e) => write!(f, "decode error: {e}"),
        }
    }
}

impl std::error::Error for PersistError {}

impl From<io::Error> for PersistError {
    fn from(e: io::Error) -> Self {
        PersistError::Io(e)
    }
}

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------

/// Serialize `graph` to `path` using bincode.
///
/// Creates parent directories if they do not exist.
pub fn save(graph: &CodeGraph, path: &Path) -> Result<(), PersistError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = bincode::serialize(graph).map_err(PersistError::Encode)?;
    let mut file = fs::File::create(path)?;
    file.write_all(&bytes)?;
    Ok(())
}

/// Deserialize a `CodeGraph` from `path`.
pub fn load(path: &Path) -> Result<CodeGraph, PersistError> {
    let mut file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let mut graph: CodeGraph = bincode::deserialize(&bytes).map_err(PersistError::Decode)?;
    // Rebuild the contains_children_index (old serialized graphs may not have it).
    graph.rebuild_contains_index();
    Ok(graph)
}
