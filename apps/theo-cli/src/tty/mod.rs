//! Terminal capability detection.
//!
//! Determines whether stderr/stdout is a real TTY, whether color is
//! enabled, and caches terminal width with SIGWINCH invalidation.

pub mod caps;
pub mod resize;

pub use caps::TtyCaps;
pub use resize::{current_width, install_resize_listener, set_width};
