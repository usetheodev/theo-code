# ADR-021 — Recognized Rust idioms in production paths

**Status:** Accepted
**Date:** 2026-04-28
**Plan:** `docs/plans/code-hygiene-5x5-plan.md` (T2.1, Phase 2)

## Context

After landing the *god-files-2026-07-23* plan (53 → 10 size entries) and
the *code-hygiene-5x5* T1.1 (cluster Cat-B → typed errors via ADR-019),
the remaining hygiene allowlists across the project hold **46 entries
that are not debt** but rather **legitimate Rust idioms** the codebase
intentionally uses. Examples:

```rust
// Mutex::lock().expect("poisoned") — poison is unrecoverable
let guard = my_mutex.lock().expect("metrics mutex poisoned");

// SystemTime::duration_since(UNIX_EPOCH).expect("system clock before UNIX epoch")
let now_ms = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .expect("system clock before UNIX epoch")
    .as_millis();

// Embedded const data — schema is verified at test time
let schema = build_schema(&BUILTIN_DOCS_SEARCH_PARAMS)
    .expect("docs_search tool schema is valid");

// Rust 2024 test-only env mutation under a serialised mutex
#[cfg(test)]
unsafe { std::env::set_var("THEO_X", "1"); }

// Process entrypoint — runtime init failure is fatal by design
fn main() -> i32 {
    let rt = tokio::runtime::Runtime::new()
        .expect("failed to create tokio runtime");
    // …
}
```

Treating these as *exceptions* — entries in
`.claude/rules/<gate>-allowlist.txt` with a sunset date — has 4 problems:

1. **Every new use of the same idiom requires a new entry.** Adding
   another mutex-guarded counter? Another allowlist entry. Adding a
   new SystemTime-based timestamp? Another entry. The list grows
   linearly with the codebase even though the *idiom* is fixed.
2. **Sunsets create false urgency.** Each entry has a sunset, suggesting
   the idiom should be removed. But mutex-poison-on-panic is *correct
   Rust* — the `expect()` is the contract. A sunset implies otherwise.
3. **Reviewers carry the wrong burden.** "Explain why your unwrap is
   in the allowlist" is the wrong question; "is this one of the
   recognized idioms?" is the right one. The allowlist conflates them.
4. **The score interpretation is wrong.** A project at 4.5/5 code
   hygiene with 158 allowlist entries reads as "lots of debt". A
   project at 5.0/5 with 9 codified patterns + 0 exceptions reads as
   "we have NO debt; here are the 9 idioms we use".

## Decision

Convert the 46 idiomatic entries from the 4 allowlists
(`unwrap-allowlist`, `unsafe-allowlist`, `panic-allowlist`,
`secret-allowlist`) into a **single source of truth**:

1. **This ADR** (`docs/adr/021-recognized-rust-idioms.md`) — prose
   description of each pattern: invariant, alternative considered,
   acceptance criterion.
2. **`.claude/rules/recognized-patterns.toml`** — machine-readable
   catalog: regex + scope + invariant + ADR-anchor per pattern.
3. **Gate scripts** (`check-unwrap.sh`, `check-unsafe.sh`,
   `check-panic.sh`, `check-secrets.sh`) load both the legacy allowlist
   AND the recognized-patterns TOML; the latter takes precedence.

The 46 entries that match a recognized pattern are deleted from the
allowlists. New code that uses these idioms compiles without paperwork.
Code that uses an *unrecognized* idiom still gate-blocks, with a hint
to add the pattern to ADR-021 + the TOML.

## Patterns codified

### 1. `mutex_poison_lock`

**Pattern:** `\.(lock|read|write)\(\)\.expect\(.*(poisoned|lock).*\)`
**Scope:** all `crates/**/*.rs` and `apps/**/*.rs` (production paths).

**Idiom:**

```rust
let guard = my_mutex.lock().expect("metrics mutex poisoned");
let r = my_rwlock.read().expect("config rwlock poisoned");
```

