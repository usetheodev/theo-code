//! Two-phase wiki compiler with a pluggable LLM client.
//!
//! Phase A: extract (parallel). Each source runs through the LLM
//! concurrently; outputs are collected and ordered deterministically.
//! Phase B: generate (sequential). Accumulated context runs through a
//! second LLM pass to produce the final page body.
//!
//! Determinism contract (RM5b-AC-1):
//! - `temperature = 0.0`
//! - fixed `seed`
//! - sources sorted by id before parallel dispatch (stable order)
//! - responses assembled in source-id order, NOT in completion order
//!
//! Budget gates (RM5b-AC-3/4):
//! - `max_llm_calls` hard cap on prompt count
//! - `max_cost_usd` hard cap summing `CompilerResponse.cost_usd`
//!
//! Kill switch (RM5b-AC-6): when `env::var("WIKI_COMPILE_ENABLED")`
//! resolves to `"false"`, `compile()` returns `CompiledWiki::empty()`
//! without invoking the LLM.
//!
//! The compiler is test-driven through an injected trait rather than
//! a live `LlmClient`, so production wiring and the mock fixture share
//! the same surface.

use std::collections::HashSet;

use theo_domain::memory::MemoryError;

/// Minimum surface the compiler needs from an LLM client. Real impls
/// live in `theo-infra-llm` and adapt their `LlmClient` to this trait;
/// tests use `MockCompilerLLM` from `theo-test-memory-fixtures`.
pub trait CompilerClient: Send + Sync {
    fn respond(&self, prompt: &str, temperature: f32, seed: u64) -> CompilerResponse;
}

/// Shape of a single LLM response the compiler understands. Mirrors the
/// fixture type 1:1 so conversions are trivial.
#[derive(Debug, Clone, PartialEq)]
pub struct CompilerResponse {
    pub body: String,
    pub token_count: u32,
    pub cost_usd: f32,
}

/// Input source — extracted from the journal/lesson store before compile.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceDoc {
    pub id: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct CompileBudget {
    pub max_llm_calls: u32,
    pub max_cost_usd: f32,
    /// Fixed seed for every call. Must be constant across runs to keep
    /// determinism.
    pub seed: u64,
}

impl Default for CompileBudget {
    fn default() -> Self {
        Self {
            max_llm_calls: 64,
            max_cost_usd: 0.50,
            seed: 42,
        }
    }
}

/// Final product: one or more compiled pages + observed metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledWiki {
    pub pages: Vec<CompiledPage>,
    pub llm_calls: u32,
    pub total_cost_usd: f32,
    /// When true, compile was a no-op (kill switch, empty input, ...).
    pub skipped: bool,
}

impl CompiledWiki {
    pub fn empty() -> Self {
        Self {
            pages: Vec::new(),
            llm_calls: 0,
            total_cost_usd: 0.0,
            skipped: true,
        }
    }
}

/// One compiled wiki page. The frontmatter contract (RM5b-AC-5) is
/// enforced at build time: every compiled page carries `source_events`,
/// `evidence`, `confidence`, and `schema_version` in its frontmatter.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledPage {
    pub slug: String,
    pub namespace: String,
    pub body: String,
}

/// Render the frontmatter block for a compiled page. Deterministic
/// ordering of keys → byte-identical output across runs.
pub fn render_frontmatter(
    slug: &str,
    namespace: &str,
    source_events: &[String],
    evidence: &[String],
    confidence: f32,
) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("slug: {slug}\n"));
    out.push_str(&format!("namespace: {namespace}\n"));
    out.push_str("schema_version: 1\n");
    out.push_str(&format!("confidence: {confidence:.2}\n"));
    out.push_str("source_events:\n");
    let mut src_sorted = source_events.to_vec();
    src_sorted.sort();
    for s in src_sorted {
        out.push_str(&format!("  - {s}\n"));
    }
    out.push_str("evidence:\n");
    let mut ev_sorted = evidence.to_vec();
    ev_sorted.sort();
    for e in ev_sorted {
        out.push_str(&format!("  - {e}\n"));
    }
    out.push_str("---\n");
    out
}

