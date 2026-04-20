//! Memory wiki subsystem: hash manifest (RM5a) + lint (RM5a).
//! Compiler (RM5b) lives in a sibling module — not part of RM5a.

pub mod hash;
pub mod lint;

pub use hash::{HashManifest, SourceHash};
pub use lint::{lint_pages, parse_page};
