# OpenDev: Building AI Coding Agents for the Terminal — Scaffolding, Harness, Context Engineering, and Lessons Learned

**Paper:** arXiv:2603.05344v1
**Autor:** Nghi D. Q. Bui (OpenDev)
**Data:** March 5, 2026
**Repo:** https://github.com/opendev-to/opendev
**Contato:** bdqnghi@gmail.com

---

## 1. O que é

OpenDev é um coding agent open-source, terminal-native, documentado como o **primeiro technical report completo** de um sistema agentic de código aberto para engenharia de software. O paper não apresenta um avanço algorítmico — é um **blueprint de engenharia** com design decisions, trade-offs, e lições aprendidas de construir um sistema de produção.

**3 princípios de design:**
1. **Separation of concerns** — cada decisão (model, context, safety, tools) é configurável independentemente
2. **Progressive degradation** — funciona graciosamente com recursos esgotados (tokens, iterações, rede)
3. **Transparency over magic** — toda ação (tool calls, compaction, memory updates) é observável e overridable

**5 contribuições concretas:**
1. Per-workflow LLM binding (compound AI architecture)
2. Extended ReAct loop com thinking + self-critique
3. Event-driven system reminders contra instruction fade-out
4. Lazy tool discovery via MCP + 5-layer safety
5. Context engineering como first-class concern (Adaptive Context Compaction)

## 2. Arquitetura de 4 Camadas

```
┌─────────────────┐  ┌───────────────────┐  ┌──────────────────┐  ┌──────────────────┐
│ Entry & UI      │  │ Agent Layer       │  │ Tool & Context   │  │ Persistence      │
│                 │  │                   │  │                  │  │                  │
│ CLI Entry Point │→ │ Model Selection   │→ │ Tool Registry    │→ │ Session Manager  │
│ TUI (Textual)   │  │ Normal/Plan Mode  │  │ Context Eng.     │  │ Config Manager   │
│ Web UI (FastAPI) │  │ Extended ReAct    │  │ Safety System    │  │ Provider Cache   │
│                 │  │ Subagent Orch.    │  │ Memory Pipeline  │  │ Operation Log    │
└─────────────────┘  └───────────────────┘  └──────────────────┘  └──────────────────┘
```

### Entry & UI Layer
- CLI, TUI (Textual/blocking), Web UI (FastAPI/WebSocket/polling)
- 4 shared managers injetados: ConfigManager, SessionManager, ModeManager, ApprovalManager
- UI-agnostic via `UICallback` contract

### Agent Layer
- **Single concrete class**: `MainAgent` — sem hierarquia de classes. Behavioral variation vem de `allowed_tools`, `_subagent_system_prompt`, e `is_subagent`
- **5 model roles** com fallback chains (ver §3)
- **Eager construction**: system prompt + tool schemas built no `__init__()`, zero first-call latency
- **Dual-mode**: Plan Mode (read-only, delegated ao Planner subagent) vs Normal Mode (full access)

### Tool & Context Layer
- **ToolRegistry**: dispatch por categoria (File, Process, Web, Symbols, Task, MCP, etc.)
- **ToolSchemaBuilder**: 3 sources — built-in schemas, MCP discovered, subagent schemas
- **Context Engineering Layer**: 6 subsystems (ver §6)
- **Safety**: 5 independent layers (ver §4)

### Persistence Layer
- JSON/JSONL/plain text — sem database externo
- Session storage: `.json` metadata + `.jsonl` transcript (separados para performance)
- Operation log + shadow git snapshots para undo
- 4-tier config: defaults → env vars → user-global → project-local

## 3. Multi-Model Architecture (5 Roles)

| Role | Propósito | Fallback | Quando usar |
|------|-----------|----------|-------------|
| **Action** | Primary execution, tool-based reasoning | — | Default para tudo |
| **Thinking** | Extended reasoning SEM tools | → Action | Quando thinking_level ≠ OFF |
| **Critique** | Self-evaluation (Reflexion-inspired) | → Thinking → Action | Apenas em thinking_level = HIGH |
| **Vision (VLM)** | Screenshots, images | → Action (if vision-capable) | Visual debugging |
| **Compact** | Summarization durante compaction | — | Context management |

