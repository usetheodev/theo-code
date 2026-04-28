# Plan: Code Hygiene 5×5 — drive remaining allowlists to zero (or codified)

> **Version 1.0** — close the residual hygiene gap that keeps the **Code hygiene**
> dimension at **4.5 / 5** instead of **5.0**. After the god-files plan landed
> (53 → 10 size entries, 81 % resolved), the remaining debt is concentrated in 6
> non-size allowlists totaling **158 entries**. This plan splits that debt into
> 4 frentes — mechanical test split, complexity decomposition, codified-idiom
> ADR, and Cat-B unwrap typed-error refactor — and walks each to zero with
> per-task TDD, file-by-file boundaries, and a coverage matrix that maps every
> remaining entry to a concrete task. Outcome: every allowlist file ends with
> 0 active entries (or the entries are explicitly codified as "accepted
> patterns" via ADR-021 and the gate validates patterns instead of tolerating
> exceptions). Target: **2026-06-30** (8 weeks).

## Context

### What exists today

After `god-files-2026-07-23-plan.md` closed (commit `2656fbd`), the workspace
has these residual allowlists (verified 2026-04-28):

| Allowlist | Entries | Nature |
|---|---:|---|
| `size-allowlist.txt` | **10** | sibling test files >800 LOC |
| `complexity-allowlist.txt` | **8** | per-crate CCN > 10 ceilings |
| `unwrap-allowlist.txt` | **34** | regex + path patterns (mostly idioms) |
| `unsafe-allowlist.txt` | **5** | `env::set_var`/`remove_var` in tests |
| `panic-allowlist.txt` | **2** | startup panics |
| `secret-allowlist.txt` | **5** | false-positive secret patterns |
| `io-test-allowlist.txt` | **94** | inline I/O tests (legacy) |
| **Total** | **158** | |

`bash scripts/check-sizes.sh` PASS (10 oversize, 0 NEW, 0 EXPIRED).
`bash scripts/check-unwrap.sh` PASS (0 violations, 90 allowlisted).
`bash scripts/check-unsafe.sh` PASS (0 violations, 26 allowlisted).
`bash scripts/check-arch-contract.sh` PASS (0 violations).
`bash scripts/check-sota-dod.sh --quick` PASS (12/12, 2 SKIP paid LLM).

The system is at maturity **3.8/5** with code hygiene specifically at **4.5/5**.
Going to 5.0 in code hygiene closes a measurable, non-controversial gap.

### Why each allowlist still has entries

**`size-allowlist`** (10): every entry is a sibling `_tests.rs` file >800 LOC.
The production halves are clean (god-files plan resolved them). The test
halves accumulate because we extract per-file tests but don't sub-split.

**`complexity-allowlist`** (8): per-crate ceilings allowing functions with
clippy `too_many_lines` warnings. Many were tied to god-files that have now
been split — the ceilings are likely already over-provisioned and could drop
to zero if we re-measure post-decomposition.

**`unwrap-allowlist`** (34): 90 % are *legitimate Rust idioms* (mutex poison,
UNIX clock invariants, schema validation). Removing them by refactoring to
typed errors adds verbosity without safety; codifying them as accepted patterns
via an ADR is more honest.

**`unsafe-allowlist`** (5): `Rust 2024 test-only env::set_var/remove_var; tests
run serially`. Same idiom 5 times.

**`panic-allowlist`** (2): startup panics where there's no recoverable path
(invalid built-in tool schema, malformed compile-time regex literal).

**`secret-allowlist`** (5): false positives in test fixtures.

**`io-test-allowlist`** (94): inline I/O tests in production files; ADR-017
already accepts this pattern.

### Evidence trail

- `docs/plans/god-files-2026-07-23-plan.md` — antecessor que resolveu 81 %
- `docs/adr/020-renewed-size-allowlist-2026-07-15.md` — ADR explicando entries renovadas
- `docs/audit/maturity-gap-analysis-2026-04-27.md` — score baseline
- `bash scripts/check-allowlist-progress.sh` — métrica reproducível

## Objective

**By 2026-06-30**, every allowlist in `.claude/rules/*-allowlist.txt` either:
- (a) Has **0 active entries** (debt removed by refactor), OR
- (b) Has its entries explicitly **codified as accepted patterns via ADR-021**,
  with the gate evolving from "tolerate exceptions" to "validate patterns".

Specific goals:

1. **Phase 1 (week 1):** Cat-B unwrap in `cluster/{subdivide,lpa}.rs` converted
   to typed errors (Bonus quick win, ADR-019).
2. **Phase 2 (week 2):** ADR-021 codifies the 34 + 5 + 2 + 5 idiomatic entries
   (`unwrap`, `unsafe`, `panic`, `secret`); gate scripts evolve from allowlist-
   tolerance to pattern-validation.
3. **Phase 3 (weeks 3-4):** 10 sibling `_tests.rs` files split per-tool /
   per-feature; `size-allowlist` zero entries.
4. **Phase 4 (weeks 5-7):** 8 complexity allowlist entries reduced to zero by
   extracting helper functions from the longest fns in each crate.
5. **Phase 5 (week 8):** `io-test-allowlist` (94) audited; legacy entries
   moved to `tests/` directory or kept under codified ADR-017 with the same
   "patterns not exceptions" treatment.
6. **Throughout:** `cargo test --workspace --exclude theo-code-desktop --no-fail-fast`
   stays green; no test regressions; no allowlist NEW entries.

## ADRs

### D1 — Treat *legitimate Rust idioms* as codified patterns, not exceptions

