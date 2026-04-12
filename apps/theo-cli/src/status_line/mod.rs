//! Status line formatting.
//!
//! A compact single-line summary of session state: mode, model,
//! tokens, cost (if tracked). Re-rendered on-demand (not persistent
//! via alternate screen — see ADR-002).

pub mod format;

pub use format::{Segment, StatusLine, render_status};
