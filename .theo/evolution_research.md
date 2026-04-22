# Evolution Research — Auto-Evolution SOTA (Phase II)

> **Source of truth**: `docs/plans/PLAN_AUTO_EVOLUTION_SOTA.md`
> **Started**: 2026-04-22 (evolution/apr22-1618 branch)
> **Scope**: 5 phases, ~1340 src LOC + ~900 test LOC. Nudge counter → autodream → skill generator → Tantivy → onboarding.
> **Primary references**: `referencias/hermes-agent/` (Python), `referencias/opendev/` (Rust — closest to our stack).

---

## SOTA Pattern Catalog (evidence-grounded)

### Pattern 1 — Nudge Counter (Hermes)

**Source**: `referencias/hermes-agent/run_agent.py:1418-1421, 8747-8753, 11846-11852`

```python
self._memory_nudge_interval = 10   # configurable via config.yaml
self._turns_since_memory = 0
self._skill_nudge_interval = 10
self._iters_since_skill = 0

# In run_conversation() — turn start:
if (self._memory_nudge_interval > 0
        and "memory" in self.valid_tool_names
        and self._memory_store):
    self._turns_since_memory += 1
    if self._turns_since_memory >= self._memory_nudge_interval:
        _should_review_memory = True
        self._turns_since_memory = 0

# Skill nudge — checked AFTER loop completes, based on tool iterations of THIS turn:
if (self._skill_nudge_interval > 0
        and self._iters_since_skill >= self._skill_nudge_interval
        and "skill_manage" in self.valid_tool_names):
    _should_review_skills = True
    self._iters_since_skill = 0
```

**Key design decisions:**
- **Memory nudge** = turn-based (counts user messages).
- **Skill nudge** = tool-iteration-based (counts tool calls per task).
- **NOT reset per `run_conversation`** — counters persist across calls. Comment in source: *"must persist across run_conversation calls so that nudge logic accumulates correctly in CLI mode"*.
- `interval = 0` disables the mechanism entirely.
- Guards on `tool_name in valid_tool_names` — never spawn if the memory/skill tool isn't registered.

**Rust translation**:
- `AtomicUsize` for both counters on `RunEngine`.
- `fetch_add(1, Relaxed)` + conditional `store(0, Relaxed)` after spawn.
- Config fields on `AgentConfig`: `memory_review_nudge_interval: usize` (default 10), `skill_review_nudge_interval: usize` (default 10). `0` disables.
- Check `cfg.memory_provider.is_some()` + `cfg.memory_review_nudge_interval > 0`.
- **Hermes Issue #8506**: counter resets in gateway mode because fresh `AIAgent` per message loses state → mitigated in Rust by `AtomicUsize` on shared `RunEngine` reused across turns.

---

### Pattern 2 — Background Review Spawn (Hermes)

**Source**: `referencias/hermes-agent/run_agent.py:2780-2879`

```python
def _spawn_background_review(self, messages_snapshot, review_memory=False, review_skills=False):
    """Fire-and-forget thread. Forks an AIAgent with the same model/tools/context.
    Writes directly to shared stores. Never modifies main history."""
    prompt = self._COMBINED_REVIEW_PROMPT if (review_memory and review_skills) \
        else self._MEMORY_REVIEW_PROMPT if review_memory \
        else self._SKILL_REVIEW_PROMPT

    def _run_review():
        review_agent = AIAgent(model=self.model, max_iterations=8, quiet_mode=True, ...)
        review_agent._memory_store = self._memory_store
        review_agent._memory_nudge_interval = 0   # prevent recursion
        review_agent._skill_nudge_interval = 0
        try:
            review_agent.run_conversation(user_message=prompt, conversation_history=messages_snapshot)
        finally:
            review_agent.close()

    threading.Thread(target=_run_review, daemon=True, name="bg-review").start()
```

**Prompts** (`run_agent.py:2745-2778`):

```
_MEMORY_REVIEW_PROMPT:
"Review the conversation above and consider saving to memory if appropriate.
Focus on:
1. Has the user revealed things about themselves — persona, desires, preferences?
2. Has the user expressed expectations about how you should behave, their work style?
If something stands out, save it using the memory tool.
If nothing is worth saving, just say 'Nothing to save.' and stop."

_SKILL_REVIEW_PROMPT:
"Review the conversation above and consider saving or updating a skill if appropriate.
Focus on: was a non-trivial approach used that required trial and error, or changing course
due to experiential findings, or did the user expect a different method/outcome?
If a relevant skill already exists, update it. Otherwise, create a new skill if reusable.
If nothing is worth saving, just say 'Nothing to save.' and stop."
```

**Key design decisions:**
- Fire-and-forget (daemon thread, no join).
- Forked agent has nudge intervals **zeroed** → prevents recursive reviewer-spawning-reviewer.
- Shares `_memory_store` (same files on disk) but has its own `max_iterations=8` budget.
- Silent (`quiet_mode=True` + stdout/stderr redirect).

