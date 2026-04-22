## Research for: PLAN_AUTO_EVOLUTION_SOTA.md — OpenDev Rust SOTA Patterns

**Date**: 2026-04-22
**Source**: `/home/paulo/Projetos/usetheo/theo-code/referencias/opendev/`
**OpenDev edition**: Rust 2024 (same as theo-code)

---

### Files Read

| File | Purpose |
|---|---|
| `crates/opendev-cli/src/runtime/background.rs` | BackgroundRuntime — shared-Arc fork pattern |
| `crates/opendev-agents/src/react_loop/mod.rs` | ReactLoop struct, iteration metrics |
| `crates/opendev-agents/src/react_loop/loop_state.rs` | LoopState counters, proactive reminders |
| `crates/opendev-agents/src/react_loop/config.rs` | ReactLoopConfig, nudge limits |
| `crates/opendev-agents/src/memory_consolidation.rs` | Autodream/dream consolidation — full implementation |
| `crates/opendev-tools-core/src/traits.rs` | BaseTool trait — name/description/schema/execute |
| `crates/opendev-tools-core/src/registry/mod.rs` | ToolRegistry — RwLock, dispatch, middleware |
| `crates/opendev-hooks/src/manager.rs` | HookManager — run_hooks_async (fire-and-forget) |
| `crates/opendev-hooks/src/models.rs` | HookEvent enum — SessionEnd, PostToolUse, UserPromptSubmit |
| `crates/opendev-cli/src/tui_runner/mod.rs` (lines 120–175) | AtomicUsize tool counter in BackgroundEventCallback |
| `crates/opendev-cli/src/runtime/mod.rs` (lines 665–691) | memory consolidation spawn at session start |

---

## Phase 1 — Nudge Counter + Memory Reviewer Background

### Pattern 1.1: AtomicUsize as turn/tool counter

**File**: `crates/opendev-cli/src/tui_runner/mod.rs`, lines 128–138

```rust
use std::sync::atomic::{AtomicUsize, Ordering};

struct BackgroundEventCallback {
    tx: mpsc::UnboundedSender<AppEvent>,
    task_id: String,
    tool_count: Arc<AtomicUsize>,
}

impl AgentEventCallback for BackgroundEventCallback {
    fn on_tool_started(&self, _tool_id: &str, tool_name: &str, _args: &HashMap<String, serde_json::Value>) {
        let count = self.tool_count.fetch_add(1, Ordering::Relaxed) + 1;
        // ... emit events
    }
}
```

**Direct applicability**: This is the exact pattern for `RunEngine::tool_calls_in_task` (Phase 1 Task 1.1 and Phase 3 Task 3.1). Use `Arc<AtomicUsize>` wrapped in the engine struct. `fetch_add(1, Ordering::Relaxed)` is correct here — Relaxed ordering is sufficient for monotonic counters where no synchronization dependency exists. The `+1` after fetch_add is the idiomatic way to get the post-increment value.

**Theo-code files**: `crates/theo-agent-runtime/src/run_engine.rs` — add `turns_since_memory_review: AtomicUsize` and `tool_calls_in_task: AtomicUsize` fields.

---

### Pattern 1.2: LoopState cross-iteration mutable state struct

**File**: `crates/opendev-agents/src/react_loop/loop_state.rs`, lines 22–63

```rust
pub(super) struct LoopState {
    pub iteration: usize,
    pub consecutive_no_tool_calls: usize,
    pub todo_nudge_count: usize,
    pub completion_nudge_sent: bool,
    pub bg_tasks_spawned: usize,
    pub bg_wait_nudge_count: usize,
    pub proactive_reminders: ProactiveReminderScheduler,
    // ...
}
```

**Direct applicability**: OpenDev bundles ALL cross-iteration counters into a single `LoopState` struct rather than scatter them as fields on the main engine. The plan's `RunEngine` fields (`turns_since_memory_review`, `tool_calls_in_task`, `skill_created_in_task`) should be grouped similarly — consider a `EvolutionCounters` sub-struct for cleanliness.

**Key observation**: `bg_tasks_spawned: usize` and `bg_wait_nudge_count: usize` are exactly the analog of our Phase 1 `turns_since_memory_review` and Phase 3 `tool_calls_in_task`. OpenDev resets these by reconstructing `LoopState::new()` at the start of each task.

**Theo-code files**: `crates/theo-agent-runtime/src/run_engine.rs`

---

### Pattern 1.3: ProactiveReminderScheduler — turn-interval based nudge

