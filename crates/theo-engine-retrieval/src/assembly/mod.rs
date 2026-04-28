//! Greedy context assembly (T4.3 split, D5).
//!
//! Sub-modules:
//!   - types.rs    — ContextItem, ContextPayload, estimate_tokens
//!   - content.rs  — community_content
//!   - greedy.rs   — assemble_greedy, assemble_with_summaries
//!   - reading.rs  — file read + symbol collection helpers
//!   - building.rs — content builders (code, signature, compressed)
//!   - codeasm.rs  — assemble_with_code
//!   - sym.rs      — assemble_by_symbol
//!   - direct.rs   — assemble_files_direct + with_inline_skip variant

mod building;
mod codeasm;
mod content;
mod direct;
mod greedy;
mod reading;
mod sym;
mod types;

pub use building::*;
pub use codeasm::*;
pub use content::*;
pub use direct::*;
pub use greedy::*;
pub use reading::*;
pub use sym::*;
pub use types::*;

#[cfg(test)]
#[path = "assembly_tests.rs"]
mod tests;