**Rust translation**:
- `tokio::spawn` with owned clones (prompt, conversation snapshot, `Arc<dyn MemoryProvider>`).
- `tokio::time::timeout` wrapper (60s default) to bound runtime.
- Trait `MemoryReviewer` + `SkillReviewer` with `async fn review(&self, messages: Vec<Message>) -> Result<usize, ReviewError>`.
- Errors logged via `tracing::warn!`, never propagated to main loop.

---

### Pattern 3 — Memory Consolidation / Autodream (OpenDev — Rust!)

**Source**: `referencias/opendev/crates/opendev-agents/src/memory_consolidation.rs:1-80, 44-95`

```rust
//! Periodically merges session notes and stale memories into durable
//! topic files. Triggered at session start when:
//! - At least 24 hours since last consolidation
//! - At least 5 `type: session` memory files exist
//! - No concurrent consolidation (lock file guard)

const MIN_SESSION_FILES: usize = 5;
const MAX_FILES_PER_RUN: usize = 20;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ConsolidationMeta {
    pub last_run: Option<String>,
    pub files_processed: usize,
}

#[derive(Debug)]
pub struct ConsolidationReport {
    pub files_consolidated: usize,
    pub files_pruned: usize,
    pub files_backed_up: usize,
}

pub fn should_consolidate(working_dir: &Path) -> bool {
    let memory_dir = paths.project_memory_dir();
    if !memory_dir.exists() { return false; }
    if paths.consolidation_lock_path().exists() { return false; }

    let meta = load_meta(&paths.consolidation_meta_path());
    if let Some(ref last_run) = meta.last_run
        && let Ok(last_time) = chrono::DateTime::parse_from_rfc3339(last_run) {
        if chrono::Utc::now().signed_duration_since(last_time) < chrono::Duration::hours(24) {
            return false;
        }
    }
    count_session_files(&memory_dir) >= MIN_SESSION_FILES
}
```

**Key design decisions:**
- **Triggered at session START**, not end (avoids slowing down shutdown; session N's data consolidated at session N+1's boot).
- Minimum 24h cadence — prevents thrashing.
- **Lock file guard** (`.consolidation.lock`) — mutex across process boundaries.
- **Backup before mutation** (`.bak` files counted in report).
- **Never touches `user` or `reference` type memories** — they're "atomic and personal".
- Iteration cap: `MAX_FILES_PER_RUN = 20` per invocation.

**Adaptation for our Phase 2**:
- Our plan says trigger on `on_session_end` but OpenDev proves session-START is better. **Revise**: trigger check at startup; if should_consolidate, spawn async.
- Lock: `std::fs::OpenOptions::new().create_new(true).open(".consolidation.lock")`; delete on drop.
- Staleness: port "type: session" → our `EpisodeSummary.memory_kind == Episodic`.
- Guard staleness by our existing `MemoryLifecycle::Active | Cooling | Archived` decay.

---

### Pattern 4 — skill_manage Tool Operations (Hermes)

**Source**: `referencias/hermes-agent/tools/skill_manager_tool.py:14-21, 56-74, 304-510`

```
Operations exposed:
  create      -- Create new skill (SKILL.md + dir structure)
  edit        -- Replace SKILL.md content (full rewrite)
  patch       -- Targeted find-and-replace within SKILL.md OR supporting file
  delete      -- Remove skill entirely
  write_file  -- Create supporting file in allowed subdirs
  remove_file -- Delete supporting file
```

Constants:
```python
MAX_NAME_LENGTH = 64
MAX_DESCRIPTION_LENGTH = 1024
MAX_SKILL_CONTENT_CHARS = 100_000   # ~36k tokens at 2.75 chars/token
MAX_SKILL_FILE_BYTES = 1_048_576    # 1 MiB
VALID_NAME_RE = re.compile(r'^[a-z0-9][a-z0-9._-]*$')
ALLOWED_SUBDIRS = {"references", "templates", "scripts", "assets"}
```

Safety integration (line 56-74):
```python
def _security_scan_skill(skill_dir: Path) -> Optional[str]:
    result = scan_skill(skill_dir, source="agent-created")
    allowed, reason = should_allow_install(result)
    if allowed is False:
        return f"Security scan blocked this skill ({reason})"
    if allowed is None:  # "ask" verdict — for agent-created, upgrade to BLOCK
        return f"Security scan blocked this skill ({reason})"
    return None
```

**Policy by origin:**
- `source="agent-created"` — "ask" → BLOCK (agent-generated with dangerous patterns never auto-allowed).
- `source="community"` — same patterns → BLOCK.
- `source="user"` — trusted, scan warns only.

