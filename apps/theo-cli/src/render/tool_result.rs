//! Pure tool-result rendering functions.
//!
//! Each `render_*` function takes the parsed event data and returns
//! a formatted string honoring the current [`StyleCaps`]. No side
//! effects — `renderer.rs` owns the `eprintln!` side.
//!
//! Keeping these pure makes them trivially unit-testable (no captured
//! stdout required) and enforces the "no raw ANSI outside style.rs"
//! invariant.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use serde_json::Value;

use crate::render::style::{
    self, StyleCaps, accent, bold, code_bg, cross_symbol, dim, error, success, tool_name,
};

/// Truncate a multi-line string to the first line, clipping at `max`
/// characters on a valid UTF-8 boundary. Adds an ellipsis marker when
/// clipped.
pub fn truncate_line(s: &str, max: usize) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.len() > max {
        let mut end = max;
        while end > 0 && !first_line.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &first_line[..end])
    } else {
        first_line.to_string()
    }
}

/// Render a status glyph — check or cross — styled according to caps.
pub fn status_glyph(ok: bool, caps: StyleCaps) -> String {
    if ok {
        success(style::check_symbol(caps), caps).to_string()
    } else {
        error(cross_symbol(caps), caps).to_string()
    }
}

/// Render a duration tail — `" (1.5s)"` when over a second, else empty.
pub fn duration_tail(ms: u64, caps: StyleCaps) -> String {
    if ms > 1000 {
        let secs = ms as f64 / 1000.0;
        format!(" {}", dim(format!("({secs:.1}s)"), caps))
    } else {
        String::new()
    }
}

/// Render the optional `[Role]` prefix for sub-agent events.
pub fn sub_agent_prefix(entity_id: &str, caps: StyleCaps) -> String {
    if entity_id.starts_with('[')
        && let Some(end) = entity_id.find(']')
    {
        let role = &entity_id[1..end];
        let label = format!("[{role}]");
        return format!("{} ", tool_name(label, caps));
    }
    String::new()
}

/// Render a single "Read <path> ✓ (N lines)" line.
pub fn render_read(
    prefix: &str,
    path: &str,
    lines: usize,
    ok: bool,
    duration_ms: u64,
    caps: StyleCaps,
) -> String {
    format!(
        "  {prefix}{bullet} Read {path_bold} {status} {lines_dim}{dur}",
        bullet = accent(style::bullet(caps), caps),
        path_bold = bold(path, caps),
        status = status_glyph(ok, caps),
        lines_dim = dim(format!("({lines} lines)"), caps),
        dur = duration_tail(duration_ms, caps),
    )
}

/// Render a single "Write <path> ✓ (N lines)" line.
pub fn render_write_header(
    prefix: &str,
    path: &str,
    lines: usize,
    ok: bool,
    duration_ms: u64,
    caps: StyleCaps,
) -> String {
    format!(
        "  {prefix}{bullet} Write {path_bold} {status} {lines_dim}{dur}",
        bullet = accent(style::bullet(caps), caps),
        path_bold = bold(path, caps),
        status = status_glyph(ok, caps),
        lines_dim = dim(format!("({lines} lines)"), caps),
        dur = duration_tail(duration_ms, caps),
    )
}

/// Render the body preview lines for a Write (green +, up to `max` lines).
pub fn render_write_preview(content: &str, max: usize, caps: StyleCaps) -> Vec<String> {
    let mut out = Vec::new();
    for line in content.lines().take(max) {
        out.push(format!(
            "    {lead} {body}",
            lead = dim("└", caps),
            body = success(truncate_line(line, 80), caps),
        ));
    }
    let total = content.lines().count();
    if total > max {
        out.push(format!(
            "    {}",
            dim(format!("  … +{} more lines", total - max), caps)
        ));
    }
    out
}

/// Render "Edit <path> ✓" header line.
pub fn render_edit_header(
    prefix: &str,
    path: &str,
    ok: bool,
    duration_ms: u64,
    caps: StyleCaps,
) -> String {
    format!(
        "  {prefix}{bullet} Edit {path_bold} {status}{dur}",
        bullet = accent(style::bullet(caps), caps),
        path_bold = bold(path, caps),
        status = status_glyph(ok, caps),
        dur = duration_tail(duration_ms, caps),
    )
}

