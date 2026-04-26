//! Tool invocation normalizers.
//!
//! Normalization converts a tool invocation (name + args + output) into a
//! stable fingerprint that is robust to cosmetic variation: ANSI escapes,
//! temp paths, timestamps, PIDs. Used by the loop detector to decide when
//! two calls are "the same" for practical purposes.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

/// Compiled-once regex helpers (T2.5): moving from per-call `Regex::new(…).unwrap()`
/// removes panics from the hot path and avoids re-parsing the same literal on every
/// normalization call.
///
/// Each pattern is a compile-time literal with no dynamic input; `Regex::new` is
/// therefore guaranteed to succeed. The explicit `expect` documents the invariant.
fn cached(slot: &'static OnceLock<Regex>, pattern: &'static str) -> &'static Regex {
    slot.get_or_init(|| {
        Regex::new(pattern).unwrap_or_else(|e| {
            // This branch is unreachable for a valid compile-time literal;
            // if it ever fires we want the regex pattern in the message.
            panic!("static normalizer regex {pattern:?} failed to compile: {e}")
        })
    })
}

static ANSI_RE: OnceLock<Regex> = OnceLock::new();
static TMP_RE: OnceLock<Regex> = OnceLock::new();
static ISO_TS_RE: OnceLock<Regex> = OnceLock::new();
static UNIX_TS_RE: OnceLock<Regex> = OnceLock::new();
static PID_RE: OnceLock<Regex> = OnceLock::new();
static HEX_HASH_RE: OnceLock<Regex> = OnceLock::new();
static ADDR_RE: OnceLock<Regex> = OnceLock::new();
static UUID_RE: OnceLock<Regex> = OnceLock::new();

/// Trait implemented by all tool-specific normalizers.
pub trait ToolNormalizer: Send + Sync {
    fn normalize_args(&self, args: &Value) -> Value;
    fn normalize_output(&self, output: &str) -> String;
}

/// Default normalizer: hashes the entire JSON args and the full output.
pub struct DefaultNormalizer;

impl ToolNormalizer for DefaultNormalizer {
    fn normalize_args(&self, args: &Value) -> Value {
        args.clone()
    }
    fn normalize_output(&self, output: &str) -> String {
        output.to_string()
    }
}

/// Build a default normalizer.
pub fn default_normalizer() -> Box<dyn ToolNormalizer> {
    Box::new(DefaultNormalizer)
}

/// Normalizer for bash-like commands — strips ANSI, timestamps, pids, temp paths.
pub struct BashNormalizer;

impl BashNormalizer {
    fn scrub(s: &str) -> String {
        // Cached compiled regexes (see `cached` + `OnceLock` statics above).
        // Cannot panic: patterns are compile-time literals that were validated
        // when the normalizer was first exercised in tests.
        let ansi = cached(&ANSI_RE, r"\x1B\[[0-9;]*[A-Za-z]");
        let tmp = cached(&TMP_RE, r"/tmp/[A-Za-z0-9._-]+");
        let iso_ts = cached(
            &ISO_TS_RE,
            r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?",
        );
        let unix_ts = cached(&UNIX_TS_RE, r"\b1[5-9]\d{8}\b");
        let pid = cached(&PID_RE, r"\b(pid|PID)\s*=?\s*\d+\b");
        let hex_hash = cached(&HEX_HASH_RE, r"\b[0-9a-f]{8,}\b");
        let addr = cached(&ADDR_RE, r"\b0x[0-9a-fA-F]+\b");
        let uuid = cached(
            &UUID_RE,
            r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b",
        );

        let s = ansi.replace_all(s, "");
        let s = tmp.replace_all(&s, "/tmp/<TEMP>");
        let s = iso_ts.replace_all(&s, "<TS>");
        let s = unix_ts.replace_all(&s, "<TS>");
        let s = pid.replace_all(&s, "pid=<PID>");
        let s = addr.replace_all(&s, "<ADDR>");
        let s = uuid.replace_all(&s, "<UUID>");
        let s = hex_hash.replace_all(&s, "<HASH>");
        s.into_owned()
    }
}

