//! Progress indicators (spinners, bars) via `indicatif`.
//!
//! When [`StyleCaps::colors`] is false, these become no-ops — piped
//! output remains free of terminal control sequences.

use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

use crate::render::style::StyleCaps;

/// A spinner handle that cleans up on drop.
///
/// When caps disable output, this is a no-op wrapper.
pub struct Spinner {
    bar: Option<ProgressBar>,
}

impl Spinner {
    /// Start a new spinner with `message`.
    ///
    /// If `caps.colors` is false, returns a no-op spinner that does
    /// nothing visible but preserves the call surface so the caller
    /// does not have to branch.
    pub fn start(message: impl Into<String>, caps: StyleCaps) -> Self {
        if !caps.colors {
            return Self { bar: None };
        }
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&[
                    "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✓",
                ])
                .template("{spinner:.cyan} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        bar.set_message(message.into());
        bar.enable_steady_tick(Duration::from_millis(100));
        Self { bar: Some(bar) }
    }

    /// Update the spinner message.
    pub fn set_message(&self, msg: impl Into<String>) {
        if let Some(bar) = &self.bar {
            bar.set_message(msg.into());
        }
    }

    /// Finish the spinner with a final message.
    pub fn finish(mut self, msg: impl Into<String>) {
        if let Some(bar) = self.bar.take() {
            bar.finish_with_message(msg.into());
        }
    }

    /// Finish with a cleared line (no final message).
    pub fn finish_and_clear(mut self) {
        if let Some(bar) = self.bar.take() {
            bar.finish_and_clear();
        }
    }

    /// Returns true if the spinner is live (colors enabled).
    pub fn is_live(&self) -> bool {
        self.bar.is_some()
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        if let Some(bar) = self.bar.take() {
            bar.finish_and_clear();
        }
    }
}

/// A discrete progress bar for known-size operations.
pub struct Bar {
    bar: Option<ProgressBar>,
}

impl Bar {
    /// Start a progress bar with a total number of steps.
    pub fn start(total: u64, message: impl Into<String>, caps: StyleCaps) -> Self {
        if !caps.colors {
            return Self { bar: None };
        }
        let bar = ProgressBar::new(total);
        bar.set_style(
            ProgressStyle::default_bar()
                .template("  {msg} [{bar:40.cyan/dim}] {pos}/{len}")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("█▓▒░ "),
        );
        bar.set_message(message.into());
        Self { bar: Some(bar) }
    }

    /// Advance by `delta` steps.
    pub fn inc(&self, delta: u64) {
        if let Some(bar) = &self.bar {
            bar.inc(delta);
        }
    }

    /// Finish the bar with a final message.
    pub fn finish(mut self, msg: impl Into<String>) {
        if let Some(bar) = self.bar.take() {
            bar.finish_with_message(msg.into());
        }
    }

    pub fn is_live(&self) -> bool {
        self.bar.is_some()
    }
}

impl Drop for Bar {
    fn drop(&mut self) {
        if let Some(bar) = self.bar.take() {
            bar.finish_and_clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> StyleCaps {
        StyleCaps::plain()
    }

    fn tty() -> StyleCaps {
        StyleCaps::full()
    }

    #[test]
    fn test_spinner_plain_is_noop() {
        let s = Spinner::start("loading", plain());
        assert!(!s.is_live());
    }

    #[test]
    fn test_spinner_tty_is_live() {
        let s = Spinner::start("loading", tty());
        assert!(s.is_live());
        s.finish("done");
    }

    #[test]
    fn test_spinner_finish_and_clear_is_safe() {
        let s = Spinner::start("x", tty());
        s.finish_and_clear();
    }

    #[test]
    fn test_spinner_set_message_plain_is_safe() {
        let s = Spinner::start("a", plain());
        s.set_message("b");
        assert!(!s.is_live());
    }

    #[test]
    fn test_spinner_drop_is_safe() {
        {
            let _s = Spinner::start("dropped", tty());
        }
    }

    #[test]
    fn test_bar_plain_is_noop() {
        let b = Bar::start(100, "work", plain());
        assert!(!b.is_live());
        b.inc(10);
    }

    #[test]
    fn test_bar_tty_is_live() {
        let b = Bar::start(10, "work", tty());
        assert!(b.is_live());
        b.inc(5);
        b.finish("done");
    }

    #[test]
    fn test_bar_drop_is_safe() {
        {
            let _b = Bar::start(5, "x", tty());
        }
    }
}
