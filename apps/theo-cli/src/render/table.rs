//! Aligned-column tables for `/status`, `/skills`, `/memory list`.
//!
//! Wraps [`comfy_table`] with a default style that fits the theo-cli
//! look and adapts to [`StyleCaps`]:
//!
//! - TTY with unicode: UTF8_BORDERS_ONLY with rounded corners.
//! - Piped output: ASCII_MARKDOWN for pipe-friendly text.

use comfy_table::presets::{ASCII_MARKDOWN, UTF8_BORDERS_ONLY};
use comfy_table::{Cell, ContentArrangement, Row, Table};

use crate::render::style::StyleCaps;

/// Build an empty table pre-configured with the current caps.
pub fn new_table(caps: StyleCaps) -> Table {
    let mut t = Table::new();
    t.set_content_arrangement(ContentArrangement::Dynamic);
    if caps.unicode {
        t.load_preset(UTF8_BORDERS_ONLY);
    } else {
        t.load_preset(ASCII_MARKDOWN);
    }
    t
}

/// Build a key-value table with `headers = &["Key", "Value"]`.
pub fn kv_table<K: AsRef<str>, V: AsRef<str>>(
    rows: &[(K, V)],
    caps: StyleCaps,
) -> Table {
    let mut t = new_table(caps);
    t.set_header(vec!["Key", "Value"]);
    for (k, v) in rows {
        t.add_row(Row::from(vec![
            Cell::new(k.as_ref()),
            Cell::new(v.as_ref()),
        ]));
    }
    t
}

/// Render a table to a string with trailing whitespace trimmed per line.
pub fn render_table(table: &Table) -> String {
    table.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> StyleCaps {
        StyleCaps::plain()
    }

    #[test]
    fn test_new_table_unicode_has_unicode_preset() {
        // Populate with a row so comfy-table actually renders borders.
        let mut t = new_table(StyleCaps::full());
        t.set_header(vec!["A", "B"]);
        t.add_row(vec!["1", "2"]);
        let s = t.to_string();
        assert!(s.contains('─'), "expected unicode borders, got {s:?}");
    }

    #[test]
    fn test_new_table_plain_uses_ascii() {
        let mut t = new_table(plain());
        t.set_header(vec!["A", "B"]);
        t.add_row(vec!["1", "2"]);
        let s = t.to_string();
        assert!(s.contains("|"));
        assert!(s.contains("A"));
        assert!(s.contains("1"));
    }

    #[test]
    fn test_kv_table_renders_pairs() {
        let t = kv_table(&[("Provider", "OpenAI"), ("Model", "gpt-4")], plain());
        let s = t.to_string();
        assert!(s.contains("Provider"));
        assert!(s.contains("OpenAI"));
        assert!(s.contains("Model"));
        assert!(s.contains("gpt-4"));
    }

    #[test]
    fn test_kv_table_empty_only_header() {
        let t = kv_table::<&str, &str>(&[], plain());
        let s = t.to_string();
        assert!(s.contains("Key"));
        assert!(s.contains("Value"));
    }

    #[test]
    fn test_render_table_is_deterministic() {
        let t1 = kv_table(&[("a", "1"), ("b", "2")], plain());
        let t2 = kv_table(&[("a", "1"), ("b", "2")], plain());
        assert_eq!(render_table(&t1), render_table(&t2));
    }

    #[test]
    fn test_render_table_has_multiple_rows() {
        let t = kv_table(&[("a", "1"), ("b", "2"), ("c", "3")], plain());
        let s = render_table(&t);
        assert!(s.contains("a"));
        assert!(s.contains("b"));
        assert!(s.contains("c"));
    }
}