/// Render a single diff line ("-" or "+") for Edit preview.
pub fn render_diff_line(sign: char, line: &str, caps: StyleCaps) -> String {
    let truncated = truncate_line(line, 78);
    let body = format!("{sign} {truncated}");
    let styled = match sign {
        '+' => success(body, caps).to_string(),
        '-' => error(body, caps).to_string(),
        _ => body,
    };
    format!("    {lead} {styled}", lead = dim("└", caps))
}

/// Render "Patch <file_list> ✓ (N hunks)" line.
pub fn render_patch(
    prefix: &str,
    file_list: &str,
    hunks: usize,
    ok: bool,
    duration_ms: u64,
    caps: StyleCaps,
) -> String {
    format!(
        "  {prefix}{bullet} Patch {files_bold} {status} {hunks_dim}{dur}",
        bullet = accent(style::bullet(caps), caps),
        files_bold = bold(file_list, caps),
        status = status_glyph(ok, caps),
        hunks_dim = dim(format!("({hunks} hunks)"), caps),
        dur = duration_tail(duration_ms, caps),
    )
}

/// Render "Search files <pattern> ✓ (N files)" for glob.
pub fn render_glob(
    prefix: &str,
    pattern: &str,
    count: usize,
    ok: bool,
    duration_ms: u64,
    caps: StyleCaps,
) -> String {
    format!(
        "  {prefix}{bullet} Search files {pat} {status} {count_dim}{dur}",
        bullet = accent(style::bullet(caps), caps),
        pat = dim(pattern, caps),
        status = status_glyph(ok, caps),
        count_dim = dim(format!("({count} files)"), caps),
        dur = duration_tail(duration_ms, caps),
    )
}

/// Render `Search code "pattern" ✓ (N matches)` for grep.
pub fn render_grep(
    prefix: &str,
    pattern: &str,
    count: usize,
    ok: bool,
    duration_ms: u64,
    caps: StyleCaps,
) -> String {
    let quoted = format!("\"{pattern}\"");
    format!(
        "  {prefix}{bullet} Search code {pat} {status} {count_dim}{dur}",
        bullet = accent(style::bullet(caps), caps),
        pat = dim(quoted, caps),
        status = status_glyph(ok, caps),
        count_dim = dim(format!("({count} matches)"), caps),
        dur = duration_tail(duration_ms, caps),
    )
}

/// Render "Ran <cmd> ✓" bash line.
pub fn render_bash_header(
    prefix: &str,
    cmd: &str,
    ok: bool,
    duration_ms: u64,
    caps: StyleCaps,
) -> String {
    let cmd_short = truncate_line(cmd, 70);
    format!(
        "  {prefix}{bullet} Ran {cmd_dim} {status}{dur}",
        bullet = accent(style::bullet(caps), caps),
        cmd_dim = dim(cmd_short, caps),
        status = status_glyph(ok, caps),
        dur = duration_tail(duration_ms, caps),
    )
}

/// Render bash output preview (first line + overflow marker).
pub fn render_bash_preview(output: &str, caps: StyleCaps) -> Vec<String> {
    let mut out = Vec::new();
    let first = output.lines().next().unwrap_or("");
    let total = output.lines().count();
    if first.is_empty() {
        return out;
    }
    out.push(format!(
        "    {} {}",
        dim("└", caps),
        dim(truncate_line(first, 78), caps)
    ));
    if total > 1 {
        out.push(format!("    {}", dim(format!("  … +{} lines", total - 1), caps)));
    }
    out
}

/// Render a generic tool line: `• <tool> ✓`.
pub fn render_generic(
    prefix: &str,
    tool: &str,
    ok: bool,
    duration_ms: u64,
    caps: StyleCaps,
) -> String {
    format!(
        "  {prefix}{bullet} {tool} {status}{dur}",
        bullet = accent(style::bullet(caps), caps),
        status = status_glyph(ok, caps),
        dur = duration_tail(duration_ms, caps),
    )
}

/// Render a think block (dim italic-style).
pub fn render_think(thought: &str, caps: StyleCaps) -> String {
    format!("\n  {}\n", dim(format!("💭 {thought}"), caps))
}

/// Render a reflect line with confidence tier color.
pub fn render_reflect(
    prefix: &str,
    confidence: u64,
    ok: bool,
    duration_ms: u64,
    caps: StyleCaps,
) -> String {
    let text = format!("(confidence: {confidence}%)");
    let colored = if confidence >= 70 {
        success(text, caps).to_string()
    } else if confidence >= 40 {
        crate::render::style::warn(text, caps).to_string()
    } else {
        error(text, caps).to_string()
    };
    format!(
        "  {prefix}{bullet} Reflect {status}{dur} {colored}",
        bullet = accent(style::bullet(caps), caps),
        status = status_glyph(ok, caps),
        dur = duration_tail(duration_ms, caps),
    )
}