**Decision:** Convert `unwrap-allowlist`, `unsafe-allowlist`, `panic-allowlist`,
`secret-allowlist` from "list of tolerated lines" into a codified ADR-021
catalog of accepted patterns. The gate scripts evolve from
`if line in allowlist: skip` to
`if line matches one of the codified patterns: skip; else: fail`.

**Rationale:** These 46 entries are not "debt" in the same sense as god-files.
They are language-level idioms (`Mutex::lock().expect("poisoned")`, `expect`
on `SystemTime::duration_since(UNIX_EPOCH)`, etc.) where:
- The invariant is type-system-inexpressible.
- The panic message is the contract.
- Refactoring to typed errors adds verbosity without safety gain.

Treating them as exceptions means anyone adding the *same* idiom must add a
new allowlist entry — turning idiomatic Rust into bureaucracy. Codifying them
as patterns reverses the polarity: the codebase has *no exceptions*, only
*recognized patterns*.

**Consequences:** Score 5/5 in code hygiene becomes earnable without 100 %
zero-allowlist (which is unrealistic for a Rust project of this size).
Requires ADR-021 to enumerate each pattern with an exact regex + use-case
+ alternative considered. Gate scripts gain a `recognized_patterns.toml`
config file. New code can use these idioms without paperwork.

### D2 — Test-half split mirrors production-half split (D2 from god-files plan)

**Decision:** When a sibling `_tests.rs` file exceeds 800 LOC, split it
following the **same logical decomposition** as the production half.
For tool families (DAP, plan, LSP) the production split was per-tool;
the test sibling splits per-tool. For algorithm modules (run_engine,
cluster) the split is per-phase / per-strategy.

**Rationale:** Test code organization should mirror production code
organization. Following the same boundaries makes "find the test for tool X"
trivial: same filename, `_tests` suffix.

**Consequences:** 10 sibling files become 30-50 smaller `_tests.rs` files
(one per tool/feature). Each ≤ 200 LOC typically. `cargo test -p X` test
discovery unaffected.

### D3 — Complexity ceilings drop to zero when production halves are clean

**Decision:** After god-files plan, the production halves of every crate
are under 800 LOC. Re-run `cargo clippy --warn clippy::too_many_lines`
per crate, identify the *current* longest functions, extract them into
helpers, then drop the per-crate complexity ceiling to zero.

**Rationale:** The complexity ceilings were sized in 2026-04-27 when god-files
existed. Many of the longest functions *were* the god-files. Now that those
are split, the ceilings are over-provisioned. We can lock in the new
post-decomposition floor.

**Consequences:** Each crate gets 1-2 small refactor commits. New CCN
violations gate-block immediately (no slack to absorb regressions).

### D4 — IO-test allowlist becomes ADR-017 v2

**Decision:** The 94 entries in `io-test-allowlist.txt` are mostly inline
tests with `tempfile::tempdir()` + small file writes. They are correct per
ADR-017 but bloat the allowlist. Move the *file-list* into ADR-017 as
"recognized inline-IO test pattern requirements" — ADR-017 v2 codifies
**when** inline IO tests are allowed (with tempfile, no shared mutable state,
no real filesystem mutation outside the temp dir). The gate validates the
pattern requirements, not the file list.

**Rationale:** Same logic as D1 applied to a different idiom. ADR-017
already exists; just version it up.

**Consequences:** `io-test-allowlist.txt` becomes empty / removed. Gate
script reads a small set of pattern requirements from ADR-017 v2. New
inline IO tests must follow the requirements; existing 94 don't need
individual entries.

## Dependency Graph

```
Phase 1 (cluster.rs Cat-B → typed errors)         [1 day]
  │
  ▼
Phase 2 (ADR-021: codify idiom allowlists)        [1 week]
  │
  ▼
   ┌────────────────────┬──────────────────────────┐
   │                    │                          │
   ▼                    ▼                          ▼
Phase 3              Phase 4                    Phase 5
(test sibling      (complexity decomp,        (ADR-017 v2 +
split, 10 files)    8 crates)                  io-test prune)
[2 weeks]           [2-3 weeks]                [1 week]
   │                    │                          │
   └────────────────────┴──────────────────────────┘
                        │
                        ▼
              Phase 6 (final validation)
              [2 days]
```

**Sequential blockers:** Phase 1 → 2 (ADR-021 references the cluster fix).
Phase 2 → 3, 4, 5 (ADR-021 sets the polarity).
**Parallel-safe:** Phases 3, 4, 5 can run concurrently — they touch
different surface areas (test files vs production fns vs allowlist files).

---

## Phase 1: Cat-B unwrap → typed errors in `cluster/{subdivide,lpa}.rs`

**Objective:** Eliminate the 7 Cat-B unwrap entries (sunset 2026-08-31)
inherited from cluster.rs B1 by converting Louvain heuristics to typed
errors.

### T1.1 — Add `ClusterError` variant for missing-label/non-orderable cases

#### Objective

Introduce a single typed error returned by Louvain heuristics so the 7
`.unwrap()` calls become `.ok_or(ClusterError::*)?`.

#### Evidence