**Design decisions:**
- Lazy client initialization — só cria HTTP client no primeiro uso de cada role
- Provider cache com 24h TTL (stale-while-revalidate)
- Model capabilities (context length, vision, pricing) fetched de API externa, cached localmente
- Switching providers = config change, não code change

## 4. Safety Architecture (5 Layers, Defense-in-Depth)

| Layer | O que faz | Mecanismo |
|-------|-----------|-----------|
| **1. Prompt-Level Guardrails** | Security policy, action safety, read-before-edit | System prompt sections |
| **2. Schema-Level Tool Restrictions** | Plan-mode whitelist, per-subagent `allowed_tools`, MCP gating | Ferramentas invisíveis ao LLM |
| **3. Runtime Approval System** | Manual/Semi-Auto/Auto levels, pattern/command/prefix/danger rules | `ApprovalRulesManager` persistente |
| **4. Tool-Level Validation** | DANGEROUS_PATTERNS blocklist, stale-read detection, output truncation | Pre-execution checks |
| **5. Lifecycle Hooks** | Pre-tool blocking (exit code 2), argument mutation, JSON stdin protocol | External scripts |

**Lição-chave: "Make unsafe tools invisible, not blocked."** Remover do schema > bloquear no runtime. O modelo não pode tentar invocar o que não sabe que existe.

**10 lifecycle hook events:** SESSION_START, USER_PROMPT_SUBMIT, PRE_TOOL_USE, POST_TOOL_USE, POST_TOOL_USE_FAILURE, SUBAGENT_START, SUBAGENT_STOP, PRE_COMPACT, SESSION_END, STOP

**Doom-loop detection (two-tier):**
1. MD5 fingerprint de `(tool_name, args)` em sliding window de 20 calls
2. Se fingerprint aparece ≥3×: injeta `[SYSTEM WARNING]` message
3. Se repete após warning: escala para `ApprovalManager` ("Agent is repeating the same action. Allow / Break?")

## 5. Extended ReAct Loop (6 Phases per Iteration)

```
┌─────────────────────────────────────────────────────┐
│ ReactExecutor.execute()                              │
│                                                      │
│  Phase 0: PRE-CHECKS & COMPACTION                   │
│    ├─ Drain injection queue                          │
│    ├─ Context pressure check: p = tokens/max_context │
│    ├─ if p > 0.99 → Full LLM summarization          │
│    ├─ if p > 0.90 → Aggressive masking              │
│    ├─ if p > 0.85 → Fast pruning                    │
│    ├─ if p > 0.80 → Observation masking              │
│    └─ if p > 0.70 → Warning log                     │
│                                                      │
│  Phase 1: THINKING (if enabled)                     │
│    ├─ Call thinking LLM (no tools!)                  │
│    ├─ if HIGH → also call critique LLM              │
│    └─ Inject trace as system reminder               │
│                                                      │
│  Phase 2: ACTION                                    │
│    ├─ PromptComposer assembles full prompt           │
│    ├─ ACE Playbook bullets injected                  │
│    └─ LLM API call → response + tool_calls          │
│                                                      │
│  Phase 3: DECISION                                  │
│    ├─ No tools? → error recovery nudge or break     │
│    ├─ Doom-loop detection (MD5 fingerprinting)      │
│    └─ Dispatch tools (parallel/sequential)          │
│                                                      │
│  Phase 4: LEARN                                     │
│    ├─ Record tool outcomes                           │
│    └─ ACE Memory: Reflector → Curator → Playbook    │
│                                                      │
│  until: task_complete called ∨ max iterations        │
└─────────────────────────────────────────────────────┘
```

**4 termination paths:**
1. Agent calls `task_complete` tool explicitly
2. Text response with no tool calls (implicit completion)
3. Error-recovery budget exhausted (3 nudge attempts)
4. Iteration count reaches safety limit

**Anti-premature-completion:** Before accepting termination, system checks for incomplete todo items and pending injection queue messages. Nudges agent to continue if outstanding work remains.

## 6. Context Engineering (6 Subsystems)