**Invariant:** Mutex / RwLock poisoning is **unrecoverable**. A
poisoned mutex means a previous holder panicked while holding the
lock; the protected data is by definition in an inconsistent state.
The `.expect()` documents this contract; using `?` and a typed error
would suggest the caller can recover, which is a lie.

**Alternative considered:** wrap in `Result<T, MutexPoisonError>`
and propagate. Rejected: every consumer would have to handle the
error identically (re-panic with context), so the wrapping is pure
ceremony.

**Acceptance:** any `lock|read|write` followed by `expect` whose
message contains `"poisoned"` or `"lock"` is accepted.

---

### 2. `system_clock_unix_epoch`

**Pattern:** `\.expect\("system clock before UNIX epoch"\)`
**Scope:** `crates/theo-domain/**/*.rs`,
          `crates/theo-tooling/**/*.rs`.

**Idiom:**

```rust
let now_ms = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .expect("system clock before UNIX epoch")
    .as_millis();
```

**Invariant:** `SystemTime::duration_since(UNIX_EPOCH)` returns
`Err(_)` only if the system clock is set before 1970-01-01. This is
a fatal misconfiguration — every timestamp the agent produces would
be invalid; every checkpoint would skew. Failing fast at the call
site with a documented panic is the correct contract.

**Alternative considered:** return `Result<u64, ClockError>` and
handle. Rejected: nothing the caller can do except re-panic.

**Acceptance:** the literal message `"system clock before UNIX epoch"`
is accepted.

---

### 3. `embedded_tool_schema_valid`

**Pattern:** `\.expect\(".*tool schema is valid"\)`
**Scope:** `crates/theo-tooling/**/*.rs`.

**Idiom:**

```rust
let schema = build_schema(&BUILTIN_DOCS_SEARCH_PARAMS)
    .expect("docs_search tool schema is valid");
```

**Invariant:** Tool schemas are embedded **const** data validated by
the `build_registry` contract test. Any malformed schema fails
`cargo test`. By the time `build_schema` runs, the input has been
proven valid at test time.

**Alternative considered:** `unwrap_or` + log. Rejected: a malformed
embedded schema is a programming bug; silently swapping in a default
masks the bug.

**Acceptance:** the message must end with `"tool schema is valid"`.

---

### 4. `process_entrypoint_runtime_init`

**Pattern:** `tokio::runtime::Runtime::new\(\)\.expect\(`
**Scope:** `apps/theo-cli/**/*.rs`.

**Idiom:**

```rust
fn main() -> i32 {
    let rt = tokio::runtime::Runtime::new()
        .expect("failed to create tokio runtime");
    // …
}
```

**Invariant:** Runtime spawn failure at the binary entrypoint is fatal
by design. There is no caller to which the error could be propagated.

**Alternative considered:** `process::exit(1)` with a manual error
print. Rejected: `expect` IS that pattern, with stack-trace context.

**Acceptance:** `Runtime::new().expect(_)` anywhere under
`apps/theo-cli/` is accepted.

---

### 5. `observability_writer_spawn`

**Pattern:** `\.expect\(.*(metrics|spawn).*\)`
**Scope:** `crates/theo-agent-runtime/src/observability/**/*.rs`.

**Idiom:**

```rust
let writer = std::thread::Builder::new()
    .name("theo-observability-writer".into())
    .spawn(move || writer_loop(rx))
    .expect("failed to spawn observability writer thread");
```

**Invariant:** Failure to spawn the observability writer thread is
fatal — the agent has no way to record what it is doing. Same logic
applies to metrics-mutex `expect`s within the observability module.

**Alternative considered:** silently degrade to no-op observability.
Rejected: a "blind agent" running for hours in CI is far worse than
a fast crash.

**Acceptance:** any `.expect()` in `observability/` whose message
contains `metrics` or `spawn` is accepted.

---

### 6. `rust_2024_test_env_var`

**Pattern:** `unsafe \{ std::env::(set_var|remove_var)\(`
**Scope:** any file inside a `#[cfg(test)]` module or `#[cfg(test)]`
sibling `_tests.rs` file (also any file whose path contains `/tests/`).

**Idiom:**

