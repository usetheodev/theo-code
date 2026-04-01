//! Generic fallback extractor for languages without dedicated extractors.
//!
//! Uses heuristic text matching on call-like CST nodes to detect:
//! - Log sinks (console.log, logger.info, logging.warning, etc.)
//! - PII in log arguments
//!
//! Does NOT extract routes, HTTP calls, or imports — those require
//! framework-specific knowledge that belongs in dedicated extractors.
//! This extractor ensures every parsed file contributes at least
//! log-sink and PII data to the CodeModel.

use std::path::Path;

use tree_sitter::{Node, Tree};

use crate::types::*;
use crate::tree_sitter::SupportedLanguage;

use super::common::{self, try_extract_log_sink};
use super::language_behavior::behavior_for;

/// Extract semantic information from any language using generic heuristics.
pub fn extract(
    file_path: &Path,
    source: &str,
    tree: &Tree,
    language: SupportedLanguage,
) -> FileExtraction {
    let root = tree.root_node();
    let call_kinds = behavior_for(language).call_node_kinds();
    let mut extraction = common::new_extraction(file_path, language);

    extract_recursive(&root, source, file_path, call_kinds, &mut extraction);

    extraction
}

fn extract_recursive(
    node: &Node,
    source: &str,
    file_path: &Path,
    call_kinds: &[&str],
    extraction: &mut FileExtraction,
) {
    if call_kinds.contains(&node.kind()) {
        try_extract_log_sink(node, source, file_path, extraction);
    }

    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i as u32) {
            extract_recursive(&child, source, file_path, call_kinds, extraction);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::tree_sitter;

    fn extract_python(source: &str) -> FileExtraction {
        let path = PathBuf::from("test.py");
        let parsed = crate::tree_sitter::parse_source(&path, source, SupportedLanguage::Python, None).unwrap();
        extract(&path, source, &parsed.tree, SupportedLanguage::Python)
    }

    fn extract_java(source: &str) -> FileExtraction {
        let path = PathBuf::from("Test.java");
        let parsed = crate::tree_sitter::parse_source(&path, source, SupportedLanguage::Java, None).unwrap();
        extract(&path, source, &parsed.tree, SupportedLanguage::Java)
    }

    fn extract_go(source: &str) -> FileExtraction {
        let path = PathBuf::from("main.go");
        let parsed = crate::tree_sitter::parse_source(&path, source, SupportedLanguage::Go, None).unwrap();
        extract(&path, source, &parsed.tree, SupportedLanguage::Go)
    }

    #[test]
    fn detects_python_logging_sink() {
        let ext = extract_python(
            r#"
import logging
logging.info("Processing request")
logging.error("Failed to connect")
"#,
        );
        assert_eq!(ext.sinks.len(), 2);
        assert_eq!(ext.sinks[0].sink_type, SinkType::Log);
    }

    #[test]
    fn detects_python_logger_sink() {
        let ext = extract_python(
            r#"
logger = logging.getLogger(__name__)
logger.warning("Slow query detected")
"#,
        );
        assert_eq!(ext.sinks.len(), 1);
    }

    #[test]
    fn detects_pii_in_python_log() {
        let ext = extract_python(
            r#"
logging.info("User email: %s", user.email)
"#,
        );
        assert_eq!(ext.sinks.len(), 1);
        assert!(ext.sinks[0].contains_pii);
    }

    #[test]
    fn detects_java_logger_sink() {
        let ext = extract_java(
            r#"
public class App {
    public void handle() {
        Logger.info("Request received");
        Logger.error("Connection failed");
    }
}
"#,
        );
        assert_eq!(ext.sinks.len(), 2);
    }

    #[test]
    fn detects_go_log_sink() {
        let ext = extract_go(
            r#"
package main

import "log"

func main() {
    log.Println("Server starting")
    log.Printf("Listening on port %d", 8080)
}
"#,
        );
        assert_eq!(ext.sinks.len(), 2);
    }

    #[test]
    fn detects_go_slog_sink() {
        let ext = extract_go(
            r#"
package main

import "log/slog"

func main() {
    slog.Info("Server started", "port", 8080)
}
"#,
        );
        assert_eq!(ext.sinks.len(), 1);
    }

    #[test]
    fn no_false_positives_on_regular_calls() {
        let ext = extract_python(
            r#"
result = calculate_total(items)
users = get_users()
data = json.loads(response.text)
"#,
        );
        assert!(ext.sinks.is_empty());
    }

    #[test]
    fn returns_empty_extraction_for_no_sinks() {
        let ext = extract_go(
            r#"
package main

func add(a, b int) int {
    return a + b
}
"#,
        );
        assert!(ext.interfaces.is_empty());
        assert!(ext.dependencies.is_empty());
        assert!(ext.sinks.is_empty());
        assert!(ext.imports.is_empty());
    }

    #[test]
    fn preserves_language_in_extraction() {
        let ext = extract_python("x = 1");
        assert_eq!(ext.language, SupportedLanguage::Python);

        let ext = extract_java("class A {}");
        assert_eq!(ext.language, SupportedLanguage::Java);
    }
}
