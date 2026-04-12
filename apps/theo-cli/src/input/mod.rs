//! Input processing for the REPL.
//!
//! Provides rustyline helpers:
//! - [`completer`]: tab completion for `/commands` and `@file` mentions
//! - [`mention`]: parsing and reading `@file` mentions in user input
//! - [`multiline`]: detection of triple-backtick multi-line input

pub mod completer;
pub mod highlighter;
pub mod hinter;
pub mod mention;
pub mod multiline;