/// Render a memory operation line.
pub fn render_memory(prefix: &str, action: &str, key: &str, ok: bool, caps: StyleCaps) -> String {
    if key.is_empty() {
        format!(
            "  {prefix}{bullet} Memory {action} {status}",
            bullet = accent(style::bullet(caps), caps),
            status = status_glyph(ok, caps),
        )
    } else {
        format!(
            "  {prefix}{bullet} Memory {action}: {key_b} {status}",
            bullet = accent(style::bullet(caps), caps),
            key_b = bold(key, caps),
            status = status_glyph(ok, caps),
        )
    }
}

/// Render a sub-agent spawn banner.
pub fn render_subagent_banner(to: &str, caps: StyleCaps) -> Option<String> {
    if let Some(count) = to.strip_prefix("SubAgentParallel:") {
        Some(format!(
            "\n  {}",
            tool_name(format!("🤖 Spawning {count} sub-agents in parallel"), caps)
        ))
    } else if let Some(role) = to.strip_prefix("SubAgent:") {
        Some(format!(
            "\n  {}",
            tool_name(format!("🤖 Spawning {role} sub-agent"), caps)
        ))
    } else {
        match to {
            "Converged" => Some(format!("\n  {}", success("✅ Converged", caps))),
            "Aborted" => Some(format!("\n  {}", error("⛔ Aborted", caps))),
            _ => None,
        }
    }
}

/// Render a capability-denied error line.
pub fn render_denied(tool: &str, caps: StyleCaps) -> String {
    format!("  {}", error(format!("🚫 {tool} denied"), caps))
}

/// Render a generic error line.
pub fn render_error(msg: &str, caps: StyleCaps) -> String {
    format!("  {}", error(format!("❌ {msg}"), caps))
}

/// Render a budget warning.
pub fn render_budget_warning(violation: &str, caps: StyleCaps) -> String {
    format!(
        "\n  {}",
        crate::render::style::warn(format!("⚠️  {violation}"), caps)
    )
}

/// Render a reasoning delta chunk (dim, no newline).
pub fn render_reasoning_chunk(text: &str, caps: StyleCaps) -> String {
    dim(text, caps).to_string()
}

/// Render a code inline snippet using code_bg style.
pub fn render_inline_code(snippet: &str, caps: StyleCaps) -> String {
    code_bg(snippet, caps).to_string()
}