**File**: `crates/opendev-agents/src/react_loop/loop_state.rs`, lines 110–117

```rust
proactive_reminders: ProactiveReminderScheduler::new(vec![ProactiveReminderConfig {
    name: "task_proactive_reminder",
    turns_since_reset: 10,
    turns_between: 10,
    class: MessageClass::Nudge,
}]),
```

**Direct applicability**: OpenDev has a first-class scheduler for "fire nudge every N turns". The plan's `memory_review_nudge_interval` (default 10) mirrors `turns_between: 10`. Rather than implementing ad-hoc counter logic, theo-code can adopt the same `ProactiveReminderScheduler` pattern — a struct that tracks last-fired turn and answers `should_fire_now(current_turn) -> bool`.

**Rust adaptation**: The config struct is clean, serializable, and testable. No `async-trait` needed here — the scheduler is pure synchronous state checked at each turn boundary.

**Theo-code files**: `crates/theo-agent-runtime/src/memory_reviewer.rs` (new file from plan Task 1.3).

---

### Pattern 1.4: Fire-and-forget tokio::spawn in HookManager

**File**: `crates/opendev-hooks/src/manager.rs`, lines 157–183

```rust
pub fn run_hooks_async(
    &self,
    event: HookEvent,
    match_value: Option<String>,
    event_data: Option<Value>,
) where
    Self: Send + Sync + 'static,
{
    if !self.has_hooks_for(event) {
        return;
    }

    let config = self.config.clone();
    let session_id = self.session_id.clone();
    let cwd = self.cwd.clone();

    tokio::spawn(async move {
        let manager = HookManager::new(config, session_id, cwd);
        let _ = manager
            .run_hooks(event, match_value.as_deref(), event_data.as_ref())
            .await;
    });
}
```

**Direct applicability**: This is the canonical fire-and-forget pattern in opendev. Three key decisions visible here:
1. Clone only what the task needs before spawning (avoid moving self).
2. Ignore the `JoinHandle` (let it drop — the task runs to completion independently).
3. Discard errors at the call site with `let _ = ...` — errors are handled inside the spawned task.

The plan's `maybe_spawn_reviewer` (Task 1.4) should follow this exact structure. Note the `where Self: Send + Sync + 'static` bound — required when spawning methods, not standalone closures.

**Theo-code files**: `crates/theo-agent-runtime/src/memory_lifecycle.rs`

---

## Phase 2 — Autodream Background Daemon

### Pattern 2.1: memory_consolidation — complete autodream reference

**File**: `crates/opendev-agents/src/memory_consolidation.rs`, full file

This is the most directly applicable reference in the entire opendev codebase. It implements the exact autodream pattern.

**Struct**:
```rust
pub struct ConsolidationReport {
    pub files_consolidated: usize,
    pub files_pruned: usize,
    pub files_backed_up: usize,
}

pub struct ConsolidationMeta {
    pub last_run: Option<String>,   // ISO 8601
    pub files_processed: usize,
}
```

**Guard function** (lines 43–84):
```rust
pub fn should_consolidate(working_dir: &Path) -> bool {
    // 1. Check memory dir exists
    // 2. Check lock file absent (prevents concurrent runs)
    // 3. Check time since last run >= 24h
    // 4. Count session files >= MIN_SESSION_FILES (5)
    // Returns false early on any guard failing
}
```

**Spawn at session start** (`crates/opendev-cli/src/runtime/mod.rs`, lines 673–691):
```rust
if opendev_agents::memory_consolidation::should_consolidate(&wd) {
    tokio::spawn(async move {
        tracing::info!("Starting background memory consolidation");
        match opendev_agents::memory_consolidation::consolidate(&wd).await {
            Some(report) => tracing::info!(
                consolidated = report.files_consolidated,
                pruned = report.files_pruned,
                "Memory consolidation complete"
            ),
            None => tracing::debug!("Memory consolidation skipped"),
        }
    });
}
```

**Direct applicability**: The plan's `AutodreamExecutor::consolidate` (Task 2.1) is a trait-ified version of this pattern. Key differences to apply:
- OpenDev uses `should_consolidate()` (pure function) to guard the spawn — replicate this as `cfg.autodream_enabled && executor.should_run()`.
- OpenDev uses a file-based **lock file** to prevent concurrent consolidation. Theo-code should do the same (`~/.local/share/theo/autodream.lock`).
- OpenDev triggers at **session start** (not session end). The plan triggers at `on_session_end`. Both are valid — OpenDev's choice avoids blocking shutdown.
- The `ConsolidationReport` struct maps 1:1 to the plan's struct.
- Note: OpenDev does NOT use `tokio::time::timeout` on the spawn. The plan adds a 60s timeout — this is a theo-code improvement over opendev's pattern.

**Important**: OpenDev uses frontmatter `type: session` to identify memory files. Theo-code should use the same convention in its `MemoryEntry` structs.

**Theo-code files**: `crates/theo-agent-runtime/src/autodream.rs` (new), `crates/theo-agent-runtime/src/memory_lifecycle.rs`

---

### Pattern 2.2: Atomic backup before mutation

**File**: `crates/opendev-agents/src/memory_consolidation.rs`, lines 137–149, 413–417

```rust
// Backup before modifying
for file in &session_files {
    let dest = backup_dir.join(&file.filename);
    if let Err(e) = std::fs::copy(&file.path, &dest) {
        warn!("Failed to backup {}: {e}", file.filename);
    }
}

