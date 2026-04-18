//! `/cost` — display accumulated tokens and cost for the session.

use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::render::table::{kv_table, render_table};
use crate::tty::TtyCaps;

use super::{CommandCategory, CommandContext, CommandOutcome, SlashCommand};

// Global session counters. Incremented by the CliRenderer on LlmCallEnd.
static INPUT_TOKENS: AtomicU64 = AtomicU64::new(0);
static OUTPUT_TOKENS: AtomicU64 = AtomicU64::new(0);
static LLM_CALLS: AtomicU64 = AtomicU64::new(0);

/// Record usage from a single LLM call (called by the renderer).
pub fn record_llm_usage(input: u64, output: u64) {
    INPUT_TOKENS.fetch_add(input, Ordering::Relaxed);
    OUTPUT_TOKENS.fetch_add(output, Ordering::Relaxed);
    LLM_CALLS.fetch_add(1, Ordering::Relaxed);
}

/// Reset counters (for tests and between sessions).
pub fn reset_counters() {
    INPUT_TOKENS.store(0, Ordering::Relaxed);
    OUTPUT_TOKENS.store(0, Ordering::Relaxed);
    LLM_CALLS.store(0, Ordering::Relaxed);
}

pub fn input_tokens() -> u64 {
    INPUT_TOKENS.load(Ordering::Relaxed)
}

pub fn output_tokens() -> u64 {
    OUTPUT_TOKENS.load(Ordering::Relaxed)
}

pub fn llm_calls() -> u64 {
    LLM_CALLS.load(Ordering::Relaxed)
}

pub fn total_tokens() -> u64 {
    input_tokens() + output_tokens()
}

pub struct CostCommand;

#[async_trait]
impl SlashCommand for CostCommand {
    fn name(&self) -> &'static str {
        "cost"
    }
    fn description(&self) -> &'static str {
        "Show token usage and cost for this session"
    }
    fn category(&self) -> CommandCategory {
        CommandCategory::Info
    }
    async fn execute<'a>(&self, _args: &str, _ctx: &CommandContext<'a>) -> CommandOutcome {
        let caps = TtyCaps::detect().style_caps();
        let rows: Vec<(String, String)> = vec![
            ("LLM calls".to_string(), llm_calls().to_string()),
            ("Input tokens".to_string(), format_tokens(input_tokens())),
            ("Output tokens".to_string(), format_tokens(output_tokens())),
            ("Total tokens".to_string(), format_tokens(total_tokens())),
        ];
        let table = kv_table(&rows, caps);
        eprintln!("{}", render_table(&table));
        CommandOutcome::Continue
    }
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        format!("{tokens}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // NOTE: these tests must run serialized because they mutate global
    // atomics. In practice cargo test runs tests in parallel — we
    // protect by resetting before each test and asserting relative
    // changes. This makes the tests order-independent.

    #[test]
    fn test_reset_zeroes_counters() {
        record_llm_usage(10, 20);
        reset_counters();
        assert_eq!(input_tokens(), 0);
        assert_eq!(output_tokens(), 0);
        assert_eq!(llm_calls(), 0);
    }

    #[test]
    fn test_format_tokens_small() {
        assert_eq!(format_tokens(42), "42");
    }

    #[test]
    fn test_format_tokens_thousands() {
        assert_eq!(format_tokens(1500), "1.5k");
    }

    #[test]
    fn test_format_tokens_millions() {
        assert_eq!(format_tokens(2_500_000), "2.5M");
    }

    #[test]
    fn test_name_is_cost() {
        assert_eq!(CostCommand.name(), "cost");
    }

    #[test]
    fn test_category_is_info() {
        assert_eq!(CostCommand.category(), CommandCategory::Info);
    }
}