### 6.1 Dynamic System Prompt Construction (PromptComposer)

- **21 modular sections** stored as separate markdown files
- Each section has: `condition` predicate (runtime context check) + `priority` integer
- Pipeline: **Filter** → **Sort** (ascending priority) → **Load** (resolve `${VAR}` placeholders) → **Join**
- Sections gated on: `in_git_repo`, provider type, feature flags
- Provider-specific sections (Anthropic vs OpenAI vs Fireworks) loaded exclusively
- **Prompt caching**: split into stable (cacheable, 80-90%) + dynamic (non-cacheable) parts
- **Two-tier fallback**: missing section file → skip; all sections missing → fallback to monolithic template

### 6.2 Tool Result Optimization

Per-tool-type summarization (50-200 chars):
- File reads → `"✓ Read file (142 lines, 4,831 chars)"`
- Search → `"✓ Search completed (23 matches found)"`
- Directory listings → `"✓ Listed directory (47 items)"`
- Command execution: short (≤100 chars) verbatim, long → `"✓ Command executed (312 lines of output)"`
- Errors → truncated to 200 chars with classified prefix

**Large output offloading** (>8,000 chars):
- Full output → scratch file (`~/.opendev/scratch/<session_id>/`)
- Context gets: 500-char preview + reference path
- **Agent-aware truncation hints**: if agent has subagent access → "Delegate to Code Explorer"; if subagent → "Use search tool with offset/limit"

**Impact**: Single test suite from 30,000 tokens → under 100 tokens. Extended sessions from 15-20 turns → 30-40 turns without compaction.

### 6.3 Dual-Memory Architecture for Thinking

- **Episodic memory**: LLM summary of full conversation (decisions, goals, key findings, file paths). Max 500 chars. Regenerated every 5 messages (not incrementally — prevents summary drift)
- **Working memory**: Last 6 exchanges verbatim (fine-grained operational details)
- **Combined injection**: episodic (big picture) + working (operational detail) + current query

### 6.4 Context-Aware System Reminders

**Problem:** System prompt instructions reliably violated after 30+ tool calls. Attention decays from initial instructions toward recent messages.

**Solution:** 8 event detectors inject short `role: user` messages at the decision point:
1. Tool failure without retry
2. 5+ consecutive read operations (exploration spiral)
3. Tool call denied by user
4. Premature completion with incomplete todos
5. All todos now complete
6. Plan just approved
7. Subagent returned results
8. Empty completion message

**Guardrail counters** cap each reminder type: todo nudges ≤ 2, error recovery ≤ 3, others fire once. Prevents noise → model learns to ignore.

**Key insight:** `role: user` > `role: system` for reminders. User messages have higher recency salience.

### 6.5 Context-Injected Error Recovery

4-step pipeline:
1. Classify error (permission, file not found, edit mismatch, syntax, rate limit, timeout)
2. Retrieve template from centralized store
3. Format with context (file path, error message, mismatched content)
4. Inject as system message before next LLM call

Budget: 3 nudge attempts per error sequence. Then accept failure or ask user.

### 6.6 Adaptive Context Compaction (ACC) — 5 Stages

| Stage | Trigger (pressure) | Action | Cost |
|-------|-------------------|--------|------|
| 1. Warning | 70% | Log only, monitor trends | Free |
| 2. Observation Masking | 80% | Replace old tool results with compact pointers | Free |
| 3. Fast Pruning | 85% | Walk backward, replace outputs beyond recency budget with `[pruned]` | Free |
| 4. Aggressive Masking | 90% | Shrink preservation window to most recent only | Free |
| 5. Full Compaction | 99% | Serialize to scratch file + LLM summarization | LLM call |

**Artifact Index**: Structured registry of all files touched during session (read, created, modified, deleted). Serialized into compaction summary — agent remembers what files it worked with even after compaction.

**Impact:** ACC reduces peak context consumption of observations by ~54%, often eliminating emergency compaction entirely over 30-turn sessions.

## 7. Subagent Orchestration

### 8 Subagent Types