// Atomic write via tmp + rename
std::fs::write(&tmp_path, final_content)?;
std::fs::rename(&tmp_path, &index_path)?;
```

**Direct applicability**: The plan mentions "Tantivy persistence corrompe em crash" as a risk. OpenDev mitigates via tmp+rename for atomic writes. Theo-code should adopt this for any file that autodream/memory-reviewer writes. The tmp file approach ensures no partial writes are visible.

**Theo-code files**: `crates/theo-agent-runtime/src/autodream.rs`, `crates/theo-infra-memory/src/` (any write path)

---

## Phase 3 — Skill Generator + skill_manage Tool

### Pattern 3.1: BaseTool trait — complete interface

**File**: `crates/opendev-tools-core/src/traits.rs`, lines 414–576

```rust
#[async_trait::async_trait]
pub trait BaseTool: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameter_schema(&self) -> serde_json::Value;

    async fn execute(
        &self,
        args: HashMap<String, serde_json::Value>,
        ctx: &ToolContext,
    ) -> ToolResult;

    // Classification (all have defaults — only override what differs)
    fn is_read_only(&self, _args: &HashMap<String, serde_json::Value>) -> bool { false }
    fn is_destructive(&self, _args: &HashMap<String, serde_json::Value>) -> bool { false }
    fn category(&self) -> ToolCategory { ToolCategory::Other }
    fn skip_dedup(&self) -> bool { false }
    fn is_enabled(&self) -> bool { true }
    fn interrupt_behavior(&self) -> InterruptBehavior { InterruptBehavior::default() }

    // System prompt contribution
    fn prompt_contribution(&self) -> Option<String> { None }
}
```

**ToolResult** (lines 111–182):
```rust
pub struct ToolResult {
    pub success: bool,
    pub output: Option<String>,
    pub error: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>,
    pub duration_ms: Option<u64>,
    pub llm_suffix: Option<String>,   // Hidden context for LLM, not shown in UI
}

impl ToolResult {
    pub fn ok(output: impl Into<String>) -> Self { ... }
    pub fn fail(error: impl Into<String>) -> Self { ... }
    pub fn with_llm_suffix(mut self, suffix: impl Into<String>) -> Self { ... }
}
```

**Direct applicability**: The plan's `skill_manage` tool (Task 3.3) and `memory_search` tool (Task 4.4) should implement theo-code's own `Tool` trait (which currently differs from OpenDev's `BaseTool`). Key patterns to adopt from opendev:
- `llm_suffix` field on `ToolResult` — silent guidance to LLM on errors without showing it in UI. Extremely useful for skill validation errors.
- `prompt_contribution() -> Option<String>` — allows `SkillManageTool` to inject its auto-improvement reminder directly without modifying `system.rs`. This is cleaner than the plan's Task 3.7 approach.
- `is_enabled()` method — allows feature-flagging tools at runtime without removing from registry.

**Theo-code files**: `crates/theo-tooling/src/skill_manage/mod.rs`, `crates/theo-tooling/src/memory_search/mod.rs`

---

### Pattern 3.2: ToolRegistry with RwLock — interior mutability

**File**: `crates/opendev-tools-core/src/registry/mod.rs`, lines 27–44, 112–116

```rust
pub struct ToolRegistry {
    pub(super) tools: RwLock<HashMap<String, Arc<dyn BaseTool>>>,
    pub(super) middleware: RwLock<Vec<Arc<dyn ToolMiddleware>>>,
    pub(super) tool_timeouts: RwLock<HashMap<String, ToolTimeoutConfig>>,
    pub(super) dedup_cache: Mutex<HashMap<String, ToolResult>>,
    core_tools: RwLock<HashSet<String>>,
}

