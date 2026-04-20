//! Memory wiki subsystem: hash manifest (RM5a) + lint (RM5a).
//! Compiler (RM5b) lives in a sibling module — not part of RM5a.

pub mod compiler;
pub mod hash;
pub mod lint;

pub use compiler::{
    CompileBudget, CompiledPage, CompiledWiki, CompilerClient, CompilerResponse, SourceDoc,
    compile, render_frontmatter,
};
pub use hash::{HashManifest, SourceHash};
pub use lint::{lint_pages, parse_page};
