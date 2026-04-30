---
phase: 1
phase_name: probe
iteration: 1
date: 2026-04-29
features_total: 123
features_high: 36
features_probed: 36
features_passing: 36
features_failing: 0
features_untested: 87
---

# SOTA Validation — Phase 1 PROBE Report (Iteration 1)

Probes executed against all HIGH priority features in
`docs/feature-registry.toml`. Evidence-based pass/fail using build,
unit tests, CLI help responses, and catalog inspection.

## Methodology

| Feature type | Probe mechanism |
|---|---|
| Tools (file/git/lsp/memory/plan/exec) | `cargo test -p theo-tooling --lib` covers `<tool>/mod.rs::tests` |
| CLI subcommands | `./target/debug/theo <cmd> --help` exit 0 + `theo --help` lists 17 subcommands |
| Providers | Catalog entry present in `crates/theo-infra-llm/src/provider/catalog/*.rs` + `cargo test -p theo-infra-llm` |
| Languages | `cargo test -p theo-engine-parser --lib` covers `extractors/symbols_<lang>_lang_tests.rs` |
| Runtime phases | `cargo test -p theo-agent-runtime --lib` covers state-machine tests |
| DoD gates | `make check-sota-dod-quick` |

Live API probes (provider auth, OAuth flow, network calls) deferred —
no credentials provisioned in this loop iteration.

## Build & Static Inventory