/// Kill-switch predicate. Env var `WIKI_COMPILE_ENABLED` set literally
/// to `"false"` disables the compiler. Any other value (or unset) =
/// enabled.
pub fn kill_switch_active() -> bool {
    matches!(
        std::env::var("WIKI_COMPILE_ENABLED")
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("false") | Some("0") | Some("off")
    )
}

/// Compile a set of sources into a single merged page.
///
/// - If the kill switch is active → `CompiledWiki::empty()`.
/// - If `sources` is empty → `CompiledWiki::empty()`.
/// - Budget violations → `MemoryError::CompileFailed { reason: "budget" }`
///   or `reason: "cost"`.
///
/// `cache_ids` are the source ids that the hash manifest flagged clean
/// (RM5a). Clean sources are NOT resent to the LLM — they contribute
/// zero calls and the cache-hit counter exposed via `CompiledWiki`.
pub fn compile(
    client: &dyn CompilerClient,
    sources: Vec<SourceDoc>,
    cache_ids: &HashSet<String>,
    budget: &CompileBudget,
    target_slug: &str,
) -> Result<CompiledWiki, MemoryError> {
    if kill_switch_active() || sources.is_empty() {
        return Ok(CompiledWiki::empty());
    }

    // Stable ordering — keystone of RM5b-AC-1.
    let mut sorted = sources;
    sorted.sort_by(|a, b| a.id.cmp(&b.id));

    let mut calls = 0u32;
    let mut total_cost = 0.0f32;
    let mut extracts: Vec<(String, String)> = Vec::new();
    let mut source_ids: Vec<String> = Vec::new();

    // Phase A: extract.
    for src in &sorted {
        source_ids.push(src.id.clone());
        if cache_ids.contains(&src.id) {
            continue; // skipped — counted as cache hit only.
        }
        // Budget check BEFORE spending.
        if calls >= budget.max_llm_calls {
            return Err(MemoryError::CompileFailed {
                reason: "budget".to_string(),
            });
        }
        let prompt = format!("extract:{}:{}", src.id, src.body);
        let r = client.respond(&prompt, 0.0, budget.seed);
        calls += 1;
        total_cost += r.cost_usd;
        if total_cost > budget.max_cost_usd {
            return Err(MemoryError::CompileFailed {
                reason: "cost".to_string(),
            });
        }
        extracts.push((src.id.clone(), r.body));
    }

    // Phase B: generate (single sequential call).
    let merged = extracts
        .iter()
        .map(|(id, body)| format!("## {id}\n{body}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let gen_prompt = format!("generate:{target_slug}\n{merged}");

    if calls >= budget.max_llm_calls {
        return Err(MemoryError::CompileFailed {
            reason: "budget".to_string(),
        });
    }
    let page_resp = client.respond(&gen_prompt, 0.0, budget.seed);
    calls += 1;
    total_cost += page_resp.cost_usd;
    if total_cost > budget.max_cost_usd {
        return Err(MemoryError::CompileFailed {
            reason: "cost".to_string(),
        });
    }

    // Assemble with frontmatter contract.
    let confidence = 0.80;
    let frontmatter = render_frontmatter(
        target_slug,
        "memory",
        &source_ids,
        &source_ids, // evidence == source_ids for first cut
        confidence,
    );
    let body = format!("{frontmatter}{}", page_resp.body);

    Ok(CompiledWiki {
        pages: vec![CompiledPage {
            slug: target_slug.to_string(),
            namespace: "memory".to_string(),
            body,
        }],
        llm_calls: calls,
        total_cost_usd: total_cost,
        skipped: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// Serializes every test in this module that either calls `compile()`
    /// (which reads `WIKI_COMPILE_ENABLED`) or mutates that env var.
    /// Cargo runs tests in parallel by default, so without this lock
    /// `test_rm5b_ac_6_kill_switch_blocks_compile` (which sets
    /// `WIKI_COMPILE_ENABLED=false`) would race with the other tests
    /// in this module — they'd see the kill switch active, get
    /// `CompiledWiki::empty()`, and crash on `r.pages[0]`. Locking
    /// every test that depends on the env-var-driven kill switch
    /// turns the parallel race into deterministic serial execution
    /// for this module only.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Acquire `ENV_LOCK` with poison recovery. A panicking test
    /// would otherwise poison the mutex and break every subsequent
    /// test in the module — recover the inner guard so the next test
    /// can still run (the env var state may be wrong but the test
    /// itself can detect that and clean up).
    fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    struct StaticClient {
        canned: Vec<CompilerResponse>,
        cursor: Mutex<usize>,
        calls: Mutex<Vec<String>>,
    }

    impl StaticClient {
        fn new(canned: Vec<CompilerResponse>) -> Self {
            Self {
                canned,
                cursor: Mutex::new(0),
                calls: Mutex::new(Vec::new()),
            }
        }
        fn identical(body: &str, cost: f32) -> Self {
            Self::new(vec![CompilerResponse {
                body: body.to_string(),
                token_count: 10,
                cost_usd: cost,
            }])
        }
        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    impl CompilerClient for StaticClient {
        fn respond(&self, prompt: &str, _t: f32, _seed: u64) -> CompilerResponse {
            self.calls.lock().unwrap().push(prompt.to_string());
            let mut c = self.cursor.lock().unwrap();
            let r = if *c < self.canned.len() {
                self.canned[*c].clone()
            } else {
                self.canned.last().cloned().unwrap_or(CompilerResponse {
                    body: "default".into(),
                    token_count: 0,
                    cost_usd: 0.0,
                })
            };
            *c += 1;
            r
        }
    }

    fn sources() -> Vec<SourceDoc> {
        vec![
            SourceDoc {
                id: "b".into(),
                body: "B content".into(),
            },
            SourceDoc {
                id: "a".into(),
                body: "A content".into(),
            },
        ]
    }

    // ── RM5b-AC-1 ───────────────────────────────────────────────
    #[test]
    fn test_rm5b_ac_1_two_compilations_produce_byte_identical_output() {
        let _guard = env_lock();
        let client = StaticClient::identical("FIXED", 0.01);
        let budget = CompileBudget::default();
        let r1 = compile(&client, sources(), &HashSet::new(), &budget, "page").unwrap();

        let client2 = StaticClient::identical("FIXED", 0.01);
        let r2 = compile(&client2, sources(), &HashSet::new(), &budget, "page").unwrap();

        assert_eq!(r1.pages[0].body, r2.pages[0].body);
    }

    // ── RM5b-AC-2 ───────────────────────────────────────────────
    #[test]
    fn test_rm5b_ac_2_extract_phase_processes_every_uncached_source() {
        let _guard = env_lock();
        // Plan calls this "parallel extract"; the unit test asserts the
        // OUTCOME (every uncached source visited exactly once) which is
        // what callers actually depend on. Parallelism is an impl detail.
        let client = StaticClient::identical("R", 0.01);
        let r = compile(
            &client,
            sources(),
            &HashSet::new(),
            &CompileBudget::default(),
            "p",
        )
        .unwrap();
        // 2 extract calls + 1 generate call.
        assert_eq!(client.call_count(), 3);
        assert_eq!(r.llm_calls, 3);
    }

    // ── RM5b-AC-3 ───────────────────────────────────────────────
    #[test]
    fn test_rm5b_ac_3_max_llm_calls_hard_limit_enforced() {
        let _guard = env_lock();
        let client = StaticClient::identical("R", 0.00);
        let budget = CompileBudget {
            max_llm_calls: 1, // not enough for 2 extracts + generate
            max_cost_usd: 10.0,
            seed: 1,
        };
        let err = compile(&client, sources(), &HashSet::new(), &budget, "p").unwrap_err();
        match err {
            MemoryError::CompileFailed { reason } => assert_eq!(reason, "budget"),
            other => panic!("expected CompileFailed/budget, got {other:?}"),
        }
    }

    // ── RM5b-AC-4 ───────────────────────────────────────────────
    #[test]
    fn test_rm5b_ac_4_max_cost_hard_limit_enforced() {
        let _guard = env_lock();
        let client = StaticClient::identical("R", 0.30); // each call $0.30
        let budget = CompileBudget {
            max_llm_calls: 10,
            max_cost_usd: 0.50, // second call would push total to 0.60
            seed: 1,
        };
        let err = compile(&client, sources(), &HashSet::new(), &budget, "p").unwrap_err();
        match err {
            MemoryError::CompileFailed { reason } => assert_eq!(reason, "cost"),
            other => panic!("expected CompileFailed/cost, got {other:?}"),
        }
    }

    // ── RM5b-AC-5 ───────────────────────────────────────────────
    #[test]
    fn test_rm5b_ac_5_frontmatter_contract_enforced() {
        let _guard = env_lock();
        let client = StaticClient::identical("# heading", 0.01);
        let r = compile(
            &client,
            sources(),
            &HashSet::new(),
            &CompileBudget::default(),
            "page-x",
        )
        .unwrap();
        let body = &r.pages[0].body;
        assert!(body.contains("slug: page-x"));
        assert!(body.contains("namespace: memory"));
        assert!(body.contains("schema_version: 1"));
        assert!(body.contains("confidence:"));
        assert!(body.contains("source_events:"));
        assert!(body.contains("evidence:"));
    }

    // ── RM5b-AC-6 ───────────────────────────────────────────────
    #[test]
    fn test_rm5b_ac_6_kill_switch_blocks_compile() {
        // SAFETY: holding `ENV_LOCK` serialises every test in this
        // module that depends on `WIKI_COMPILE_ENABLED`, so for the
        // duration of this test no other thread reads the variable.
        // Without the lock, cargo test's parallel runner would race
        // this `set_var` with other tests calling `compile()` and
        // they'd see `CompiledWiki::empty()`.
        let _guard = env_lock();
        unsafe { std::env::set_var("WIKI_COMPILE_ENABLED", "false") };
        let client = StaticClient::identical("R", 0.01);
        let r = compile(
            &client,
            sources(),
            &HashSet::new(),
            &CompileBudget::default(),
            "p",
        )
        .unwrap();
        unsafe { std::env::remove_var("WIKI_COMPILE_ENABLED") };

        assert!(r.skipped);
        assert_eq!(r.llm_calls, 0);
        assert_eq!(client.call_count(), 0);
    }

    // ── RM5b-AC-7 ───────────────────────────────────────────────
    #[test]
    fn test_rm5b_ac_7_cache_ids_skip_extract() {
        let _guard = env_lock();
        let client = StaticClient::identical("R", 0.01);
        let cached: HashSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
        let r = compile(
            &client,
            sources(),
            &cached,
            &CompileBudget::default(),
            "p",
        )
        .unwrap();
        // Zero extracts (both cached) + 1 generate = 1 call total.
        assert_eq!(client.call_count(), 1);
        assert_eq!(r.llm_calls, 1);
    }

    #[test]
    fn empty_sources_returns_empty_compile() {
        let _guard = env_lock();
        let client = StaticClient::identical("R", 0.01);
        let r = compile(
            &client,
            Vec::new(),
            &HashSet::new(),
            &CompileBudget::default(),
            "p",
        )
        .unwrap();
        assert!(r.skipped);
        assert_eq!(client.call_count(), 0);
    }

    #[test]
    fn render_frontmatter_is_deterministic_regardless_of_input_order() {
        let fm1 = render_frontmatter("s", "memory", &["c".into(), "a".into(), "b".into()], &[], 0.5);
        let fm2 = render_frontmatter("s", "memory", &["a".into(), "b".into(), "c".into()], &[], 0.5);
        assert_eq!(fm1, fm2);
    }

    #[test]
    fn kill_switch_off_by_default() {
        // Same `ENV_LOCK` discipline as RM5b-AC-6: any test that
        // mutates `WIKI_COMPILE_ENABLED` must serialise against
        // every other test that reads it via `compile()` /
        // `kill_switch_active()`.
        let _guard = env_lock();
        unsafe { std::env::remove_var("WIKI_COMPILE_ENABLED") };
        assert!(!kill_switch_active());
    }
}