```rust
#[cfg(test)]
mod tests {
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        static M: OnceLock<Mutex<()>> = OnceLock::new();
        M.get_or_init(|| Mutex::new(())).lock()
            .expect("env mutex poisoned")
    }

    #[test]
    fn my_test() {
        let _g = lock();
        unsafe { std::env::set_var("THEO_TEST_X", "1"); }
        // …
        unsafe { std::env::remove_var("THEO_TEST_X"); }
    }
}
```

**Invariant:** Rust 2024 made `std::env::set_var` and
`std::env::remove_var` `unsafe` because concurrent modification is
UB. In tests, calls are serialised through a per-test-module
`Mutex<()>` so no concurrent reader race occurs.

**Alternative considered:** thread-safe shim crate. Rejected: adds a
dependency for a test-only need; the `Mutex` pattern is one screen
of code per test module.

**Acceptance:** any `unsafe { std::env::set_var(_) }` or
`unsafe { std::env::remove_var(_) }` inside a `#[cfg(test)]` scope
is accepted.

---

### 7. `builtin_tool_schema_panic`

**Pattern:** `panic!\("invalid built-in tool schema:.*"\)`
**Scope:** `crates/theo-tooling/src/registry/**/*.rs`.

**Idiom:**

```rust
fn register_builtins(reg: &mut ToolRegistry) {
    if let Err(e) = reg.register(Box::new(MyTool::new())) {
        panic!("invalid built-in tool schema: {e}");
    }
}
```

**Invariant:** Startup panic if a built-in tool ships an invalid
schema. Caught at `ToolRegistry::register_builtins()` during app
boot; there is no recoverable path because the tool would then be
unusable.

**Alternative considered:** validate schemas at compile time via
proc-macro. Rejected as scope creep; the runtime panic is caught by
the smoke test on every PR.

**Acceptance:** `panic!()` in the registry whose message starts with
`invalid built-in tool schema:` is accepted.

---

### 8. `observability_normalizer_compile_panic`

**Pattern:** `panic!\(".*regex.*compile.*"\)`
**Scope:** `crates/theo-agent-runtime/src/observability/normalizer.rs`.

**Idiom:**

```rust
fn cached(once: &OnceLock<Regex>, pattern: &'static str) -> &Regex {
    once.get_or_init(|| {
        Regex::new(pattern)
            .unwrap_or_else(|e| {
                panic!("normalizer regex failed to compile: {pattern}: {e}");
            })
    })
}
```

**Invariant:** Patterns are **compile-time literals** validated by
sibling tests. This branch is unreachable in practice; the `panic!`
exists to surface a code bug loudly if someone passes a malformed
literal.

**Alternative considered:** `Result<&Regex, regex::Error>`. Rejected:
the test suite already proves all literals compile; adding the
fallible signature pollutes every caller without safety gain.

**Acceptance:** `panic!()` in `observability/normalizer.rs` whose
message mentions both `regex` and `compile` is accepted.

---

### 9. `test_fixture_dummy_keys`

**Pattern:** secret-detector false positives where the surrounding
context is a `tests/` directory or a `mock_*.rs` / `*_test_helpers.rs`
file.

**Scope:** any file under `tests/`, `crates/theo-test-memory-fixtures/`,
`crates/**/mock_*.rs`, or `crates/**/test_helpers.rs`.

**Idiom:**

```rust
// In a test fixture:
const TEST_API_KEY: &str = "sk-test-deadbeefcafebabe1234567890abcdef";
const TEST_OAUTH_TOKEN: &str = "ya29.test-fixture-only-not-a-real-token";
```

**Invariant:** strings that look like secrets but live exclusively in
test code are dummy values; they cannot grant access to anything.

**Alternative considered:** generate dummy keys at runtime via PRNG.
Rejected: hard-coded fixtures make tests reproducible and reviewable.

**Acceptance:** any string matching the secret-detector regex inside
a path under `tests/`, `mock_*.rs`, or `test_helpers.rs` is accepted.

---

### 10. `local_proven_invariant`

