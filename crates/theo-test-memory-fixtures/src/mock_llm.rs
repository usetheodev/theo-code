//! `MockCompilerLLM` — deterministic fake for the RM5b wiki compiler.
//!
//! Real wiki compilation routes through an LLM at temperature 0 + a fixed
//! seed. For tests we want byte-identical output AND the ability to
//! assert which prompts the compiler sent. `MockCompilerLLM` canned
//! responses keyed by a stable prompt hash satisfy both.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// A single response the mock will return from `respond`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompilerResponse {
    /// Fully-formed page body (frontmatter + markdown) the compiler would
    /// have written to disk.
    pub body: String,
    /// Approximate token count used to drive the budget gate.
    pub token_count: u32,
    /// Cost in USD attributed to this response, used to drive the dollar
    /// gate.
    pub cost_usd: f32,
}

impl CompilerResponse {
    pub fn simple(body: impl Into<String>) -> Self {
        Self {
            body: body.into(),
            token_count: 64,
            cost_usd: 0.001,
        }
    }
}

/// One recorded prompt plus metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompilerCall {
    pub prompt: String,
    pub temperature: f32,
    pub seed: u64,
}

/// Deterministic, introspectable LLM fake for compiler tests.
pub struct MockCompilerLLM {
    /// FIFO queue of responses. `respond()` pops the head.
    queue: Mutex<VecDeque<CompilerResponse>>,
    /// Prompt → canned response. Checked BEFORE the queue to allow
    /// per-prompt fixtures when ordering is not important.
    by_prompt: Mutex<HashMap<String, CompilerResponse>>,
    /// Every prompt seen, in order.
    calls: Mutex<Vec<CompilerCall>>,
    /// Default response used when both `queue` and `by_prompt` are empty.
    default: CompilerResponse,
}

impl MockCompilerLLM {
    /// Construct with a single default. Further fixtures can be added via
    /// `push_response` or `with_prompt_response`.
    pub fn with_default(default: CompilerResponse) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            by_prompt: Mutex::new(HashMap::new()),
            calls: Mutex::new(Vec::new()),
            default,
        }
    }

    /// Convenience ctor matching the plan's `with_response(json)` form.
    pub fn with_response(body: impl Into<String>) -> Self {
        Self::with_default(CompilerResponse::simple(body))
    }

    pub fn push_response(&self, r: CompilerResponse) {
        self.queue.lock().unwrap().push_back(r);
    }

    pub fn with_prompt_response(&self, prompt: impl Into<String>, r: CompilerResponse) {
        self.by_prompt.lock().unwrap().insert(prompt.into(), r);
    }

    /// Simulate the round-trip. Records the call, returns a canned response.
    pub fn respond(&self, prompt: &str, temperature: f32, seed: u64) -> CompilerResponse {
        self.calls.lock().unwrap().push(CompilerCall {
            prompt: prompt.to_string(),
            temperature,
            seed,
        });
        if let Some(r) = self.by_prompt.lock().unwrap().get(prompt) {
            return r.clone();
        }
        if let Some(r) = self.queue.lock().unwrap().pop_front() {
            return r;
        }
        self.default.clone()
    }

    pub fn calls(&self) -> Vec<CompilerCall> {
        self.calls.lock().unwrap().clone()
    }

    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_response_returned_when_nothing_queued() {
        let m = MockCompilerLLM::with_response("hello");
        let r = m.respond("prompt-1", 0.0, 42);
        assert_eq!(r.body, "hello");
    }

    #[test]
    fn queued_responses_consumed_in_order() {
        let m = MockCompilerLLM::with_response("default");
        m.push_response(CompilerResponse::simple("first"));
        m.push_response(CompilerResponse::simple("second"));

        assert_eq!(m.respond("p", 0.0, 1).body, "first");
        assert_eq!(m.respond("p", 0.0, 1).body, "second");
        assert_eq!(m.respond("p", 0.0, 1).body, "default");
    }

    #[test]
    fn prompt_fixture_wins_over_queue() {
        let m = MockCompilerLLM::with_response("default");
        m.push_response(CompilerResponse::simple("queued"));
        m.with_prompt_response("special", CompilerResponse::simple("pinned"));

        assert_eq!(m.respond("special", 0.0, 1).body, "pinned");
        assert_eq!(m.respond("other", 0.0, 1).body, "queued");
    }

    #[test]
    fn calls_record_prompt_temperature_and_seed() {
        let m = MockCompilerLLM::with_response("_");
        m.respond("hello", 0.0, 99);
        m.respond("world", 0.0, 99);
        let calls = m.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].prompt, "hello");
        assert_eq!(calls[0].seed, 99);
        assert_eq!(calls[1].prompt, "world");
    }

    #[test]
    fn determinism_same_inputs_same_outputs() {
        let a = MockCompilerLLM::with_response("x");
        let b = MockCompilerLLM::with_response("x");
        assert_eq!(a.respond("p", 0.0, 7).body, b.respond("p", 0.0, 7).body);
    }
}
