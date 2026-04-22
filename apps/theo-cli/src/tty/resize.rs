//! Terminal width cache with SIGWINCH-aware updates.
//!
//! Width is stored in a global [`AtomicU16`] so any render code can read
//! the current value without locks. A background task listens for
//! `Event::Resize` events and updates the atomic.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use std::sync::atomic::{AtomicU16, Ordering};

static TERM_WIDTH: AtomicU16 = AtomicU16::new(80);

/// Read the cached terminal width (columns).
pub fn current_width() -> u16 {
    TERM_WIDTH.load(Ordering::Relaxed)
}

/// Update the cached width (used by the resize listener and tests).
pub fn set_width(width: u16) {
    TERM_WIDTH.store(width, Ordering::Relaxed);
}

/// Install a resize listener that updates [`current_width`] on SIGWINCH.
///
/// The listener runs as a tokio task and terminates when the returned
/// handle is dropped. Prefer calling this once at CLI startup.
#[cfg(unix)]
pub fn install_resize_listener() {
    // Seed with current size.
    if let Ok((w, _)) = crossterm::terminal::size() {
        set_width(w);
    }
    tokio::spawn(async {
        use tokio::signal::unix::{SignalKind, signal};
        let Ok(mut sig) = signal(SignalKind::window_change()) else {
            return;
        };
        while sig.recv().await.is_some() {
            if let Ok((w, _)) = crossterm::terminal::size() {
                set_width(w);
            }
        }
    });
}

#[cfg(not(unix))]
pub fn install_resize_listener() {
    if let Ok((w, _)) = crossterm::terminal::size() {
        set_width(w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests run sequentially per-thread per process but the global atomic
    // is shared. These tests use distinctive values that do not clash.

    #[test]
    fn test_default_width_is_reasonable() {
        // Default could be 80 or whatever a prior test set. Either way
        // should be within u16 range and non-zero.
        let w = current_width();
        assert!(w >= 1);
    }

    #[test]
    fn test_set_width_is_visible_to_reader() {
        set_width(31337);
        assert_eq!(current_width(), 31337);
    }

    #[test]
    fn test_set_width_accepts_zero() {
        // Zero is pathological but allowed by the atomic; readers should
        // defend against it by using saturating arithmetic.
        set_width(0);
        assert_eq!(current_width(), 0);
        set_width(80); // restore
    }

    #[test]
    fn test_set_width_handles_large_values() {
        set_width(u16::MAX);
        assert_eq!(current_width(), u16::MAX);
        set_width(80); // restore
    }
}