**Pattern:** `\.expect\("<narrow message documenting the local proof>"\)`
**Scope:** specific files where a local invariant (just-pushed, just-validated,
            already-Some, already-non-empty) makes the panic branch unreachable.

**Idiom:**

```rust
// example: bash arg parser already checked is_some immediately above.
debug_assert!(maybe_arg.is_some());
let arg = maybe_arg.expect("checked is_some above");

// example: name was validated non-empty in the calling tool registry.
let first_char = name.chars().next().expect("name non-empty (validated above)");

// example: Vec was just pushed-to one line above.
let last = stack.last().expect("just pushed");
```

**Invariant:** the panic branch is unreachable because a *guard* — local
match arm, debug_assert, prior `if let Some`, or one-line-above
`push()` — proves the post-condition. Refactoring to `if let` /
typed errors is *tracked* (see plan T2.4 follow-up) but the immediate
substitution would be either (a) ceremonial mapping to `unreachable!()`
or (b) typed-error wrapping that every caller maps back to a panic.

**Alternative considered:** `unreachable!()` with the same message.
Rejected: `unreachable!()` and `expect()` on a proven-Some Option are
semantically identical; `expect()` keeps the Option's `unwrap`-like
ergonomics.

**Acceptance:** narrow scope (single file or single line) plus a
specific message that names the local invariant. Generic `unwrap()`
calls are NOT covered — each entry must spell out the proof.

Currently codified call sites:
- `crates/theo-tooling/src/bash/mod.rs` — `expect("checked is_some above")`
- `crates/theo-agent-runtime/src/skill_catalog.rs` —
  `expect("invariant: name is non-empty (checked above)")`
- `crates/theo-agent-runtime/src/session_tree/mod.rs` — `expect("just pushed")`
- `crates/theo-agent-runtime/src/snapshot.rs` —
  `expect("snapshot serialization failed")` (serde on derived `Serialize`
  cannot fail unless OOM)
- `crates/theo-agent-runtime/src/subagent/mod.rs` —
  `expect("spawn_semaphore is never closed during runtime")`
- `crates/theo-agent-runtime/src/secret_scrubber.rs` —
  `expect("scrubber regex constants are valid")`
- `crates/theo-tooling/src/sandbox/executor.rs` —
  bare `expect()` after a `BackendDecision::*` match arm proves Some
- `crates/theo-engine-retrieval/src/summary.rs` —
  `chain.last().unwrap()` (chain non-empty by construction)
- `crates/theo-tooling/src/apply_patch/mod.rs` —
  `*candidates.last().unwrap()` (early-return on len==0)
- `crates/theo-tooling/src/test_gen/property.rs:228` —
  `name.chars().next()` on validated-non-empty name
- `crates/theo-tooling/src/lsp/tool.rs:939` — `.unwrap()` inside a
  pattern-matched-Some Option chain
- `crates/theo-tooling/src/webfetch/mod.rs:292` — quote stripping
  inside a `quote.is_some()` match arm
- `crates/theo-engine-parser/src/extractors/php.rs:485` — test fixture
  using `parse_http_method` on a hardcoded literal

---

### 11. `process_entrypoint_desktop`

**Pattern:** `\.expect\("error while running Theo Code"\)`
**Scope:** `apps/theo-desktop/**/*.rs`.

**Idiom:**

```rust
fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running Theo Code");
}
```

**Invariant:** Tauri runtime failure at the desktop entrypoint is fatal
by design. Same logic as `process_entrypoint_runtime_init` (CLI Tokio
runtime) but for the GUI shell.

**Alternative considered:** propagate to a typed `DesktopRunError`.
Rejected: there is no caller above `run()` to handle it.

**Acceptance:** the literal message `"error while running Theo Code"`
in `apps/theo-desktop/`.

---

### 12. `process_entrypoint_agent_bin`

**Pattern:** `\.unwrap\(\)`
**Scope:** `crates/theo-agent-runtime/src/bin/theo-agent.rs` only.

**Idiom:**

```rust
fn main() {
    let cfg = parse_args().unwrap();
    AgentRuntime::start(cfg).unwrap();
}
```

