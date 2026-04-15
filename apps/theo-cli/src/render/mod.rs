//! Terminal rendering subsystem.
//!
//! All ANSI escape sequences MUST be emitted through this module.
//! Direct `\x1b[...]` literals outside `render::style` are a CI error.

pub mod banner;
pub mod code_block;
pub mod diff;
pub mod errors;
pub mod markdown;
pub mod progress;
pub mod streaming;
pub mod style;
pub mod table;
pub mod tool_result;