| Type | Tools | Purpose |
|------|-------|---------|
| Code-Explorer | Read-only | Codebase navigation |
| Planner | Read-only + present_plan | Strategic planning |
| PR-Reviewer | Read-only + diff analysis | Code review |
| Security-Reviewer | Read-only + vulnerability scanning | Security audit |
| Web-Clone | Web fetch + file write | Website replication |
| Web-Generator | Full web tools | Site creation from spec |
| Project-Init | Full scaffold tools | Project bootstrapping |
| Ask-User | UI-only | Structured user surveys |

**Automatic parallelization:** Multiple `spawn_subagent` calls in same LLM response → `asyncio.gather()` → concurrent execution. Each subagent gets own iteration budget and tool worker pool.

**Parameters:** model override (haiku/sonnet/opus), background execution, session resume by agent ID.

**Design evolution:** Early versions gave subagents same tools as main agent → context pollution, role confusion, file conflicts. Restricting tools per role improved both focus and efficiency.

## 8. ACE Memory Pipeline (Agentic Context Engineering)

4-stage learning loop:

```
Stage 1: BulletSelector
  ├─ Score: effectiveness × 0.5 + recency × 0.3 + semantic × 0.2
  └─ Top-K bullets → Generator system prompt

Stage 2: Reflector (every 5 messages)
  ├─ Analyze accumulated experience
  └─ Produce: reasoning trace, error ID, root cause, correct approach

Stage 3: Curator
  ├─ Read reflection
  └─ Plan mutations: ADD, UPDATE, TAG (helpful/harmful/neutral), REMOVE

Stage 4: Apply
  ├─ DeltaBatch mutations → Playbook bullet table
  └─ Persist to session-scoped JSON file
```

**Playbook**: Collection of natural-language bullets, each tagged with effectiveness counters (helpful/harmful/neutral) and creation timestamp. Persistent across session, session-scoped.

## 9. Persistence & Undo

### Session Storage
- Metadata (`.json`) + transcript (`.jsonl`) — separated for fast listing
- Atomic writes via `fcntl.flock` + `os.rename()`
- Auto-save every 5 turns
- Self-healing session index (regenerates if corrupted)
- Cost tracking persisted in session metadata (`cost_tracking` object)

### Shadow Git Snapshots
- Bare git repo at `~/.opendev/snapshot/<project-id>/`
- `git add . && git write-tree` at every agent step
- `/undo` computes `git diff` between current and snapshot tree
- `git checkout <hash> -- <file>` restores specific files
- `git gc --prune=7.days` keeps compact

### Operation Log
- In-memory list (capped at 50) + `operations.jsonl` on disk
- Records: operation type, file path, timestamp, unique ID, file content before operation
- Undo: created files → delete; modified → restore backup; deleted → restore

## 10. Design Lessons (Section 3)

### 10.1 Context Pressure as Central Design Constraint
> "Treat context as a budget, not a buffer."
- Tool outputs consume 70-80% of context
- Graduated reduction (monitor → prune → mask → summarize) >> binary emergency compaction
- Fast pruning alone often avoids expensive LLM compaction

### 10.2 Steering Behavior Over Long Horizons
> "Inject reminders at the point of decision, not upfront."
- System prompt instructions violated after 30+ tool calls
- `role: user` reminders >> `role: system` reminders (higher recency salience)
- Cap reminder frequency to prevent noise
> "Separate thinking from action."
- Providing thinking LLM WITHOUT tools produces better reasoning traces
- Mechanism: absence of tool schemas from API call, not instruction

### 10.3 Safety Through Architectural Constraints
> "Make unsafe tools invisible, not blocked."
- Schema gating > runtime permission checks
- Defense-in-depth with independent layers
- Approval persistence prevents fatigue
- Lifecycle hooks as extensibility mechanism

### 10.4 Designing for Approximate Outputs
> "Design tools to absorb LLM imprecision."
- edit_file: 9-pass fuzzy matching chain (exact → whitespace → indentation → escape → anchor)
- Short-circuits on first match, zero overhead for exact matches
> "Adapt recovery hints to the agent's available tool set."
- Context-aware truncation hints (subagent delegation vs offset/limit)
- Classified error recovery templates >> generic "try again"

