//! Input processing for the REPL.
//!
//! Provides rustyline helpers:
//! - [`completer`]: tab completion for `/commands` and `@file` mentions
//! - [`mention`]: parsing and reading `@file` mentions in user input
//! - [`multiline`]: detection of triple-backtick multi-line input
//! - [`stdin_buffer`]: batching fragmented stdin into complete escape sequences

pub mod completer;
pub mod highlighter;
pub mod hinter;
pub mod keyboard;
pub mod mention;
pub mod model_selector;
pub mod multiline;
pub mod stdin_buffer;