```
bash scripts/check-unwrap.sh --report 2>&1 | grep "cluster/(subdivide|lpa)"
```
returns 7 sites; the regex allowlist entry has sunset 2026-08-31 (per
this plan's antecessor commit `2656fbd`).

#### Files to edit

```
crates/theo-engine-graph/src/cluster/types.rs — add ClusterError enum + impl Error
crates/theo-engine-graph/src/cluster/subdivide.rs — return Result; replace 5 unwraps
crates/theo-engine-graph/src/cluster/lpa.rs — return Result; replace 2 unwraps
crates/theo-engine-graph/src/cluster/mod.rs — re-export ClusterError
crates/theo-engine-graph/src/lib.rs — re-export ClusterError if needed
.claude/rules/unwrap-allowlist.txt — remove the cluster/* regex entry
docs/adr/019-cluster-louvain-typed-errors.md — (NEW) ADR
```

#### Deep file dependency analysis

- **cluster/types.rs**: currently holds `Community` and `ClusterResult`. Adding
  `ClusterError` keeps the type module cohesive. No downstream consumer (since
  current public API only returns `ClusterResult`/`Vec<Community>`).
- **cluster/subdivide.rs**: 5 unwrap sites in `subdivide_community` /
  `subdivide_with_lpa_seeded`. Both currently return `Vec<Community>`. Need to
  decide: bubble the error up (change signature → break callers), or recover
  gracefully (return empty community + log) when invariant fails. Decision:
  bubble (D2 of this Phase 1) — invariants are programming errors.
- **cluster/lpa.rs**: 2 unwrap sites in `lpa_seeded`. Same treatment.
- **cluster/mod.rs**: needs `pub use types::ClusterError;`.
- **Callers**: `theo-engine-retrieval/src/search/multi.rs` calls
  `detect_communities`. Verify it currently doesn't catch — if it `unwrap`ed
  the result, the change is breaking. (Check before refactor.)

#### Deep Dives

- `ClusterError` enum:
  ```rust
  #[derive(Debug, thiserror::Error)]
  pub enum ClusterError {
      #[error("missing label for node `{0}`; partition is inconsistent")]
      MissingLabel(String),
      #[error("partial_cmp returned None on edge weight; edge weights must be finite")]
      NonOrderableWeight,
  }
  ```
- Invariant before/after: callers must handle the `Result` (or unwrap with
  context); the contract becomes typed instead of panic-via-unwrap.
- Edge cases: graph with NaN edge weight (real bug — surface it), empty
  community (already handled by early return).

#### Tasks

1. Read `cluster/subdivide.rs` and `cluster/lpa.rs` to map exactly which
   functions need signature changes.
2. Grep all callers of `detect_communities` / `leiden_communities` /
   `lpa_seeded` / `subdivide_community` to scope the breaking change.
3. Add `ClusterError` to `cluster/types.rs`.
4. Update fn signatures in `subdivide.rs` and `lpa.rs` to return
   `Result<T, ClusterError>`.
5. Replace 7 `.unwrap()` with `.ok_or(ClusterError::*)?`.
6. Update callers (likely 1-2 files).
7. Write ADR `docs/adr/019-cluster-louvain-typed-errors.md`.
8. Remove the regex entry from `unwrap-allowlist.txt`.
9. Run gate: `bash scripts/check-unwrap.sh; echo $?` should still be 0.

#### TDD

```
RED:     test_subdivide_community_returns_missing_label_when_label_set_incomplete()
RED:     test_lpa_seeded_returns_non_orderable_weight_when_nan_edge()
RED:     test_cluster_error_implements_std_error_trait_with_descriptive_message()
GREEN:   Add ClusterError + propagate via ?.
REFACTOR: None expected — invariant doc-comments stay alongside the typed-error returns.
VERIFY:  cargo test -p theo-engine-graph
```

#### Acceptance Criteria

- [ ] `cluster/types.rs` exposes `ClusterError` with at least 2 variants.
- [ ] All 7 `.unwrap()` sites in `cluster/{subdivide,lpa}.rs` replaced with `?`.
- [ ] Public API of `theo-engine-graph::cluster` includes `ClusterError`.
- [ ] `unwrap-allowlist.txt` no longer has the cluster regex entry.
- [ ] `bash scripts/check-unwrap.sh` exit 0.
- [ ] All callers updated to handle `Result`.
- [ ] ADR-019 written and linked from this plan.
- [ ] Pass: cargo clippy zero warnings.

#### DoD

- [ ] `cargo test -p theo-engine-graph` green.
- [ ] `cargo test --workspace --exclude theo-code-desktop --no-fail-fast` green.
- [ ] `bash scripts/check-unwrap.sh` exit 0.
- [ ] CHANGELOG `[Unreleased] / Changed` entry referencing ADR-019.

---

## Phase 2: ADR-021 codifies legitimate idiom allowlists

**Objective:** Convert 4 allowlists (`unwrap`, `unsafe`, `panic`, `secret`)
from "tolerate listed lines" into "validate against codified patterns",
removing the per-line debt accumulator while keeping the gate strictness.

### T2.1 — Author ADR-021 — "Recognized Rust idioms in production paths"

#### Objective

A single-source-of-truth ADR that catalogs every pattern the project
accepts as idiomatic, with regex + invariant + alternative-considered.

#### Evidence

```
.claude/rules/unwrap-allowlist.txt | grep -c '^regex:'  → 11 regex entries
.claude/rules/unwrap-allowlist.txt | grep -cv '^#\|^$' → 34 active entries
.claude/rules/unsafe-allowlist.txt → 5 entries (all "Rust 2024 test-only env::set_var")
.claude/rules/panic-allowlist.txt → 2 entries (startup panics)
.claude/rules/secret-allowlist.txt → 5 entries (false positives)
```

These 46 entries cluster into ~8 unique patterns. ADR-021 enumerates the
patterns once.

#### Files to edit

```
docs/adr/021-recognized-rust-idioms.md — (NEW) ADR with each pattern
.claude/rules/recognized-patterns.toml — (NEW) machine-readable pattern list
```

#### Deep file dependency analysis

- **ADR-021**: prose document explaining each pattern, the invariant, the
  alternative considered (and why rejected). Read by humans. Linked from
  CLAUDE.md and from the gate script error messages.
- **recognized-patterns.toml**: machine-readable; consumed by check-unwrap.sh
  / check-unsafe.sh / check-panic.sh / check-secrets.sh. Format:
  ```toml
  [[unwrap_pattern]]
  name = "mutex_poison_lock"
  regex = '\.(lock|read|write)\(\)\.expect\(.*(poisoned|lock)'
  scope = "crates/**/*.rs"
  invariant = "Mutex/RwLock poison is unrecoverable; .expect() documents the invariant."
  adr = "021-recognized-rust-idioms.md#mutex-poison-lock"
  ```

#### Deep Dives

Patterns to codify (8 inferred from existing allowlists):

1. **mutex_poison_lock** — `.lock().expect("poisoned")` and friends.
2. **system_clock_unix_epoch** — `SystemTime::duration_since(UNIX_EPOCH).expect("system clock before UNIX epoch")`.
3. **embedded_tool_schema_valid** — `expect("<tool> schema is valid")` on compile-time embedded const data validated by `build_registry` contract test.
4. **process_entrypoint_runtime_init** — `tokio::runtime::Runtime::new().expect()` at CLI/binary entrypoint.
5. **observability_writer_spawn** — thread spawn / metrics lock that is fatal-by-design.
6. **rust_2024_test_env_var** — `unsafe { std::env::set_var(...) }` inside `#[cfg(test)]` (Rust 2024 made env_var unsafe).
7. **builtin_tool_schema_panic** — startup panic if a built-in tool ships an invalid schema (registry/mod.rs:217).
8. **observability_normalizer_compile_panic** — panic guard for `OnceLock<Pattern>` with compile-time-validated literal.

Plus secret-allowlist patterns:
9. **test_fixture_dummy_keys** — patterns that look like API keys but live in `tests/`/`fixtures/`.

For each pattern: the regex + the in-code idiom + tests proving the invariant
holds in real cases.

#### Tasks

1. Inventory all entries across the 4 allowlists; group by pattern.
2. For each pattern: write ADR-021 section with invariant, justification,
   alternative considered.
3. Author `recognized-patterns.toml` with regex + scope + ADR anchor per
   pattern.
4. Write a Rust example for each pattern showing correct vs incorrect usage.

#### TDD

```
RED:     test_recognized_patterns_toml_loads_and_compiles_all_regexes()
RED:     test_every_existing_allowlist_regex_has_a_recognized_pattern_match()
RED:     test_each_pattern_has_at_least_one_real_use_in_codebase()
GREEN:   Author ADR-021 + recognized-patterns.toml + tests against the live
         tree.
REFACTOR: None expected — first iteration.
VERIFY:  cargo test -p theo-governance --lib hygiene
```

#### Acceptance Criteria

- [ ] `docs/adr/021-recognized-rust-idioms.md` exists with ≥ 9 sections.
- [ ] `.claude/rules/recognized-patterns.toml` lists ≥ 9 entries.
- [ ] Each pattern has: regex, scope, invariant, adr-anchor, alternative-rejected.
- [ ] Self-test: every existing regex in unwrap/unsafe/panic/secret allowlists
      maps to at least one ADR-021 pattern.
- [ ] CLAUDE.md and `.theo/AGENTS.md` reference ADR-021.

#### DoD

- [ ] ADR-021 reviewed and committed.
- [ ] recognized-patterns.toml validated by self-test.
- [ ] Cross-references to ADR-021 added everywhere allowlists are mentioned.

### T2.2 — Update gate scripts to read recognized-patterns.toml

#### Objective

Each of `check-unwrap.sh`, `check-unsafe.sh`, `check-panic.sh`,
`check-secrets.sh` reads recognized-patterns.toml *first*, then falls back to
the legacy allowlist for any remaining entries.

#### Evidence

The current scripts are 200-300 LOC bash with ad-hoc regex parsers. Adding
TOML support is mechanical (use `tq` / `python3 -c 'import tomllib'`).

#### Files to edit

```
scripts/check-unwrap.sh — load recognized-patterns.toml + check unwrap_pattern entries
scripts/check-unsafe.sh — same for unsafe_pattern
scripts/check-panic.sh  — same for panic_pattern
scripts/check-secrets.sh — same for secret_pattern
scripts/check-recognized-patterns.sh — (NEW) shared pattern loader (dev convenience)
```

#### Deep file dependency analysis

- Each script gains a `load_recognized_patterns()` helper that:
  1. `python3 -c 'import tomllib; ...'` to parse the TOML.
  2. Echoes pattern entries in the same `regex:<glob>@@<regex>@@<sunset>@@<reason>` format the existing scripts already understand.
- Backward compat: legacy allowlist entries continue to work; the TOML is
  appended to the in-memory list before processing.

#### Deep Dives

- Avoid changing the public CLI of each script (`--report`, `--json`,
  `--strict` flags stay).
- Make the TOML loader a single function so the 4 scripts can `source` it
  without duplication.

#### Tasks

1. Write `scripts/check-recognized-patterns.sh` (the loader).
2. In each of the 4 gate scripts, add `source check-recognized-patterns.sh` +
   merge the TOML patterns into the pattern array.
3. Add per-pattern coverage assertion: every TOML pattern matches at least
   one production line (otherwise it's dead).

#### TDD

```
RED:     test_check_unwrap_loads_recognized_patterns_toml()
RED:     test_check_unwrap_treats_toml_pattern_match_as_allowed()
RED:     test_check_unwrap_fails_when_unknown_pattern_appears_in_production()
GREEN:   Implement the loader and merge logic.
REFACTOR: Extract loader to shared script.
VERIFY:  bash scripts/check-recognized-patterns.test.sh
```

#### Acceptance Criteria

- [ ] All 4 gate scripts read recognized-patterns.toml.
- [ ] All 4 scripts still exit 0 on the current tree.
- [ ] A test fixture introduces an *unrecognized* idiom in production — the
      gate fails with an error message pointing at ADR-021.
- [ ] No dead patterns: every TOML entry has at least one match in the tree.
- [ ] Pass: complexity ≤ 10 per fn in the new loader.

#### DoD

- [ ] All 4 gates exit 0.
- [ ] Test fixture (in `scripts/check-recognized-patterns.test.sh`) green.
- [ ] CHANGELOG entry under `[Unreleased] / Changed`.

### T2.3 — Drain idiomatic allowlists (`unwrap`, `unsafe`, `panic`, `secret`)

#### Objective

For each of the 46 entries that maps to a recognized pattern, *remove* it
from its allowlist. The pattern in recognized-patterns.toml does the same
job globally.

#### Evidence

After T2.2 lands, the gate already accepts these idioms via patterns. The
allowlist entries are duplicates (and waste sunsets).

#### Files to edit

```
.claude/rules/unwrap-allowlist.txt — remove 11 regex entries + matching path entries
.claude/rules/unsafe-allowlist.txt — remove 5 entries
.claude/rules/panic-allowlist.txt  — remove 2 entries
.claude/rules/secret-allowlist.txt — remove 5 entries
```

#### Deep file dependency analysis

- Each allowlist gets pruned. After this task, `unsafe-allowlist.txt` and
  `panic-allowlist.txt` and `secret-allowlist.txt` should be **empty** (just
  comments). `unwrap-allowlist.txt` retains only the path-specific
  whole-file allowlists for true test fixtures (`mock_llm.rs`, etc.).

#### Tasks

1. For each allowlist, walk the entries and identify which map to a
   recognized pattern.
2. Remove those entries (replace with `# COVERED BY ADR-021#<pattern>` comments).
3. Re-run each gate; every must still exit 0.
4. Update CHANGELOG.

#### TDD

```
RED:     test_no_idiomatic_entries_remain_in_unsafe_allowlist()
RED:     test_no_idiomatic_entries_remain_in_panic_allowlist()
RED:     test_no_idiomatic_entries_remain_in_secret_allowlist()
RED:     test_unwrap_allowlist_only_has_test_fixture_path_entries()
GREEN:   Drain the allowlists.
REFACTOR: None.
VERIFY:  bash scripts/check-{unwrap,unsafe,panic,secrets}.sh; all exit 0
```

#### Acceptance Criteria

- [ ] `unsafe-allowlist.txt`: 0 active entries.
- [ ] `panic-allowlist.txt`: 0 active entries.
- [ ] `secret-allowlist.txt`: 0 active entries.
- [ ] `unwrap-allowlist.txt`: ≤ 5 active entries (only true test-fixture
      whole-file allowlists).
- [ ] Total allowlists drained: 46 → ≤ 5 active entries.

#### DoD

- [ ] All 4 gates exit 0.
- [ ] CHANGELOG `[Unreleased] / Changed` entry.

---

## Phase 3: Sibling test split — drive `size-allowlist` to 0

**Objective:** Apply ADR D2 (test-half mirrors production-half) to the 10
remaining sibling `_tests.rs` files >800 LOC.

### T3.1 — DAP `tool_tests.rs` (1281 LOC, 73 tests) → 11 per-tool

#### Objective

Split `crates/theo-tooling/src/dap/tool_tests.rs` into 11 sibling files,
one per Debug*Tool, each ≤ 200 LOC.

#### Evidence

Earlier attempt during the god-files plan failed due to a regex bug in the
prefix matcher (`set_breakpoint` and `stack_trace` weren't recognized
because the non-greedy `\w+?` only matched up to the first `_`). The fix
is mechanical.

#### Files to edit

```
crates/theo-tooling/src/dap/tool_tests.rs — DELETED
crates/theo-tooling/src/dap/status_tests.rs    — (NEW) 4 tests
crates/theo-tooling/src/dap/launch_tests.rs    — (NEW) 7 tests
crates/theo-tooling/src/dap/breakpoint_tests.rs — (NEW) ~10 tests
crates/theo-tooling/src/dap/continue_tests.rs   — (NEW) 4 tests
crates/theo-tooling/src/dap/step_tests.rs       — (NEW) 6 tests
crates/theo-tooling/src/dap/eval_tests.rs       — (NEW) 7 tests
crates/theo-tooling/src/dap/stack_trace_tests.rs — (NEW)
crates/theo-tooling/src/dap/variables_tests.rs   — (NEW) 6 tests
crates/theo-tooling/src/dap/scopes_tests.rs      — (NEW) 4 tests
crates/theo-tooling/src/dap/threads_tests.rs     — (NEW) 3 tests
crates/theo-tooling/src/dap/terminate_tests.rs   — (NEW) 5 tests
crates/theo-tooling/src/dap/test_helpers.rs      — (NEW) shared make_ctx + empty_manager
crates/theo-tooling/src/dap/mod.rs — attach each per-tool _tests.rs via #[path]
.claude/rules/size-allowlist.txt — remove dap/tool_tests.rs entry
```

#### Deep file dependency analysis

- Each per-tool test file shares `make_ctx` and `empty_manager` helpers. Those
  go to `test_helpers.rs` (cfg(test) + pub(super)) so each per-tool file
  imports them.
- `dap/mod.rs` already attaches `tool_tests.rs` via `#[cfg(test)] #[path]
  mod tests;`. Replace with one `mod` per per-tool file:
  ```rust
  #[cfg(test)]
  mod dap_tests {
      mod status_tests;
      mod launch_tests;
      // ...
  }
  ```

#### Deep Dives

- The prefix-matching bug in the previous attempt: regex
  `t131tool_(\w+?)_\w+` with non-greedy `\w+?` matched up to first `_`,
  producing `set` for `t131tool_set_breakpoint_id_and_category`. Fix:
  match all candidate prefixes by descending length with `startswith`.
- Test names already encode the tool: `t131tool_<tool_id>_<assertion>` —
  trivial to grep.
- After split: `cargo test -p theo-tooling --lib dap` must show **same 73
  tests** (no count drop).

#### Tasks

1. Write a Python splitter that:
   - Lists tools (status, launch, set_breakpoint, continue, step, eval,
     stack_trace, variables, scopes, threads, terminate).
   - Iterates fns in tool_tests.rs; matches longest tool prefix.
   - Writes per-tool _tests.rs with header + helpers + matching fns.
2. Extract `make_ctx` + `empty_manager` to `test_helpers.rs`.
3. Update `dap/mod.rs` to attach per-tool _tests.rs files.
4. Delete `tool_tests.rs`.
5. Update size-allowlist.

#### TDD

```
RED:     test_dap_tool_tests_count_unchanged_post_split() — manual: cargo test -p theo-tooling --lib dap → 73 tests.
GREEN:   Run the splitter; verify cargo test green.
REFACTOR: Lift any duplicated helper into test_helpers.rs.
VERIFY:  cargo test -p theo-tooling --lib dap
         bash scripts/check-sizes.sh — dap/tool_tests.rs gone.
```

#### Acceptance Criteria

- [ ] 11 per-tool `_tests.rs` files exist; each ≤ 200 LOC.
- [ ] `dap/test_helpers.rs` ≤ 50 LOC.
- [ ] `dap/tool_tests.rs` deleted.
- [ ] `cargo test -p theo-tooling --lib dap` → 73 PASS.
- [ ] `size-allowlist.txt` lost the dap/tool_tests.rs entry (10 → 9).
- [ ] `cargo clippy --all-targets -D warnings` zero.

#### DoD

- [ ] All 73 dap tests still passing.
- [ ] Allowlist trimmed.
- [ ] Cross-sibling imports clean.

### T3.2 — `run_engine/mod_tests.rs` (1252 LOC) → split per phase

#### Objective

Split into per-phase `_tests.rs` files following the production-half
boundaries (run_engine/mod.rs already split into bootstrap, builders,
contexts, dispatch, execution, handoff, lifecycle, llm_call, main_loop,
post_dispatch_updates, stream_batcher, text_response).

#### Evidence

run_engine/mod.rs is already a module-dir with sub-files; the test sibling
should mirror that.

#### Files to edit

```
crates/theo-agent-runtime/src/run_engine/mod_tests.rs — DELETED
crates/theo-agent-runtime/src/run_engine/dispatch_tests.rs — (NEW)
crates/theo-agent-runtime/src/run_engine/execution_tests.rs — (NEW)
crates/theo-agent-runtime/src/run_engine/lifecycle_tests.rs — (NEW)
crates/theo-agent-runtime/src/run_engine/main_loop_tests.rs — (NEW)
crates/theo-agent-runtime/src/run_engine/post_dispatch_tests.rs — (NEW)
crates/theo-agent-runtime/src/run_engine/test_helpers.rs — (NEW) shared
crates/theo-agent-runtime/src/run_engine/mod.rs — attach each
.claude/rules/size-allowlist.txt — remove run_engine/mod_tests.rs entry
```

#### Tasks/TDD/Acceptance/DoD: same shape as T3.1.

### T3.3 — `symbols_tests.rs` (1132 LOC) → per-language

(Mirrors symbols.rs; per-language tests for csharp/go/java/javascript/php/python/ruby/rust/typescript.)

### T3.4 — `domain/plan_tests.rs` (1090 LOC) → per-feature

(Mirrors create/update/advance/replan/log/summary/next_task/failure_status sections of plan.rs.)

### T3.5 — `subagent/mod_tests.rs` (1017 LOC) → per-phase

(Mirrors spawn/run/resume/reap of subagent/mod.rs.)

### T3.6 — `plan/mod_tests.rs` (937 LOC) → per-tool

(Mirrors the 8 plan_* tools.)

### T3.7 — Smaller siblings (`registry/mod_tests`, `lsp/tool_tests`,
`symbol_table_tests`, `subagent/resume_tests` — each 826-832 LOC)

Single-pass split each by the dominant grouping in their fn names.

---

## Phase 4: Complexity decomposition (8 → 0 entries)

**Objective:** Re-measure clippy `too_many_lines` per crate post-god-files;
extract helpers from the longest functions; lock in the new floor.

### T4.1 — Re-measure clippy CCN per crate

#### Objective

Get an honest current count of functions over 100 LOC per crate.

#### Files to edit

```
docs/audit/complexity-baseline-2026-04-29.md — (NEW) snapshot
scripts/check-complexity-baseline.sh — (NEW)
```

#### Tasks

1. For each of the 8 crates in `complexity-allowlist`, run
   `cargo clippy -p X -- -W clippy::too_many_lines 2>&1 | grep too_many_lines | wc -l`.
2. Compare against the per-crate ceiling.
3. Identify the per-crate target ceiling (current count).

#### Acceptance Criteria

- [ ] Snapshot exists.
- [ ] Per-crate current count documented.
- [ ] Delta vs current allowlist ceiling computed.

### T4.2..T4.9 — One task per crate (8 tasks)

Per crate: extract the 1-3 longest functions into helpers until the count
drops to 0 OR drops below a sensible new floor.

Per task template (T4.2 example for `theo-engine-retrieval`, ceiling 24):

#### T4.2 — Drop `theo-engine-retrieval` complexity ceiling 24 → 0

##### Files to edit

```
crates/theo-engine-retrieval/src/assembly/{greedy,reading,building,codeasm,direct}.rs — extract helpers
crates/theo-engine-retrieval/src/search/{multi,file_bm25,bm25_index}.rs — extract helpers
crates/theo-engine-retrieval/src/file_retriever.rs — extract helpers
.claude/rules/complexity-allowlist.txt — remove theo-engine-retrieval entry
```

##### TDD

```
RED:     test_extracted_helper_for_X_returns_same_result_as_inline()
GREEN:   Extract helper.
REFACTOR: Verify helper is testable in isolation.
VERIFY:  cargo test -p theo-engine-retrieval
         cargo clippy -p theo-engine-retrieval -- -W clippy::too_many_lines → 0 warnings
```

##### Acceptance Criteria

- [ ] `cargo clippy -p theo-engine-retrieval -- -W clippy::too_many_lines` → 0.
- [ ] Allowlist entry removed.
- [ ] Public API unchanged.

(Repeat T4.3..T4.9 for theo, theo-infra-llm, theo-agent-runtime,
theo-application, theo-tooling, theo-domain, theo-engine-graph.)

---

## Phase 5: ADR-017 v2 — codify inline I/O test pattern

**Objective:** Drain `io-test-allowlist` (94 entries) into ADR-017 v2.

### T5.1 — Author ADR-017 v2 — "Inline I/O test pattern requirements"

#### Objective

Codify *when* inline I/O tests are allowed and what invariants must hold.

#### Files to edit

```
docs/adr/017-inline-io-tests.md — (UPDATE) v2 with codified requirements
.claude/rules/recognized-patterns.toml — add inline_io_test_pattern
.claude/rules/io-test-allowlist.txt — clear all 94 entries
scripts/check-inline-io-tests.sh — load TOML pattern, validate requirements
```

#### Pattern requirements

1. Test must be inside `#[cfg(test)]` (not `#![cfg(test)]` at file top —
   that's the sibling pattern, separate D2).
2. Filesystem mutations only via `tempfile::tempdir()` — no `/tmp` or
   `std::env::temp_dir()` direct.
3. No shared mutable state between tests in the same file.
4. Test names follow the project convention (`test_<behavior>_<condition>`).

#### TDD

```
RED:     test_check_inline_io_tests_loads_recognized_patterns_toml()
RED:     test_inline_io_test_with_tempdir_passes()
RED:     test_inline_io_test_with_shared_static_fails()
GREEN:   Implement pattern validator.
REFACTOR: Share TOML loader with Phase 2.
VERIFY:  bash scripts/check-inline-io-tests.sh; exit 0
```

#### Acceptance Criteria

- [ ] ADR-017 v2 has codified requirements.
- [ ] `recognized-patterns.toml` has `inline_io_test_pattern` entry.
- [ ] `io-test-allowlist.txt` is empty (just comments + ADR-017 v2 link).
- [ ] Gate validates requirements, not file list.
- [ ] All 94 previously-allowlisted tests still pass under the new pattern
      validator.

#### DoD

- [ ] Gate exit 0.
- [ ] All 94 files validated as conforming.
- [ ] CHANGELOG entry.

---

## Phase 6: Final validation + sunset close

### T6.1 — Final validation

#### Tasks

1. `cargo test --workspace --exclude theo-code-desktop --no-fail-fast`
2. `cargo clippy --workspace --exclude theo-code-desktop --all-targets -D warnings`
3. `make check-arch`
4. `bash scripts/check-unwrap.sh`
5. `bash scripts/check-unsafe.sh`
6. `bash scripts/check-panic.sh`
7. `bash scripts/check-secrets.sh`
8. `bash scripts/check-sizes.sh`
9. `bash scripts/check-complexity.sh`
10. `bash scripts/check-inline-io-tests.sh`
11. `bash scripts/check-sota-dod.sh --quick`

All must exit 0.

#### Acceptance Criteria

- [ ] All 11 gates pass.
- [ ] All allowlists drained or codified via ADR.
- [ ] CHANGELOG `[Unreleased]` summarises the 5 phases.

### T6.2 — Maturity score update

Update `docs/audit/maturity-gap-analysis-*.md` with the new score:
**Code hygiene 4.5 → 5.0**.

---

## Coverage Matrix

| # | Allowlist entry / pattern | Phase / Task | Resolution |
|---|---|---|---|
| 1 | `unwrap-allowlist`: cluster Cat-B (7 sites) | T1.1 | Typed errors via ClusterError |
| 2 | `unwrap-allowlist`: 11 regex idioms | T2.1 + T2.3 | ADR-021 codifies; entries removed |
| 3 | `unwrap-allowlist`: 11+ path entries (test fixtures) | T2.3 | Keep ≤ 5; rest covered by patterns |
| 4 | `unsafe-allowlist`: 5 env_var entries | T2.1 + T2.3 | ADR-021 pattern `rust_2024_test_env_var` |
| 5 | `panic-allowlist`: 2 startup panics | T2.1 + T2.3 | ADR-021 pattern `builtin_tool_schema_panic` + `observability_normalizer_compile_panic` |
| 6 | `secret-allowlist`: 5 false positives | T2.1 + T2.3 | ADR-021 pattern `test_fixture_dummy_keys` |
| 7 | `size-allowlist`: dap/tool_tests.rs | T3.1 | Per-tool split |
| 8 | `size-allowlist`: run_engine/mod_tests.rs | T3.2 | Per-phase split |
| 9 | `size-allowlist`: symbols_tests.rs | T3.3 | Per-language split |
| 10 | `size-allowlist`: domain/plan_tests.rs | T3.4 | Per-feature split |
| 11 | `size-allowlist`: subagent/mod_tests.rs | T3.5 | Per-phase split |
| 12 | `size-allowlist`: plan/mod_tests.rs | T3.6 | Per-tool split |
| 13 | `size-allowlist`: 4 misc siblings (registry/lsp/symbol_table/resume) | T3.7 | Per-grouping split |
| 14 | `complexity-allowlist`: theo-engine-retrieval (24) | T4.2 | Helper extraction |
| 15 | `complexity-allowlist`: theo (11) | T4.3 | Helper extraction in cli/tui |
| 16 | `complexity-allowlist`: theo-infra-llm (10) | T4.4 | Helper extraction in providers |
| 17 | `complexity-allowlist`: theo-agent-runtime (10) | T4.5 | Helper extraction |
| 18 | `complexity-allowlist`: theo-application (9) | T4.6 | Helper extraction |
| 19 | `complexity-allowlist`: theo-tooling (8) | T4.7 | Helper extraction |
| 20 | `complexity-allowlist`: theo-domain (2) | T4.8 | Helper extraction |
| 21 | `complexity-allowlist`: theo-engine-graph (1) | T4.9 | Helper extraction |
| 22 | `io-test-allowlist`: 94 entries | T5.1 | ADR-017 v2 codifies pattern |

**Coverage: 158 / 158 entries (100 %)**

22 task IDs cover all 7 allowlist files. Each phase ends with the corresponding
allowlist drained or codified via ADR.

## Global Definition of Done

- [ ] All 6 phases completed by 2026-06-30.
- [ ] `cargo test --workspace --exclude theo-code-desktop --no-fail-fast` green.
- [ ] `cargo clippy --workspace --exclude theo-code-desktop --all-targets -D warnings` zero warnings.
- [ ] `bash scripts/check-arch-contract.sh` 0 violations.
- [ ] `bash scripts/check-unwrap.sh` exit 0; allowlist ≤ 5 path entries.
- [ ] `bash scripts/check-unsafe.sh` exit 0; allowlist 0 entries.
- [ ] `bash scripts/check-panic.sh` exit 0; allowlist 0 entries.
- [ ] `bash scripts/check-secrets.sh` exit 0; allowlist 0 entries.
- [ ] `bash scripts/check-sizes.sh` exit 0; allowlist 0 entries.
- [ ] `bash scripts/check-complexity.sh` exit 0; allowlist 0 entries.
- [ ] `bash scripts/check-inline-io-tests.sh` exit 0; allowlist 0 entries.
- [ ] `bash scripts/check-sota-dod.sh --quick` 12/12 PASS, 2 SKIP.
- [ ] `docs/adr/019-cluster-louvain-typed-errors.md` written.
- [ ] `docs/adr/021-recognized-rust-idioms.md` written with ≥ 9 patterns.
- [ ] `docs/adr/017-inline-io-tests.md` updated to v2.
- [ ] `.claude/rules/recognized-patterns.toml` exists with ≥ 10 patterns.
- [ ] CHANGELOG `[Unreleased]` summarises the 6 phases.
- [ ] `docs/audit/maturity-gap-analysis-*.md` updated: Code hygiene **5.0/5**.
- [ ] Plan-specific criterion: total active allowlist entries: 158 → ≤ 5
      (only true test-fixture whole-file allowlists remain on `unwrap`).

---

## Riscos e premissas

| Risco | Mitigação |
|---|---|
| Phase 4 (complexity decomp) requires understanding runtime-critical code (`run_engine::execute_with_history`, `assembly::assemble_with_code`). Extracting helpers may regress correctness. | Per task: write characterization tests *before* extraction; use the existing 5247-test suite as regression net; only extract pure helpers (no side effects). |
| ADR-021 (T2.1) requires consensus on what counts as a "recognized idiom". | Each pattern has a concrete invariant + alternative-considered + 1-2 named maintainers in the section header; deferred patterns go to a follow-up ADR. |
| Phase 3 (test split) introduces 30-50 new files; possible test discovery quirks. | Each per-tool _tests.rs uses `#[path = "..._tests.rs"] mod ...` from the parent mod's `mod tests {}` block; `cargo test -p X` does NOT need new flags. Verified during DAP split attempt in god-files plan. |
| recognized-patterns.toml + 4 gate scripts is a small new system to maintain. | All 4 scripts share `check-recognized-patterns.sh` loader; one place to maintain. |

## Métricas para acompanhar

```bash
# Snapshot inicial:
for f in .claude/rules/*-allowlist.txt; do
  printf "%-40s %3s\n" "$(basename "$f")" "$(grep -cvE '^(#|$)' "$f")"
done
```

Per checkpoint após cada Phase: re-run and diff. Total deve cair monotonicamente:

```
   Pre Phase 1:     158 entries
   Post Phase 1:    151 entries (-7)
   Post Phase 2:    109 entries (-46 + ADR-021 codifies)
   Post Phase 3:    99 entries (-10)
   Post Phase 4:    91 entries (-8)
   Post Phase 5:    -3 entries  (-94 + ADR-017 v2 codifies)  ⇒ ≤ 5 final
```

(The exact final number depends on how many true-fixture path entries remain
in `unwrap-allowlist` — those are kept as path-specific, not codified.)

## Não vai ser feito aqui (referências cruzadas)

| Item | Onde rastreio |
|---|---|
| Strategic gaps (sidecars E2E, mutation testing, multi-provider bench, SWE-Bench) | `docs/audit/maturity-gap-analysis-2026-04-27.md` §2 |
| Operational readiness (release pipeline, packages, SLA) | mesmo arquivo §2.10 |
| Future god-files that may emerge | `.claude/rules/size-allowlist.txt` + `make check-allowlist-progress` |
| Maturity score progression past 5.0 in code hygiene (other dimensions) | A ser rastreado em plano separado (3.8 → 4.0 maturity push) |