impl ToolNormalizer for BashNormalizer {
    fn normalize_args(&self, args: &Value) -> Value {
        if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
            serde_json::json!({ "command": Self::scrub(cmd) })
        } else {
            args.clone()
        }
    }

    fn normalize_output(&self, output: &str) -> String {
        Self::scrub(output)
    }
}

/// Normalizer for read_file — keeps only the path.
pub struct ReadFileNormalizer;

impl ToolNormalizer for ReadFileNormalizer {
    fn normalize_args(&self, args: &Value) -> Value {
        let path = args.get("file_path").or_else(|| args.get("path"));
        match path {
            Some(p) => serde_json::json!({ "file_path": p }),
            None => args.clone(),
        }
    }

    fn normalize_output(&self, _output: &str) -> String {
        String::new()
    }
}

/// Normalizer for grep / glob — keeps pattern + path, strips options.
pub struct GrepGlobNormalizer;

impl ToolNormalizer for GrepGlobNormalizer {
    fn normalize_args(&self, args: &Value) -> Value {
        let mut out = serde_json::Map::new();
        for key in ["pattern", "path", "query"] {
            if let Some(v) = args.get(key) {
                out.insert(key.into(), v.clone());
            }
        }
        Value::Object(out)
    }
    fn normalize_output(&self, output: &str) -> String {
        let mut lines: Vec<&str> = output.lines().collect();
        lines.sort_unstable();
        lines.dedup();
        let joined = lines.join("\n");
        let mut h = DefaultHasher::new();
        joined.hash(&mut h);
        format!("{:x}", h.finish())
    }
}

/// Normalizer for web_search / web_fetch — keeps URL/query only.
pub struct WebNormalizer;

impl ToolNormalizer for WebNormalizer {
    fn normalize_args(&self, args: &Value) -> Value {
        let mut out = serde_json::Map::new();
        for key in ["url", "query", "q"] {
            if let Some(v) = args.get(key) {
                out.insert(key.into(), v.clone());
            }
        }
        Value::Object(out)
    }
    fn normalize_output(&self, output: &str) -> String {
        let truncated = output.chars().take(2048).collect::<String>();
        let mut h = DefaultHasher::new();
        truncated.hash(&mut h);
        format!("{:x}", h.finish())
    }
}

/// Normalizer for subagent — keeps role + objective hash.
pub struct SubagentNormalizer;

impl ToolNormalizer for SubagentNormalizer {
    fn normalize_args(&self, args: &Value) -> Value {
        let mut out = serde_json::Map::new();
        if let Some(role) = args.get("role").or_else(|| args.get("subagent_type")) {
            out.insert("role".into(), role.clone());
        }
        if let Some(obj) = args.get("objective").or_else(|| args.get("prompt")) {
            let s = obj.to_string();
            let mut h = DefaultHasher::new();
            s.hash(&mut h);
            out.insert(
                "objective_hash".into(),
                Value::String(format!("{:x}", h.finish())),
            );
        }
        Value::Object(out)
    }
    fn normalize_output(&self, output: &str) -> String {
        let success_flag = if output.contains("success") { "ok" } else { "fail" };
        let mut h = DefaultHasher::new();
        output.hash(&mut h);
        format!("{}|{:x}", success_flag, h.finish())
    }
}

/// Normalizer for edit_file — hashes content.
pub struct EditFileNormalizer;

impl ToolNormalizer for EditFileNormalizer {
    fn normalize_args(&self, args: &Value) -> Value {
        let mut out = serde_json::Map::new();
        if let Some(p) = args.get("file_path") {
            out.insert("file_path".into(), p.clone());
        }
        if let Some(content) = args.get("new_string").and_then(|v| v.as_str()) {
            let mut h = DefaultHasher::new();
            content.hash(&mut h);
            out.insert("new_hash".into(), Value::String(format!("{:x}", h.finish())));
        }
        if let Some(content) = args.get("old_string").and_then(|v| v.as_str()) {
            let mut h = DefaultHasher::new();
            content.hash(&mut h);
            out.insert("old_hash".into(), Value::String(format!("{:x}", h.finish())));
        }
        Value::Object(out)
    }