pub fn register(&self, tool: Arc<dyn BaseTool>) {
    let name = tool.name().to_string();
    let mut tools = self.tools.write().expect("ToolRegistry lock poisoned");
    tools.insert(name, tool);
}
```

**Direct applicability**: The `RwLock` pattern (readers concurrent, writer exclusive) enables `register()` via `&self` — critical for late registration (e.g., adding `skill_manage` tool after `Arc<ToolRegistry>` is already shared). The `dedup_cache: Mutex` is separate from `tools: RwLock` because the dedup cache is cleared per-turn (high write frequency). This split is worth copying in theo-code's tooling layer.

**Theo-code files**: `crates/theo-tooling/src/` — when registering `skill_manage` and `memory_search` in `create_default_registry`.

---

### Pattern 3.3: HookEvent enum — lifecycle points for event-driven dispatch

**File**: `crates/opendev-hooks/src/models.rs`, lines 9–31

```rust
pub enum HookEvent {
    SessionStart,
    UserPromptSubmit,   // <-- Phase 5 auto-improvement reminder hook
    PreToolUse,
    PostToolUse,        // <-- Phase 3 ToolExecuted counter
    PostToolUseFailure,
    SubagentStart,
    SubagentStop,
    Stop,
    PreCompact,
    SessionEnd,         // <-- Phase 2 autodream spawn
}
```

**Direct applicability**: Theo-code's `EventBus` already has `ToolExecuted` and session lifecycle events. The opendev `HookEvent` enum confirms the minimal set needed. Key mappings:
- `PostToolUse` → `EventType::ToolExecuted` (already exists in theo-code)
- `SessionEnd` → `on_session_end()` hook (already exists in `memory_lifecycle.rs`)
- `UserPromptSubmit` → Phase 5 `UserPromptSubmit` hook (to be added)

The important pattern: opendev gates fire-and-forget spawns with `has_hooks_for(event)` check BEFORE doing any async work. Theo-code should mirror: check `cfg.autodream_enabled` before spawning, check `cfg.memory_review_nudge_interval > 0` before spawn — same early return.

---

## Phase 4 — Tantivy Persistent Index

### Pattern 4.1: OnceLock for single-initialization resources

**File**: `crates/opendev-web/src/routes/auth.rs`, line 62

```rust
static KEY: std::sync::OnceLock<&'static [u8]> = std::sync::OnceLock::new();
```

**Direct applicability**: For `MemoryTantivyIndex::open_or_create`, use `OnceLock` if the index needs to be a process-level singleton (e.g., only one index per data directory). The pattern: `static INDEX: OnceLock<Arc<Mutex<MemoryTantivyIndex>>> = OnceLock::new()`. However, if the index is injected as a config field (plan Task 4.5 `transcript_index: Option<TranscriptIndexHandle>`), `OnceLock` is not needed — initialization happens at application startup.

**Theo-code files**: `crates/theo-engine-retrieval/src/memory_tantivy.rs`

---

### Pattern 4.2: BackgroundRuntime — shared-Arc fork for spawned tasks

**File**: `crates/opendev-cli/src/runtime/background.rs`, lines 28–44, 141–176

```rust
pub struct BackgroundRuntime {
    // Shared (Arc clone from parent)
    tool_registry: Arc<ToolRegistry>,
    http_client: Arc<opendev_http::adapted_client::AdaptedClient>,

    // Owned (fresh per background task)
    config: AppConfig,
    session_manager: SessionManager,
    react_loop: ReactLoop,
    cost_tracker: Mutex<CostTracker>,
}