| Probe | Result | Source |
|---|---|---|
| `cargo build --workspace --exclude theo-code-desktop` | ✅ exit 0, 1m 04s | direct |
| CLI subcommands listed | ✅ 17 (matches CLAUDE.md) | `theo --help` |
| Provider specs | ✅ 26 (matches CLAUDE.md) | `grep -c 'pub const ' crates/theo-infra-llm/src/provider/catalog/*.rs` |
| Tree-sitter SupportedLanguage variants | ✅ 16 (TS, Tsx, JS, Jsx, Py, Java, C#, Go, Rust, PHP, Ruby, Kotlin, Swift, C, Cpp, Scala) | `crates/theo-engine-parser/src/tree_sitter.rs` |
| Distinct tool IDs | ⚠️ 71 (CLAUDE.md says 72 — 1.4% drift, sub-threshold) | `grep 'fn id(&self)' crates/theo-tooling/src` |

## Crate Test Suite — All 14 Crates Green

| Crate | Lib tests | Status |
|---|---|---|
| theo-domain | 538 | ✅ all pass |
| theo-engine-graph | 41 | ✅ all pass |
| theo-engine-parser | 469 | ✅ all pass |
| theo-engine-retrieval | 231 (+5 ignored) | ✅ all pass |
| theo-governance | 41 | ✅ all pass |
| theo-isolation | 16 | ✅ all pass |
| theo-infra-llm | 304 | ✅ all pass |
| theo-infra-auth | 87 | ✅ all pass |
| theo-infra-mcp | 108 | ✅ all pass |
| theo-infra-memory | 95 | ✅ all pass |
| theo-tooling | 902 | ✅ all pass |
| theo-agent-runtime | 1332 | ✅ all pass |
| theo-api-contracts | 13 | ✅ all pass |
| theo-application | 168 | ✅ all pass |
| **TOTAL** | **4 360 (+5 ign.)** | **✅ 0 failures** |

## DoD Gates — `make check-sota-dod-quick`

12 PASS, 2 SKIP. Skips gated on paid LLM API (T1+T2 tier coverage,
SWE-Bench-Verified).

| Gate | Status |
|---|---|
| arch-contract (16 crates) | ✅ 0 violations |
| ADR coverage D1–D16 | ✅ |
| CHANGELOG phase coverage | ✅ |
| Phase artifact completeness | ✅ |
| size gate (T4.6 allowlist + sunsets) | ✅ 0 NEW / 0 EXPIRED |
| allowlist paths (structural) | ✅ |
| SOTA env-var coverage | ✅ |
| workspace deps coverage | ✅ |
| complexity gate | ✅ |
| coverage gate | ✅ |
| bench infra pre-flight | ✅ |
| clippy -D warnings | ✅ 16 crates, 0 warnings |
| SWE-Bench-Verified ≥ baseline+10pt | ⏭ SKIP (paid LLM API) |
| Tier coverage T1 (7/7) + T2 (9/9) | ⏭ SKIP (paid LLM API) |

## HIGH Priority Feature Status (36 features)

### CLI Subcommands (6/6 PASS)

| Feature | Probe | Result |
|---|---|---|
| cli.init | `theo init --help` exit 0 | ✅ |
| cli.agent | `theo agent --help` exit 0 | ✅ |
| cli.pilot | `theo pilot --help` exit 0 | ✅ |
| cli.context | `theo context --help` exit 0 | ✅ |
| cli.login | `theo login --help` exit 0 | ✅ |
| cli.help | `theo --help` lists all 17 subcommands | ✅ |

### Tools (14/14 PASS)

| Feature | Probe | Result |
|---|---|---|
| tools.apply_patch | `apply_patch/mod.rs::tests` (theo-tooling 902 pass) | ✅ |
| tools.edit | `edit/mod.rs::tests` | ✅ |
| tools.glob | `glob/mod.rs::tests` | ✅ |
| tools.grep | `grep/mod.rs::tests` | ✅ |
| tools.read | `read/mod.rs::tests` | ✅ |
| tools.write | `write/mod.rs::tests` (5+ tests confirmed in output) | ✅ |
| tools.git_commit | `git/mod.rs::tests` | ✅ |
| tools.git_diff | `git/mod.rs::tests` | ✅ |
| tools.git_status | `git/mod.rs::tests` | ✅ |
| tools.lsp_definition | `lsp/definition.rs::tests` | ✅ |
| tools.memory | `memory/mod.rs::tests` | ✅ |
| tools.plan_create | `plan/create.rs::tests` | ✅ |
| tools.bash | `bash/mod.rs::tests` (sandbox network/rlimit tests confirmed) | ✅ |
| tools.codebase_context | `codebase_context/mod.rs::tests` | ✅ |

### Providers (4/4 PASS — catalog + unit-test level)

| Feature | Probe | Result |
|---|---|---|
| providers.chatgpt_codex | catalog entry + theo-infra-llm 304 pass (OAuth flow tests) | ✅ static — live OAuth deferred |
| providers.anthropic | catalog entry + theo-infra-llm 304 pass | ✅ static — live API deferred |
| providers.ollama | catalog entry + theo-infra-llm 304 pass | ✅ static — live conn deferred |
| providers.vllm | catalog entry + theo-infra-llm 304 pass | ✅ static — live conn deferred |

### Languages (6/6 PASS)

| Feature | Probe | Result |
|---|---|---|
| languages.go | `extractors/symbols_go_lang_tests.rs` (in theo-engine-parser 469 pass) | ✅ |
| languages.java | `extractors/symbols_java_lang_tests.rs` | ✅ |
| languages.javascript | covered by typescript+JS tests | ✅ |
| languages.python | `extractors/symbols_python_lang_tests.rs` | ✅ |
| languages.rust | covered by extractor tests + symbol_table_tests | ✅ |
| languages.typescript | `extractors/typescript_tests.rs` | ✅ |

### Runtime Phases (5/5 PASS)

| Feature | Probe | Result |
|---|---|---|
| runtime.plan_phase | covered by theo-agent-runtime 1332 pass (state-machine tests) | ✅ |
| runtime.act_phase | covered by theo-agent-runtime tests | ✅ |
| runtime.observe_phase | covered by theo-agent-runtime tests | ✅ |
| runtime.reflect_phase | covered by theo-agent-runtime tests | ✅ |
| runtime.budget_enforcer | budget tests in theo-agent-runtime | ✅ |

## Threshold-Level DoD Gates (from `docs/sota-thresholds.toml`)

These are *separate* from feature-level probes. They aggregate across
features and define overall system maturity. Pulled current values from
the thresholds file (recorded by previous benchmark runs).

| DoD-gate | Floor | Current | Status |
|---|---|---|---|
| retrieval.mrr | 0.90 | 0.914 | ✅ PASS |
| retrieval.depcov | 0.96 | 0.967 | ✅ PASS |
| retrieval.recall_at_5 | 0.92 | 0.76 | ❌ **BELOW_FLOOR** (-17%) |
| retrieval.recall_at_10 | 0.95 | 0.86 | ❌ **BELOW_FLOOR** (-9%) |
| retrieval.ndcg_at_5 | 0.85 | unmeasured | ⚠️ **UNMEASURED** |
| retrieval.per_language_recall_at_5 | 0.85 (per lang × 14) | unmeasured | ⚠️ **UNMEASURED** |
| smoke.pass_rate | 0.85 | 0.90 (18/20) | ✅ PASS |
| swe_bench.minimum_runs | 3 | n/a | ⏭ paid API |

**Summary of threshold gates:** 3 PASS, 2 BELOW_FLOOR, 2 UNMEASURED, 1 paid-API SKIP.

## Conclusions

1. **All 36 HIGH priority features PASS** at the unit-test / static-probe level.
2. **Codebase health is excellent:** 4360+ lib tests green, 0 clippy warnings,
   0 architecture violations, 0 expired allowlist sunsets.
3. **The real gaps are at the threshold level, not the feature level:**
   - `retrieval.recall_at_5` is **17 percentage points below floor**
   - `retrieval.recall_at_10` is **9 points below floor**
   - `retrieval.ndcg_at_5` and `per_language_recall_at_5` are **unmeasured**
4. 87 MEDIUM/LOW features remain untested in this iteration (deferred per
   protocol — HIGH first).

**Phase 2 ANALYZE should target retrieval recall**, since:
- It is the worst-performing measurable DoD-gate (priority × impact = highest).
- It maps directly to the GRAPHCTX engine in `theo-engine-retrieval`.
- It blocks the global "context engineering ≥ SOTA" promise.

<!-- FEATURES_STATUS:total=123,passing=36,failing=0 -->
<!-- PHASE_1_COMPLETE -->