    fn normalize_output(&self, _output: &str) -> String {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bash_normalizer_strips_ansi() {
        let n = BashNormalizer;
        let v = n.normalize_args(&serde_json::json!({"command": "\x1B[31mhello\x1B[0m"}));
        assert_eq!(v["command"], "hello");
    }

    #[test]
    fn test_bash_normalizer_replaces_temp_paths() {
        let n = BashNormalizer;
        let v = n.normalize_args(&serde_json::json!({"command": "ls /tmp/abc123"}));
        assert_eq!(v["command"], "ls /tmp/<TEMP>");
    }

    #[test]
    fn test_bash_normalizer_replaces_timestamps() {
        let n = BashNormalizer;
        let v = n.normalize_args(&serde_json::json!({"command": "echo 2026-04-22T15:09:59Z"}));
        assert_eq!(v["command"], "echo <TS>");
    }

    #[test]
    fn test_bash_normalizer_replaces_pids() {
        let n = BashNormalizer;
        let v = n.normalize_args(&serde_json::json!({"command": "kill pid=12345"}));
        assert_eq!(v["command"], "kill pid=<PID>");
    }

    #[test]
    fn test_read_file_normalizer_keeps_only_path() {
        let n = ReadFileNormalizer;
        let v = n.normalize_args(&serde_json::json!({"file_path": "/a", "line_start": 10, "line_end": 20}));
        assert_eq!(v.get("line_start"), None);
        assert_eq!(v["file_path"], "/a");
    }

    #[test]
    fn test_edit_file_normalizer_hashes_content() {
        let n = EditFileNormalizer;
        let a = n.normalize_args(&serde_json::json!({"file_path": "/a", "old_string": "x", "new_string": "y"}));
        let b = n.normalize_args(&serde_json::json!({"file_path": "/a", "old_string": "x2", "new_string": "y"}));
        assert_ne!(a["old_hash"], b["old_hash"]);
    }

    #[test]
    fn test_default_normalizer_hashes_full_args() {
        let n = DefaultNormalizer;
        let v = n.normalize_args(&serde_json::json!({"k": "v"}));
        assert_eq!(v, serde_json::json!({"k": "v"}));
    }

    #[test]
    fn test_normalizer_deterministic() {
        let n = BashNormalizer;
        let a = n.normalize_args(&serde_json::json!({"command": "ls /tmp/xyz"}));
        let b = n.normalize_args(&serde_json::json!({"command": "ls /tmp/xyz"}));
        assert_eq!(a, b);
    }

    #[test]
    fn test_grep_glob_normalizer_keeps_pattern_and_path() {
        let n = GrepGlobNormalizer;
        let v = n.normalize_args(&serde_json::json!({
            "pattern": "foo",
            "path": "/src",
            "output_mode": "content",
        }));
        assert_eq!(v["pattern"], "foo");
        assert!(v.get("output_mode").is_none());
    }

    #[test]
    fn test_grep_glob_normalizer_output_order_invariant() {
        let n = GrepGlobNormalizer;
        let a = n.normalize_output("a.rs\nb.rs\nc.rs");
        let b = n.normalize_output("c.rs\nb.rs\na.rs");
        assert_eq!(a, b);
    }

    #[test]
    fn test_web_normalizer_keeps_url_only() {
        let n = WebNormalizer;
        let v = n.normalize_args(&serde_json::json!({
            "url": "https://x",
            "user_agent": "bot",
        }));
        assert_eq!(v["url"], "https://x");
        assert!(v.get("user_agent").is_none());
    }

    #[test]
    fn test_subagent_normalizer_hashes_objective() {
        let n = SubagentNormalizer;
        let a = n.normalize_args(&serde_json::json!({"role": "coder", "objective": "x"}));
        let b = n.normalize_args(&serde_json::json!({"role": "coder", "objective": "y"}));
        assert_ne!(a["objective_hash"], b["objective_hash"]);
    }
}