/// Extract a JSON string field with a default.
pub fn json_str<'a>(v: &'a Value, key: &str, default: &'a str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> StyleCaps {
        StyleCaps::plain()
    }

    // ---- truncate_line ----

    #[test]
    fn test_truncate_short_line_unchanged() {
        assert_eq!(truncate_line("hello", 80), "hello");
    }

    #[test]
    fn test_truncate_long_line_clipped() {
        let s = "a".repeat(100);
        let out = truncate_line(&s, 10);
        assert_eq!(out, format!("{}…", "a".repeat(10)));
    }

    #[test]
    fn test_truncate_first_line_only() {
        let out = truncate_line("first\nsecond\nthird", 80);
        assert_eq!(out, "first");
    }

    #[test]
    fn test_truncate_respects_utf8_boundary() {
        // "á" is 2 bytes — cutting at 1 would be invalid
        let s = "áááááá";
        let out = truncate_line(s, 3);
        // Should clip at a valid boundary (2 bytes = 1 char) with ellipsis
        assert!(out.ends_with('…'));
        assert!(out.chars().count() >= 2); // at least 1 char + …
    }

    // ---- status_glyph ----

    #[test]
    fn test_status_glyph_ok_plain() {
        assert_eq!(status_glyph(true, plain()), "OK");
    }

    #[test]
    fn test_status_glyph_fail_plain() {
        assert_eq!(status_glyph(false, plain()), "X");
    }

    #[test]
    fn test_status_glyph_ok_tty() {
        let s = status_glyph(true, StyleCaps::full());
        assert!(s.contains("✓"));
        assert!(s.contains("\x1b["));
    }

    // ---- duration_tail ----

    #[test]
    fn test_duration_under_1s_is_empty() {
        assert_eq!(duration_tail(500, plain()), "");
    }

    #[test]
    fn test_duration_exactly_1s_is_empty() {
        assert_eq!(duration_tail(1000, plain()), "");
    }

    #[test]
    fn test_duration_over_1s_shows_seconds() {
        assert_eq!(duration_tail(1500, plain()), " (1.5s)");
    }

    #[test]
    fn test_duration_large_ms_rounds_to_one_decimal() {
        assert_eq!(duration_tail(12345, plain()), " (12.3s)");
    }

    // ---- sub_agent_prefix ----

    #[test]
    fn test_sub_agent_prefix_extracts_role() {
        let p = sub_agent_prefix("[Explorer]xyz", plain());
        assert_eq!(p, "[Explorer] ");
    }

    #[test]
    fn test_sub_agent_prefix_empty_for_non_bracketed() {
        assert_eq!(sub_agent_prefix("no-role", plain()), "");
    }

    #[test]
    fn test_sub_agent_prefix_empty_for_unterminated_bracket() {
        assert_eq!(sub_agent_prefix("[unterminated", plain()), "");
    }

    // ---- render_read ----

    #[test]
    fn test_render_read_plain_output() {
        let out = render_read("", "src/main.rs", 42, true, 500, plain());
        assert_eq!(out, "  * Read src/main.rs OK (42 lines)");
    }

    #[test]
    fn test_render_read_with_duration_tail() {
        let out = render_read("", "x.rs", 10, true, 2500, plain());
        assert_eq!(out, "  * Read x.rs OK (10 lines) (2.5s)");
    }

    #[test]
    fn test_render_read_failure() {
        let out = render_read("", "x.rs", 0, false, 0, plain());
        assert!(out.contains("X"));
    }

    #[test]
    fn test_render_read_with_subagent_prefix() {
        let prefix = sub_agent_prefix("[Explorer]", plain());
        let out = render_read(&prefix, "x.rs", 1, true, 0, plain());
        assert!(out.contains("[Explorer]"));
    }

    // ---- render_write_header ----

    #[test]
    fn test_render_write_header_plain() {
        let out = render_write_header("", "out.txt", 5, true, 0, plain());
        assert_eq!(out, "  * Write out.txt OK (5 lines)");
    }

    // ---- render_write_preview ----

    #[test]
    fn test_write_preview_empty_string() {
        let lines = render_write_preview("", 3, plain());
        assert!(lines.is_empty());
    }

    #[test]
    fn test_write_preview_fewer_than_max() {
        let lines = render_write_preview("one\ntwo", 3, plain());
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("one"));
        assert!(lines[1].contains("two"));
    }

    #[test]
    fn test_write_preview_more_than_max_shows_overflow() {
        let lines = render_write_preview("a\nb\nc\nd\ne", 3, plain());
        assert_eq!(lines.len(), 4);
        assert!(lines[3].contains("+2 more lines"));
    }

    // ---- render_edit_header ----

    #[test]
    fn test_edit_header_plain() {
        let out = render_edit_header("", "x.rs", true, 0, plain());
        assert_eq!(out, "  * Edit x.rs OK");
    }

    // ---- render_diff_line ----

    #[test]
    fn test_diff_line_plus() {
        let out = render_diff_line('+', "new code", plain());
        assert_eq!(out, "    └ + new code");
    }

    #[test]
    fn test_diff_line_minus() {
        let out = render_diff_line('-', "old code", plain());
        assert_eq!(out, "    └ - old code");
    }

    #[test]
    fn test_diff_line_truncates() {
        let long = "a".repeat(200);
        let out = render_diff_line('+', &long, plain());
        assert!(out.contains('…'));
    }

    // ---- render_patch ----

    #[test]
    fn test_patch_line_plain() {
        let out = render_patch("", "a.rs, b.rs", 3, true, 0, plain());
        assert_eq!(out, "  * Patch a.rs, b.rs OK (3 hunks)");
    }

    // ---- render_glob ----

    #[test]
    fn test_glob_plain() {
        let out = render_glob("", "*.rs", 7, true, 0, plain());
        assert_eq!(out, "  * Search files *.rs OK (7 files)");
    }

    // ---- render_grep ----

    #[test]
    fn test_grep_plain_includes_quotes() {
        let out = render_grep("", "TODO", 4, true, 0, plain());
        assert_eq!(out, "  * Search code \"TODO\" OK (4 matches)");
    }

    // ---- render_bash_header ----

    #[test]
    fn test_bash_header_plain() {
        let out = render_bash_header("", "cargo test", true, 0, plain());
        assert_eq!(out, "  * Ran cargo test OK");
    }

    #[test]
    fn test_bash_header_truncates_long_command() {
        let long = format!("cargo {}", "x".repeat(200));
        let out = render_bash_header("", &long, true, 0, plain());
        assert!(out.contains('…'));
    }

    // ---- render_bash_preview ----

    #[test]
    fn test_bash_preview_empty_returns_nothing() {
        let out = render_bash_preview("", plain());
        assert!(out.is_empty());
    }

    #[test]
    fn test_bash_preview_single_line_no_overflow() {
        let out = render_bash_preview("done", plain());
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("done"));
    }

    #[test]
    fn test_bash_preview_multiline_shows_overflow() {
        let out = render_bash_preview("first\nsecond\nthird", plain());
        assert_eq!(out.len(), 2);
        assert!(out[1].contains("+2 lines"));
    }

    // ---- render_generic ----

    #[test]
    fn test_generic_plain() {
        let out = render_generic("", "custom_tool", true, 0, plain());
        assert_eq!(out, "  * custom_tool OK");
    }

    // ---- render_think ----

    #[test]
    fn test_think_plain() {
        let out = render_think("pondering", plain());
        assert!(out.contains("💭 pondering"));
    }

    // ---- render_reflect ----

    #[test]
    fn test_reflect_high_confidence_plain() {
        let out = render_reflect("", 85, true, 0, plain());
        assert!(out.contains("(confidence: 85%)"));
    }

    #[test]
    fn test_reflect_low_confidence_plain() {
        let out = render_reflect("", 20, true, 0, plain());
        assert!(out.contains("(confidence: 20%)"));
    }

    // ---- render_memory ----

    #[test]
    fn test_memory_no_key() {
        let out = render_memory("", "list", "", true, plain());
        assert_eq!(out, "  * Memory list OK");
    }

    #[test]
    fn test_memory_with_key() {
        let out = render_memory("", "save", "last_run", true, plain());
        assert_eq!(out, "  * Memory save: last_run OK");
    }

    // ---- render_subagent_banner ----

    #[test]
    fn test_subagent_parallel_banner() {
        let b = render_subagent_banner("SubAgentParallel:3", plain()).unwrap();
        assert!(b.contains("Spawning 3 sub-agents in parallel"));
    }

    #[test]
    fn test_subagent_single_banner() {
        let b = render_subagent_banner("SubAgent:Explorer", plain()).unwrap();
        assert!(b.contains("Spawning Explorer sub-agent"));
    }

    #[test]
    fn test_subagent_converged_banner() {
        let b = render_subagent_banner("Converged", plain()).unwrap();
        assert!(b.contains("Converged"));
    }

    #[test]
    fn test_subagent_aborted_banner() {
        let b = render_subagent_banner("Aborted", plain()).unwrap();
        assert!(b.contains("Aborted"));
    }

    #[test]
    fn test_subagent_unknown_state_returns_none() {
        assert!(render_subagent_banner("Random", plain()).is_none());
    }

    // ---- errors / budget ----

    #[test]
    fn test_render_denied_plain() {
        let out = render_denied("bash", plain());
        assert_eq!(out, "  🚫 bash denied");
    }

    #[test]
    fn test_render_error_plain() {
        let out = render_error("oops", plain());
        assert_eq!(out, "  ❌ oops");
    }

    #[test]
    fn test_render_budget_warning_plain() {
        let out = render_budget_warning("tokens exceeded", plain());
        assert!(out.contains("⚠️"));
        assert!(out.contains("tokens exceeded"));
    }

    // ---- reasoning / code ----

    #[test]
    fn test_render_reasoning_chunk_plain() {
        assert_eq!(render_reasoning_chunk("thinking", plain()), "thinking");
    }

    #[test]
    fn test_render_inline_code_plain() {
        assert_eq!(render_inline_code("let x = 1", plain()), "let x = 1");
    }

    // ---- json_str ----

    #[test]
    fn test_json_str_returns_value() {
        let v = serde_json::json!({"name": "theo"});
        assert_eq!(json_str(&v, "name", "?"), "theo");
    }

    #[test]
    fn test_json_str_returns_default_on_missing() {
        let v = serde_json::json!({});
        assert_eq!(json_str(&v, "name", "fallback"), "fallback");
    }

    #[test]
    fn test_json_str_returns_default_on_wrong_type() {
        let v = serde_json::json!({"name": 42});
        assert_eq!(json_str(&v, "name", "?"), "?");
    }
}