impl AgentRuntime {
    pub fn create_background_runtime(&self, session_manager: SessionManager)
        -> Result<BackgroundRuntime, String>
    {
        Ok(BackgroundRuntime {
            tool_registry: Arc::clone(&self.tool_registry),  // shared
            http_client: Arc::clone(&self.http_client),      // shared
            config: self.config.clone(),                      // owned copy
            react_loop: ReactLoop::new(ReactLoopConfig::default()), // fresh
            cost_tracker: Mutex::new(CostTracker::new()),    // fresh
            // ...
        })
    }
}
```

**Direct applicability**: For Phase 2 `LlmAutodreamExecutor` and Phase 3 `LlmMemoryReviewer` — both need an LLM provider. Instead of cloning the full `RunEngine`, inject only the `Arc<dyn LlmProvider>` (shared) plus fresh `SessionManager` (owned). This is precisely what `BackgroundRuntime::create_background_runtime` does: shared = Arc-wrapped expensive resources, owned = cheap per-task state. Apply this to `AutodreamHandle` and `MemoryReviewerHandle` in `AgentConfig`.

**Theo-code files**: `crates/theo-agent-runtime/src/autodream.rs`, `crates/theo-agent-runtime/src/memory_reviewer.rs`

---

## Phase 5 — Onboarding + Auto-improvement

### Pattern 5.1: should_consolidate() — pure function first-run detection

**File**: `crates/opendev-agents/src/memory_consolidation.rs`, lines 43–84

```rust
pub fn should_consolidate(working_dir: &Path) -> bool {
    let memory_dir = ...;
    if !memory_dir.exists() { return false; }
    let lock_path = ...;
    if lock_path.exists() { return false; }    // concurrent guard
    // check last_run timestamp
    // count session files
    true
}
```

**Direct applicability**: The plan's `needs_bootstrap(memory_dir: &Path) -> bool` (Task 5.1) follows this exact pattern — a pure, synchronous function that reads the filesystem and returns a bool. No async, no side effects, easy to test. Mapping:
- OpenDev checks `lock_path.exists()` → theo-code checks `user_path.exists()`
- OpenDev checks `session_count >= MIN_SESSION_FILES` → theo-code checks `content.len() >= 50`
- Both are called at session startup before the first iteration

**Theo-code files**: `crates/theo-agent-runtime/src/onboarding.rs` (new)

---

### Pattern 5.2: prompt_contribution() — tool-injected system prompt text

**File**: `crates/opendev-tools-core/src/traits.rs`, lines 565–576

```rust
/// Optional text this tool contributes to the system prompt.
///
/// E.g., `Bash` might add shell environment info, or `Memory` might
/// add instructions about memory file format.
fn prompt_contribution(&self) -> Option<String> {
    None
}
```

**Direct applicability**: This is the correct way to implement Phase 3 Task 3.7 (auto-improvement instruction) and Phase 5 Task 5.5 (periodic reminder). Instead of modifying `system.rs` directly, `SkillManageTool::prompt_contribution()` returns the auto-improvement instruction. The prompt composer collects contributions from all registered tools and assembles the final system prompt. This follows OCP — adding a new instruction doesn't require editing the central `system.rs`.

**Theo-code files**: `crates/theo-tooling/src/skill_manage/mod.rs`, `crates/theo-agent-runtime/src/prompts/system.rs` (adds collection loop)

---

## Delta Analysis

| Aspect | Current (theo-code) | OpenDev SOTA | Gap |
|---|---|---|---|
| Turn counter | Not present | `LoopState.iteration: usize` + named counters | Add `turns_since_memory_review: AtomicUsize` to RunEngine |
| Tool-call counter | Not present | `Arc<AtomicUsize>` in `BackgroundEventCallback` | Add `tool_calls_in_task: AtomicUsize` to RunEngine |
| Fire-and-forget background | Not present | `HookManager::run_hooks_async` with `tokio::spawn` | Add `maybe_spawn_reviewer` in `memory_lifecycle.rs` |
| Memory consolidation | Not present | Full `memory_consolidation.rs` module (250 LOC) | Implement `LlmAutodreamExecutor` using opendev as reference |
| Lock file guard | Not present | `consolidation.lock` prevents concurrent runs | Add lock file before autodream spawn |
| Tool trait | `Tool` with `schema()/category()/execute()` | `BaseTool` adds `prompt_contribution()`, `is_enabled()`, `llm_suffix` | Add `prompt_contribution()` to theo-code `Tool` trait |
| Tool registry | Present | `RwLock`-based with `mark_as_core()` deferral | Register `skill_manage` + `memory_search` via existing pattern |
| Hook event system | `EventBus` + `DomainEvent` | `HookEvent` enum + `HookManager` | Map `ToolExecuted` to counter increment, `SessionEnd` to autodream |
| First-run detection | Not present | `should_consolidate()` pure fn pattern | Implement `needs_bootstrap()` following same pattern |
| Prompt injection | Central `system.rs` | `prompt_contribution()` per-tool | Add collection loop in `system.rs`; `SkillManageTool` provides its own text |
| Background task isolation | Not present | `BackgroundRuntime` (shared Arc + owned state) | Inject `Arc<LlmProvider>` into `AutodreamHandle` and `MemoryReviewerHandle` |
| Atomic memory writes | Not present explicitly | `tmp_path` + `rename` for crash safety | Apply to autodream output writes |

---

## Adaptation Notes

- `BaseTool::execute(args: HashMap<String, Value>, ctx: &ToolContext)` → theo-code uses `execute(args: Value, ctx: &ToolContext)`. The serde_json `Value::Object` unwrap is equivalent. No change needed.
- `#[async_trait::async_trait]` → theo-code already uses this crate. Same import path.
- `tracing::info!(?report, "autodream completed")` → structured logging with `?` debug formatter. Correct pattern for `ConsolidationReport` in theo-code.
- `Arc::clone(&self.tool_registry)` → Arc clone is zero-cost at runtime (increments ref count). Use for LLM provider sharing in background tasks.
- `Ordering::Relaxed` for `AtomicUsize::fetch_add` → correct for counters where the value itself is the only shared state and no memory ordering dependency exists (no "if count == N, then read from X" pattern).
- OpenDev's `memory_consolidation.rs` uses `chrono` for timestamps. Theo-code uses `std::time::SystemTime` in places. Standardize on `chrono` if adding timestamps to `ConsolidationMeta`.