### 10.5 Lazy Loading and Bounded Growth
> "Bound every resource that grows with session length."
- MCP lazy discovery: startup context from 40% → <5%
- Skills: metadata index at startup, full content on-demand
- Iteration limits, undo history caps, concurrent tool caps
> "Prefer empirical threshold tuning over first-principles calculation."
- 70% compaction trigger, 3 nudge attempts, 6 thinking depths — all from iterative failure analysis

## 11. Números de Referência

| Métrica | Valor | Contexto |
|---------|-------|----------|
| ACC context reduction | **~54%** | Peak observation consumption reduction |
| Tool result summarization | 30,000 → **<100 tokens** | Single test suite output |
| Session extension | 15-20 → **30-40 turns** | Before needing compaction |
| MCP startup context | 40% → **<5%** | Lazy discovery vs eager loading |
| Prompt caching savings | **~88%** | Input token reduction for cached prefix |
| Fuzzy edit passes | **9** | Chain-of-responsibility for edit matching |
| Subagent types | **8** | Specialized roles |
| Lifecycle hook events | **10** | Full agent lifecycle coverage |
| Compaction stages | **5** | Progressive context pressure response |
| System reminder events | **8** | Behavioral steering detectors |
| Safety layers | **5** | Defense-in-depth |

## 12. Relevância para Theo Code

### Patterns a adotar diretamente

| Pattern OpenDev | Crate Theo Code alvo | Prioridade |
|----------------|----------------------|-----------|
| 5-stage ACC (compaction) | `theo-agent-runtime` (compaction_stages.rs) | HIGH — já temos estágios, verificar alinhamento |
| Doom-loop MD5 fingerprinting | `theo-agent-runtime` (agent_loop.rs) | HIGH — mais robusto que contagem simples |
| System reminders (8 detectors + guardrail counters) | `theo-agent-runtime` | HIGH — resolve attention decay |
| 9-pass fuzzy edit matching | `theo-tooling` (edit tool) | MEDIUM — reduz "content not found" errors |
| Tool result summarization | `theo-tooling` | MEDIUM — context savings massivos |
| Dual-memory (episodic 500 chars + working 6 turns) | `theo-infra-memory` | HIGH — RM roadmap aligned |
| ACE Playbook (effectiveness scoring) | `theo-infra-memory` (lesson_store) | HIGH — RM4 aligned |
| Lazy MCP discovery | `theo-infra-mcp` | MEDIUM — 40% → 5% context |
| Shadow git snapshots | `theo-agent-runtime` | LOW — temos git-based undo similar |
| Provider cache (stale-while-revalidate) | `theo-infra-llm` | MEDIUM |

### Anti-patterns confirmados

1. **Binary compaction** (compact everything at 95%) → Graduated stages are 54% more efficient
2. **Runtime permission checks** → Schema gating is fundamentally more robust
3. **Single model for all workloads** → Per-workflow binding saves cost without code changes
4. **Generic "try again" error recovery** → Classified templates with context dramatically improve recovery
5. **Loading all MCP schemas at startup** → 40% context wasted on never-used tools
6. **Incremental summarization** → Accumulates distortion. Periodic regeneration from full history corrects drift

### Thresholds para o SOTA registry

| Métrica | Threshold | Source |
|---------|-----------|-------|
| Compaction stages | ≥ 5 | OpenDev ACC (5 stages at 70/80/85/90/99%) |
| Doom-loop detection threshold | ≥ 3 identical fingerprints | OpenDev MD5 window-20 |
| System reminder event types | ≥ 8 | OpenDev detector catalog |
| Tool result summary compression | ≥ 100× para test suites | 30K → <100 tokens |
| MCP lazy discovery overhead | ≤ 5% context at startup | vs 40% eager loading |
| Fuzzy edit matching passes | ≥ 3 (exact + whitespace + indent) | OpenDev uses 9 |
| Edit stale-read detection | ≤ 50ms tolerance | OpenDev FileTimeTracker |

---

**Citação:**
```
Bui, N. D. Q. (2026). Building AI Coding Agents for the Terminal: Scaffolding, Harness, Context Engineering, and Lessons Learned. arXiv:2603.05344v1.
```