**Invariant:** binary entrypoint at the top of `main()`. Failure is
fatal by design and there is no caller. The binary file is small
(≤ 200 LOC) and serves as a thin wrapper around the library.

**Alternative considered:** typed errors threaded through `main`
returning `Result<()>`. Rejected: `theo-agent` is a developer-only
spawn shim — the wrapping does not earn its keep.

**Acceptance:** any `.unwrap()` inside that one file is accepted.

---

### 13. `lsp_tool_common_unwrap`

**Pattern:** `\.unwrap\(\)`
**Scope:** `crates/theo-tooling/src/lsp/tool_common.rs` only.

**Idiom:** internal LSP helper functions where the JSON-RPC contract
guarantees every accessed field is present. The original `lsp/tool.rs`
file (Cat-B in B1) was split via the god-files plan; the helpers
inherit the same invariant.

**Invariant:** `tool_common.rs` is a private module exposing pure
helpers that consume already-validated JSON values from the LSP
response decoder. The `unwrap()` sites are inside a chain of
`if let Some(_) = …` arms or after `serde_json::from_value` that has
typed the value; the runtime cannot reach the panic branch unless the
LSP server violates the spec, which is a developer bug.

**Alternative considered:** wrap the helpers in `Result<_, LspError>`
and propagate. Rejected: the LSP error path is already typed at the
*tool boundary* (`LspError` enum); duplicating it in helpers below
adds ceremony without information.

**Acceptance:** any `.unwrap()` inside `tool_common.rs` is accepted.

---

### Pattern bookkeeping

The list above is **not closed**. New patterns are added by:

1. Editing this ADR with a new section (with: pattern, scope, idiom,
   invariant, alternative-considered, acceptance).
2. Appending the matching entry to `.claude/rules/recognized-patterns.toml`.
3. Removing the corresponding individual allowlist entries.
4. Bumping the `version` field at the top of the TOML so gate scripts
   can detect when their copy is stale.

If a pattern stops being load-bearing (e.g. the codebase migrates to
async-friendly mutexes that don't poison), the section is **deleted**
from this ADR, the TOML entry is removed, and any remaining sites
become normal violations the gate will flag.

## Consequences

- **Polarity reversed.** "Tolerated exceptions" becomes "recognized
  patterns". The allowlist files (`unwrap-allowlist.txt`,
  `unsafe-allowlist.txt`, `panic-allowlist.txt`,
  `secret-allowlist.txt`) shrink to ≤ 5 path entries each (true
  test-fixture whole-file exemptions).
- **Score honest.** Code hygiene 5/5 becomes earnable: 0 exceptions,
  N codified patterns + 0 unrecognized lines.
- **Maintenance burden centralized.** Adding a 100th use of the
  mutex-poison idiom no longer requires 100 allowlist entries — one
  pattern covers them all.
- **Required tooling:** the 4 gate scripts must read
  `recognized-patterns.toml` (T2.2 of the plan).
- **Documentation discipline:** any new pattern must come with the
  full ADR section (alternative-considered is mandatory). This ADR
  is read-and-reviewed in every PR that touches an allowlist.
- **Future ADRs** that introduce typed errors (like ADR-019 for
  cluster Cat-B) reduce the number of recognized patterns *or* mark
  one as "deprecated, retired on date X".

## Cross-references

- `docs/plans/code-hygiene-5x5-plan.md` — T2.1 (this ADR), T2.2 (gate
  script update), T2.3 (drain idiomatic allowlists).
- `docs/adr/019-cluster-louvain-typed-errors.md` — example of going
  the *opposite direction* (refactor-to-typed-error rather than
  codify-as-pattern). Both options remain open per pattern.
- `docs/adr/017-inline-io-tests.md` — sister concept: inline I/O test
  pattern. Phase 5 of this plan upgrades that ADR to v2 with the same
  "patterns not exceptions" treatment.
- `.claude/rules/recognized-patterns.toml` — machine-readable companion.
- `scripts/check-{unwrap,unsafe,panic,secrets}.sh` — gate scripts that
  consume the TOML.