---

## Implementation Plan (phase order preserved)

### Phase 1 (150 LOC)
1. `crates/theo-agent-runtime/src/run_engine.rs` — Add `turns_since_memory_review: AtomicUsize` (from Pattern 1.1). Follow opendev's `LoopState` grouping: consider `EvolutionCounters` sub-struct.
2. `crates/theo-agent-runtime/src/memory_lifecycle.rs` — Implement `maybe_spawn_reviewer` following Pattern 1.4 (HookManager::run_hooks_async structure).
3. `crates/theo-agent-runtime/src/memory_reviewer.rs` (new) — `MemoryReviewer` trait + `NullMemoryReviewer`. Use Pattern 1.3's scheduler config for `memory_review_nudge_interval`.

### Phase 2 (200 LOC)
4. `crates/theo-agent-runtime/src/autodream.rs` (new) — Direct port of `memory_consolidation.rs` (Pattern 2.1) as a trait-ified version. Add `tokio::time::timeout` wrapper (improvement over opendev). Apply Pattern 2.2 (atomic writes).
5. `crates/theo-agent-runtime/src/memory_lifecycle.rs` — Add lock file guard before spawning (Pattern 2.1 lines 97–99).

### Phase 3 (300 LOC)
6. `crates/theo-tooling/src/skill_manage/mod.rs` (new) — Implement `Tool` trait following Pattern 3.1 (`BaseTool`). Add `prompt_contribution()` for auto-improvement text (Pattern 5.2).
7. `crates/theo-agent-runtime/src/run_engine.rs` — Add `tool_calls_in_task: AtomicUsize` + `skill_created_in_task: AtomicBool` (Pattern 1.1).
8. `crates/theo-agent-runtime/src/memory_lifecycle.rs` — Subscribe to `ToolExecuted` events, increment counter, spawn reviewer at 5 (Pattern 1.4).

### Phase 4 (400 LOC)
9. `crates/theo-engine-retrieval/src/memory_tantivy.rs` — Change `Index::create_in_ram` to `MmapDirectory::open` (plan Task 4.1). Use `OnceLock` if needed (Pattern 4.1).
10. `crates/theo-tooling/src/memory_search/mod.rs` (new) — Implement `Tool` trait (Pattern 3.1). 3-tier dispatch in `execute()`.

### Phase 5 (170 LOC)
11. `crates/theo-agent-runtime/src/onboarding.rs` (new) — `needs_bootstrap()` as pure fn (Pattern 5.1). Exact structure mirrors `should_consolidate()`.
12. `crates/theo-agent-runtime/src/prompts/system.rs` — Add collection loop: iterate registered tools, call `prompt_contribution()`, append non-None results (Pattern 5.2).

---

## Key Imports Used in OpenDev (for reference)

```toml
# From opendev Cargo.toml — relevant to our phases
async-trait = "0.1"
tokio = { version = "1", features = ["full"] }
tokio-util = { version = "0.7", features = ["sync"] }   # CancellationToken
tracing = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
chrono = { version = "0.4", features = ["serde"] }
```

All of these are already present in theo-code's workspace `Cargo.toml`.
