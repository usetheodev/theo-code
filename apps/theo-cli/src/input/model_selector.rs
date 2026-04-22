//! Model selector for Ctrl+P cycling.
//!
//! Provides infrastructure for cycling through configured model entries.
//! The actual keybinding integration with rustyline happens at a higher level.
//!
//! Pi-mono ref: `packages/coding-agent/src/core/keybindings.ts`

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
/// A configured model entry for cycling.
#[derive(Debug, Clone)]
pub struct ModelEntry {
    pub provider: String,
    pub model_id: String,
    pub display_name: String,
    pub thinking_level: Option<String>,
}

/// Model selector that cycles through configured entries.
pub struct ModelSelector {
    entries: Vec<ModelEntry>,
    current: usize,
}

impl ModelSelector {
    pub fn new(entries: Vec<ModelEntry>) -> Self {
        Self {
            entries,
            current: 0,
        }
    }

    pub fn current(&self) -> Option<&ModelEntry> {
        self.entries.get(self.current)
    }

    /// Cycle to the next model. Wraps around.
    pub fn next(&mut self) -> Option<&ModelEntry> {
        if self.entries.is_empty() {
            return None;
        }
        self.current = (self.current + 1) % self.entries.len();
        self.entries.get(self.current)
    }

    /// Cycle to the previous model. Wraps around.
    pub fn prev(&mut self) -> Option<&ModelEntry> {
        if self.entries.is_empty() {
            return None;
        }
        self.current = if self.current == 0 {
            self.entries.len() - 1
        } else {
            self.current - 1
        };
        self.entries.get(self.current)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(name: &str) -> ModelEntry {
        ModelEntry {
            provider: "test".to_string(),
            model_id: name.to_string(),
            display_name: name.to_string(),
            thinking_level: None,
        }
    }

    #[test]
    fn test_empty_selector_returns_none() {
        let mut selector = ModelSelector::new(vec![]);
        assert!(selector.current().is_none());
        assert!(selector.next().is_none());
        assert!(selector.prev().is_none());
        assert!(selector.is_empty());
        assert_eq!(selector.len(), 0);
    }

    #[test]
    fn test_single_entry_cycling() {
        let mut selector = ModelSelector::new(vec![make_entry("gpt-4o")]);

        assert_eq!(selector.current().map(|e| e.model_id.as_str()), Some("gpt-4o"));
        assert_eq!(selector.len(), 1);
        assert!(!selector.is_empty());

        // Next wraps back to the same entry.
        assert_eq!(selector.next().map(|e| e.model_id.as_str()), Some("gpt-4o"));
        // Prev wraps back to the same entry.
        assert_eq!(selector.prev().map(|e| e.model_id.as_str()), Some("gpt-4o"));
    }

    #[test]
    fn test_multi_entry_next_wraps() {
        let mut selector = ModelSelector::new(vec![
            make_entry("model-a"),
            make_entry("model-b"),
            make_entry("model-c"),
        ]);

        // Starts at 0.
        assert_eq!(selector.current().map(|e| e.model_id.as_str()), Some("model-a"));

        // Next cycles forward.
        assert_eq!(selector.next().map(|e| e.model_id.as_str()), Some("model-b"));
        assert_eq!(selector.next().map(|e| e.model_id.as_str()), Some("model-c"));

        // Wraps around to first.
        assert_eq!(selector.next().map(|e| e.model_id.as_str()), Some("model-a"));
    }

    #[test]
    fn test_multi_entry_prev_wraps() {
        let mut selector = ModelSelector::new(vec![
            make_entry("model-a"),
            make_entry("model-b"),
            make_entry("model-c"),
        ]);

        // Starts at 0, prev wraps to last.
        assert_eq!(selector.prev().map(|e| e.model_id.as_str()), Some("model-c"));
        assert_eq!(selector.prev().map(|e| e.model_id.as_str()), Some("model-b"));
        assert_eq!(selector.prev().map(|e| e.model_id.as_str()), Some("model-a"));
    }

    #[test]
    fn test_next_then_prev_returns_to_original() {
        let mut selector = ModelSelector::new(vec![
            make_entry("model-a"),
            make_entry("model-b"),
            make_entry("model-c"),
        ]);

        selector.next(); // -> model-b
        selector.next(); // -> model-c
        selector.prev(); // -> model-b
        assert_eq!(selector.current().map(|e| e.model_id.as_str()), Some("model-b"));
    }
}
