//! Terminal capability detection.
//!
//! Determines whether stderr/stdout is a real TTY, whether color is
//! enabled, and caches terminal width with SIGWINCH invalidation.

pub mod caps;
pub mod resize;

pub mod caps;

pub use caps::TtyCaps;