**Rust translation**:
- New tool `SkillManageTool` in `crates/theo-tooling/src/skill_manage/mod.rs`.
- 5-operation enum dispatched on `args["operation"]`.
- Reuse `theo_infra_memory::security::scan()` + new `scan_skill_body()` adding destructive/exfil/persistence patterns.
- Frontmatter field `origin: agent | community | user` parsed by existing `skill_catalog.rs` YAML parser.

---

### Pattern 5 — Threat Patterns for skill_guard (Hermes — ~80 regexes)

**Source**: `referencias/hermes-agent/tools/skills_guard.py:82-250+`

Categories with examples:

| Category | Pattern | Severity |
|---|---|---|
| env_exfil_curl | `curl\s+[^\n]*\$\{?\w*(KEY\|TOKEN\|SECRET\|PASSWORD\|API)` | critical |
| env_exfil_wget | same with `wget` | critical |
| ssh_dir_access | `\$HOME/\.ssh\|\~/\.ssh` | high |
| aws_dir_access | `\$HOME/\.aws\|\~/\.aws` | high |
| hermes_env_access | `\$HOME/\.hermes/\.env` | critical |
| read_secrets_file | `cat\s+.*(\.env\|credentials\|\.netrc\|\.pgpass)` | critical |
| dns_exfil | `\b(dig\|nslookup\|host)\s+.*\$` | critical |
| prompt_injection_ignore | `ignore\s+.*(previous\|all\|above)\s+instructions` | critical |
| role_hijack | `you\s+are\s+.*now\s+` | high |
| deception_hide | `do\s+not\s+.*tell\s+.*the\s+user` | critical |
| sys_prompt_override | `system\s+prompt\s+override` | critical |
| bypass_restrictions | `act\s+as\s+(if\|though)\s+.*have\s+no\s+.*(restrictions\|limits)` | critical |
| translate_execute | `translate\s+.*\s+and\s+(execute\|run\|eval)` | critical |
| html_comment_injection | `<!--[^>]*(ignore\|override\|system)[^>]*-->` | high |
| destructive_root_rm | `rm\s+-rf\s+/` | critical |
| destructive_home_rm | `rm\s+.*r.*\$HOME` | critical |
| system_overwrite | `>\s*/etc/` | critical |
| format_filesystem | `\bmkfs\b` | critical |
| disk_overwrite | `\bdd\s+.*if=.*of=/dev/` | critical |

**Rust translation**:
- `once_cell::Lazy<Vec<ThreatPattern>>` with pre-compiled `regex::Regex`.
- Struct: `ThreatPattern { regex, id, severity, category, description }`.
- `scan_skill_body(body: &str) -> Result<(), Vec<Finding>>` reuses existing `scan()` + appends destructive/exfil/persistence patterns.
- Severity enum: `Critical | High | Medium | Low`. Any critical → BLOCK; any high → ASK; medium/low → WARN.

---

### Pattern 6 — Memory Size Limits (Hermes `MemoryStore`)

**Source**: `referencias/hermes-agent/tools/memory_tool.py:116-120`

```python
def __init__(self, memory_char_limit: int = 2200, user_char_limit: int = 1375):
    self.memory_char_limit = memory_char_limit    # MEMORY.md cap
    self.user_char_limit = user_char_limit         # USER.md cap
```

**Our current system**: `memory_budget_fraction: 0.15` of context window (percentage-based). More adaptive but less predictable.

**Decision**: keep percentage as default. Add optional hard caps `user_md_max_chars` + `memory_md_max_chars` (disabled by default).

---

### Pattern 7 — Tantivy Persistent Index

**Source**: `crates/theo-engine-retrieval/src/memory_tantivy.rs:1-80` (our current state — RAM only).

Migration:
```rust
pub fn open_or_create(index_dir: &Path) -> Result<Self, tantivy::TantivyError> {
    std::fs::create_dir_all(index_dir)?;
    let dir = MmapDirectory::open(index_dir)?;
    let schema = Self::build_schema();
    let index = Index::open_or_create(dir, schema)?;
    // ...tokenizer setup unchanged...
}
```

New schema fields for transcripts: `session_id` (STORED+STRING), `turn_index` (STORED+U64), `timestamp_unix` (STORED+U64).

**3-tier expansion** from memsearch pattern:
```
Tier 1: memory_search     → BM25 query, top-K (slug + snippet + score)
Tier 2: memory_get        → expand chunk hash → full markdown section
Tier 3: memory_transcript → full session transcript
```

---

### Pattern 8 — BOOTSTRAP Q&A (OpenClaw)

Flow:
1. On session start, check `USER.md` existence + content.
2. If empty/absent, prepend `BOOTSTRAP_PROMPT` to system message.
3. Agent asks ONE question at a time, waits.
4. After answers, write to `USER.md` via `memory_tool`.
5. Delete `BOOTSTRAP.md` marker (runs once).

**Rust**: `onboarding::needs_bootstrap(&memory_dir) -> bool` + conditional prompt injection.

---

## Architecture Fit with theo-code

| Theo-code module | SOTA pattern applied | LOC estimate |
|---|---|---|
| `run_engine.rs` | Atomic counters (mem + skill turns) | +20 |
| `config.rs` | 5 new config fields | +15 |
| `memory_reviewer.rs` (NEW) | Trait + LlmMemoryReviewer | ~120 |
| `skill_reviewer.rs` (NEW) | Trait + LlmSkillReviewer | ~120 |
| `autodream.rs` (NEW) | ConsolidationMeta + lock + backup + 24h gate | ~200 |
| `memory_lifecycle.rs` | `maybe_spawn_reviewer` + autodream hook | +50 |
| `onboarding.rs` (NEW) | `needs_bootstrap` + BOOTSTRAP_PROMPT | ~100 |
| `skill_manage/mod.rs` (NEW, theo-tooling) | 5-op tool | ~250 |
| `memory_search/mod.rs` (NEW, theo-tooling) | 3-tier tool | ~150 |
| `security.rs` (theo-infra-memory) | `scan_skill_body` + ~50 patterns | +120 |
| `memory_tantivy.rs` (theo-engine-retrieval) | RAM → MmapDirectory + transcript schema | +100 |
| `skill_catalog.rs` | Add `origin` frontmatter field | +15 |
| `user_profile.rs` (NEW, theo-infra-memory) | UserProfile struct + markdown roundtrip | ~80 |
| **Total source** | | **~1340 LOC** |
| Tests (~28 new across 5 `tests/*.rs`) | | **~900 LOC** |

---

## Key Risks Identified During Research

1. **`tokio::spawn` lifetime**: background tasks need `'static` owned data. Must clone `Arc<...>`, `Vec<Message>`, prompts. **No `&self` capture**.

2. **Hermes Issue #8506 (nudge reset bug)**: gateway mode's per-message agent instantiation resets counters. Our Rust advantage — `RunEngine` is persistent with `AtomicUsize` — **eliminates by design**.

3. **Recursive review spawning**: Hermes mitigates by zeroing intervals on forked agent. We do same — reviewer's `AgentConfig` clone must have `memory_review_nudge_interval = 0` + `skill_review_nudge_interval = 0`.

4. **Tantivy persistence + crash recovery**: partial commits corrupt. Solution: `IndexWriter::commit()` atomically + `.corrupt` rename on load failure (mimic episodes/).

5. **Autodream lock orphan**: if LLM call fails, lock stays. Solution: timestamp check ignores locks older than 2×max_runtime.

6. **Promptware attacks on skill bodies** (Hermes Issue #496): regex-only misses conjunctions. Mitigation: after regex, LLM-side adversarial check before quarantine→active promotion. (Already part of our 7-gate chain.)

7. **Tantivy growth**: content hash of transcript (SHA-256 of last N msgs) to skip re-index on session retake.

---

## Phase Execution Plan (verified against plan)

```
Phase 1 — Nudge Counter + MemoryReviewer trait + spawn        (~200 src + ~150 test)
Phase 2 — Autodream daemon (ConsolidationMeta + lock + 24h)   (~250 src + ~200 test)
Phase 3 — SkillReviewer + skill_manage tool + scan_skill_body (~370 src + ~250 test)
Phase 4 — Tantivy persistence + memory_search 3-tier tool     (~350 src + ~200 test)
Phase 5 — Onboarding + UserProfile + auto-improvement prompt  (~170 src + ~100 test)
                                                               ─────────────────────
                                                               ~1340 src + ~900 test
```

Each phase mergeable independently. `theo-evaluate.sh` baseline score must not decrease (hygiene floor).

---

## Reference Citations

- `referencias/hermes-agent/run_agent.py:1418-1521, 2745-2879, 8738-8760, 11846-11875` — nudge + spawn
- `referencias/hermes-agent/tools/memory_tool.py:116-220` — MemoryStore char limits
- `referencias/hermes-agent/tools/skill_manager_tool.py:1-510` — 5-op tool
- `referencias/hermes-agent/tools/skills_guard.py:82-250+` — threat patterns
- `referencias/opendev/crates/opendev-agents/src/memory_consolidation.rs:1-441` — Rust consolidation + lock + 24h
- `referencias/opendev/crates/opendev-agents/src/doom_loop.rs:1-205` — agent loop structure
- `docs/plans/PLAN_AUTO_EVOLUTION_SOTA.md` — target plan (DRAFT)
- `docs/plans/PLAN_MEMORY_SUPERIORITY.md` — foundation (APPROVED)
- `docs/plans/PLAN_CONTEXT_WIRING.md` — context wiring (DRAFT, merged)
