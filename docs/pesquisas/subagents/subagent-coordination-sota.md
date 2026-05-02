---
type: report
question: "What are the state-of-the-art subagent coordination patterns for AI coding agents, including Agent Teams, file locking, shared task lists, and automatic parallelization?"
generated_at: 2026-04-29T12:00:00-03:00
confidence: 0.85
sources_used: 19
supplements: sota-subagent-architectures.md
---

# Subagent Coordination for AI Coding Agents: State of the Art

## Executive Summary

Subagent coordination has evolved from simple orchestrator-worker patterns (covered in sota-subagent-architectures.md) to sophisticated multi-agent systems with shared state, peer-to-peer messaging, and automatic parallelization. Three developments define the 2026 frontier: (1) Claude Code Agent Teams introduce a progressive complexity ladder from subagents (return-only) to teams (shared task list + peer messaging + file locking), available since v2.1.32 (February 2026). (2) The "Dive into Claude Code" paper (arXiv:2604.14228) reveals that 98.4% of a production agent's codebase is deterministic infrastructure, not AI logic -- meaning coordination primitives matter more than model improvements. (3) OpenDev's subagent compilation pipeline demonstrates a practical pattern for cheap subagent construction via shared tool registry references, with file locking via `threading.Lock` per file path and lightweight kanban-style task management. The consensus emerging across systems is that three focused agents consistently outperform one generalist working three times as long, but only when collision prevention (file locking, task partitioning, worktree isolation) is robust.

---

## Part 1: Claude Code Agent Teams -- Deep Dive

### 1.1 Architecture

Agent Teams shipped February 5, 2026, alongside Claude Opus 4.6. The feature is experimental, behind `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` in settings.json or environment.

```
                    +------------------+
                    |    Team Lead     |
                    | (main session)   |
                    +--------+---------+
                             |
                    TeamCreate tool
                    TeamMessage tool
                    SendMessage tool
                             |
              +--------------+--------------+
              |              |              |
        +-----v----+  +-----v----+  +------v---+
        | Teammate |  | Teammate |  | Teammate |
        |  "auth"  |  |  "api"   |  | "tests"  |
        +-----+----+  +-----+----+  +------+---+
              |              |              |
              +--------------+--------------+
                    Shared Task List
                    ~/.claude/tasks/{team-name}/
                    
                    Mailbox System
                    (peer-to-peer messaging)
```

### 1.2 Four Core Components

| Component | Description | Implementation |
|-----------|-------------|---------------|
| **Team Lead** | Main Claude Code session. Creates teams, spawns teammates, orchestrates. | Uses TeamCreate tool. Assigns names to each teammate. |
| **Teammates** | Independent Claude Code processes. Own context window. Full tool access. | Spawned as separate processes. 2-16 teammates per team. |
| **Shared Task List** | Coordination backbone. Tasks have status, ownership, dependencies. | Files in `~/.claude/tasks/{team-name}/`. Statuses: pending, in_progress, completed, blocked. |
| **Mailbox System** | Peer-to-peer messaging. Any teammate can message any other. | SendMessage tool. Direct (by name) or broadcast (all teammates). |

### 1.3 Shared Task List -- Detailed Mechanics

The shared task list is the central coordination mechanism:

```
Task {
  id: string
  title: string
  description: string
  status: "pending" | "in_progress" | "completed" | "blocked"
  owner: string | null           // teammate name
  dependencies: string[]         // task IDs that must complete first
  blocked_by: string[]           // auto-computed from dependencies
  files: string[]                // files this task will modify
  created_at: timestamp
  completed_at: timestamp | null
}
```

**Workflow:**
1. Team lead creates tasks with dependencies
2. Teammates claim available (unblocked, pending) tasks
3. Teammate marks task in_progress
4. On completion, marks task completed
5. Any tasks blocked by the completed task automatically unblock
6. Teammates check for newly available tasks

**Key Difference from OpenDev's todo system:** Claude Code Agent Teams support dependency tracking (task B waits for task A). OpenDev's `write_todos/update_todo/complete_todo/list_todos` is a flat kanban without dependencies.

### 1.4 Peer-to-Peer Messaging

Unlike standard subagents (return-only to parent), Agent Teams support direct peer communication:

| Message Type | Target | Use Case |
|-------------|--------|----------|
| **Direct** | One teammate by name | "auth-agent: the User model schema changed, update your imports" |
| **Broadcast** | All teammates | Sparingly -- cost scales with team size |
| **To lead** | Team lead | Report completion or request guidance |

**Communication triggers:**
- Automatic message delivery when a teammate is idle
- Idle notifications when a teammate finishes work
- Shared task list updates visible to all

### 1.5 Progressive Complexity Ladder

Claude Code provides a deliberate progression:

| Level | Feature | Coordination | Cost |
|-------|---------|-------------|------|
| **Single agent** | Main session only | None | 1x |
| **Subagents** | Spawned via tool, return summary only | Parent manages all | ~2-3x |
| **Agent Teams** | Independent sessions, shared state | Peer-to-peer + shared tasks | ~5-16x |

This ladder lets users start simple and add coordination only when needed.

### 1.6 Limitations

- Token usage scales with number of active teammates (each has own context window)
- Experimental: behind feature flag, not GA
- File locking is advisory, not enforced at filesystem level
- Broadcast messages are expensive (sent to every teammate)
- Requires Pro ($20/mo), Max ($100-200/mo), Team ($150/user/mo), or Enterprise plan

---

## Part 2: "Dive into Claude Code" Analysis (arXiv:2604.14228)

### 2.1 Key Findings

The paper by Liu et al. (April 2026) reverse-engineered Claude Code v2.1.88, analyzing 1,884 files and ~512K lines of TypeScript.

**Infrastructure vs AI Logic:**

```
Total codebase: ~512K lines
  |
  |-- 98.4% Deterministic Infrastructure
  |   |-- Permission system (7 modes, ML classifier)
  |   |-- 5-layer compaction pipeline
  |   |-- 54 tools with typed schemas
  |   |-- 27 hook events
  |   |-- Session persistence
  |   |-- Error handling and recovery
  |   |-- Terminal rendering (389 components)
  |
  |-- 1.6% AI Decision Logic
      |-- Core agent loop (while-loop: call model, run tools, repeat)
      |-- Model selection
      |-- Prompt construction
```

**Implication:** The critical differentiator for agent reliability is the deterministic engineering harness, not the AI decision logic. Subagent coordination is part of the 98.4%.

### 2.2 Seven-Component Architecture

| Component | Description | Relevance to Subagents |
|-----------|-------------|----------------------|
| **User** | Input handling, preferences | Defines team structure |
| **Interfaces** | CLI, TUI, API | Multiple teammates need separate interfaces |
| **Agent Loop** | While-loop: model call -> tool use -> repeat | Each subagent/teammate runs its own loop |
| **Permission System** | 7 modes, deny-first, ML classifier | Shared across teammates |
| **Tools** | 54 tools with typed schemas | Tool access restricted by role |
| **State & Persistence** | Session state, context management | Shared task list is state management |
| **Execution Environment** | Sandbox, process management | Each teammate is a separate process |

### 2.3 Safety Architecture

Five human values motivating design: human decision authority, safety and security, reliable execution, capability amplification, contextual adaptability. These are traced through 13 design principles to implementation.

The safety architecture uses defense in depth with reversibility-weighted risk assessment: actions with higher irreversibility require more safety checks. This is relevant for subagent coordination because file writes are less reversible than file reads, so writing subagents need higher safety bars.

---

## Part 3: OpenDev Subagent Compilation Pipeline

### 3.1 SubAgentSpec to CompiledSubAgent

OpenDev implements a 4-step subagent compilation pipeline:

```python
# Step 1: Define SubAgentSpec
spec = SubAgentSpec(
    name="code_explorer",
    description="Explores codebase structure and finds relevant code",
    tools=["read_file", "list_files", "search_text", "find_symbol"],
    system_prompt_template="You are a code exploration agent...",
    max_iterations=50,
    timeout_seconds=300
)

# Step 2: register_subagent() -- 4-step pipeline
def register_subagent(spec: SubAgentSpec) -> CompiledSubAgent:
    # 2a. Resolve tools from shared registry (reference, not copy)
    resolved_tools = tool_registry.resolve(spec.tools)
    
    # 2b. Create AppConfig with subagent-specific settings
    config = AppConfig(
        model=spec.model or default_model,
        max_iterations=spec.max_iterations,
        tools=resolved_tools,
        is_subagent=True
    )
    
    # 2c. Construct MainAgent instance
    agent = MainAgent(config)
    
    # 2d. Set system prompt
    agent.set_system_prompt(spec.system_prompt_template)
    
    return CompiledSubAgent(
        name=spec.name,
        description=spec.description,
        agent=agent,
        tool_list=resolved_tools
    )

# Step 3: Spawn
result = await compiled_subagent.agent.run(task_prompt)
```

**Key Design: Cheap Construction.** The `CompiledSubAgent` holds a reference to the shared tool registry, not a copy. This makes subagent construction lightweight because tool schemas (which can be large) are shared across all agents.

### 3.2 File Locking

OpenDev implements file locking via `threading.Lock` per file path in `FileTimeTracker`:

```python
class FileTimeTracker:
    """Tracks file access and prevents concurrent writes."""
    
    def __init__(self):
        self._locks: Dict[str, threading.Lock] = {}
        self._lock_creation_lock = threading.Lock()
    
    def get_lock(self, file_path: str) -> threading.Lock:
        """Get or create a lock for a specific file path."""
        with self._lock_creation_lock:
            if file_path not in self._locks:
                self._locks[file_path] = threading.Lock()
            return self._locks[file_path]
    
    def acquire_for_write(self, file_path: str, timeout: float = 30.0) -> bool:
        """Acquire write lock. Returns False if timeout."""
        lock = self.get_lock(file_path)
        return lock.acquire(timeout=timeout)
    
    def release_write(self, file_path: str):
        """Release write lock."""
        lock = self.get_lock(file_path)
        if lock.locked():
            lock.release()
```

**Comparison with other approaches:**

| System | Locking Strategy | Level | Issues |
|--------|-----------------|-------|--------|
| **OpenDev** | `threading.Lock` per file path | Advisory, in-process | Only works within same process |
| **Claude Code Agent Teams** | File field in shared task list | Advisory, cross-process | Not enforced at OS level |
| **Cursor** | Tried optimistic concurrency | Cross-process | Failed: agents became risk-averse |
| **Cursor** | Tried lock-based | Cross-process | Failed: agents held locks too long (20 agents -> throughput of 2-3) |
| **Forge Orchestrator** | File locking + drift detection | Cross-process | Rust binary, ~3MB |
| **Git Worktrees** | Filesystem isolation | OS-level | Each agent gets own working directory |

**Lesson from Cursor's failures:** Lock-based approaches fail when agents hold locks too long. Optimistic concurrency fails because agents become risk-averse and avoid hard tasks. The industry consensus is moving toward **"One File, One Owner"** -- partition work so no two agents ever touch the same file.

### 3.3 Shared Task List -- OpenDev's Lightweight Kanban

OpenDev implements task management as four tools available to the main agent:

| Tool | Function | Access |
|------|----------|--------|
| `write_todos` | Create new tasks | Main agent only |
| `update_todo` | Update task status/description | Main agent only |
| `complete_todo` | Mark task as done | Main agent only |
| `list_todos` | List all tasks with status | Main agent only |

**Key constraint:** Task management tools are excluded from subagents. Only the main agent coordinates. This prevents coordination chaos where subagents create tasks for each other.

**Comparison with Claude Code Agent Teams:**

| Feature | OpenDev | Claude Code Agent Teams |
|---------|---------|----------------------|
| Dependency tracking | No (flat list) | Yes (blocked/unblocked) |
| Who manages tasks | Main agent only | Lead + teammates can claim |
| File association | No | Yes (tasks declare files) |
| Auto-unblock | No | Yes (on dependency completion) |
| Peer messaging | No | Yes (mailbox system) |

---

## Part 4: Automatic Parallelization

### 4.1 OpenDev Pattern

When the model returns multiple `spawn_subagent` tool calls in the same response, OpenDev automatically parallelizes them:

```python
async def execute_tool_calls(self, tool_calls: List[ToolCall]):
    """Execute tool calls, parallelizing subagent spawns."""
    
    # Separate subagent spawns from other tool calls
    subagent_calls = [tc for tc in tool_calls if tc.name == "spawn_subagent"]
    other_calls = [tc for tc in tool_calls if tc.name != "spawn_subagent"]
    
    # Execute other tools sequentially
    for tc in other_calls:
        await self.execute_tool(tc)
    
    # Execute subagent spawns in parallel
    if subagent_calls:
        results = await asyncio.gather(
            *[self.spawn_subagent(tc) for tc in subagent_calls]
        )
        # Each subagent has its own iteration budget and tool worker pool
```

**Key constraints per subagent:**
- Own iteration budget (default: 50 iterations)
- Own tool worker pool
- Shared tool registry (reference)
- Own context window

### 4.2 Theo Code's Current Parallel Execution

From sota-subagent-architectures.md, Theo already has:
- `spawn_parallel()` with `tokio::JoinSet`
- `PrefixedEventForwarder` tagging events by role
- Timeout per role (5-10 min)
- Depth=1 enforcement

**What's missing for 4.0+:**
- Automatic parallelization (detecting independent tasks and spawning in parallel without explicit instruction)
- File locking to prevent parallel agents from conflicting
- Shared task list for coordination beyond return-only

### 4.3 Parallelization Thresholds

| Metric | Target | Source |
|--------|--------|--------|
| **max_depth** | 1 (consensus across Claude Code, Codex, OpenDev) | Industry standard |
| **max_concurrent** | 5 (sweet spot for manageable review) | Addy Osmani, Mike Mason |
| **compute_delegation** | >= 70% (proportion of compute done by subagents vs main) | Anthropic multi-agent research |
| **conflict rate** | < 5% (file edit conflicts between parallel agents) | Target, no industry benchmark |
| **orchestration overhead** | < 15% of total tokens spent on coordination | Target |

---

## Part 5: Coordination Patterns -- Cross-System Analysis

### 5.1 File Conflict Prevention Strategies

| Strategy | How It Works | Adoption |
|----------|-------------|----------|
| **One File, One Owner** | Partition tasks so no two agents touch the same file | Emerging consensus |
| **Git Worktrees** | Each agent gets its own working directory | Claude Code Agent Teams, Forge |
| **Advisory Locks** | Agent declares intent, manager rejects conflicts | OpenDev, proposed for Theo |
| **Optimistic Concurrency** | Write freely, detect and resolve conflicts after | Cursor (failed) |
| **Task-File Association** | Tasks declare which files they'll modify | Claude Code Agent Teams |

### 5.2 Communication Topology Comparison

```
Return-Only (Subagents):          Peer-to-Peer (Agent Teams):
                                  
    Lead                              Lead
    /  \                             /    \
   S1   S2   S3                   T1 --- T2 --- T3
   |    |    |                     \     |     /
   v    v    v                      Shared Task List
   Lead (aggregates)              
                                  
Pros: Simple, clean context       Pros: Rich coordination
Cons: No peer collaboration       Cons: Higher token cost, complexity
```

### 5.3 When to Use Each Pattern

| Scenario | Recommended Pattern | Rationale |
|----------|-------------------|-----------|
| Independent code review of 3 files | Parallel subagents (return-only) | No coordination needed |
| Full-stack feature implementation | Agent Teams (shared task list) | Frontend depends on API types |
| Codebase exploration/research | Single Explorer subagent | Sequential exploration is fine |
| Large refactoring across modules | Agent Teams + worktrees | High conflict risk without isolation |
| Bug investigation | Sequential subagents | Each step depends on previous findings |

---

## Part 6: Evidence Table

| System | Coordination Model | File Locking | Task Management | Peer Messaging | Max Concurrent | Key Paper/Source |
|--------|-------------------|-------------|----------------|---------------|---------------|-----------------|
| **Claude Code (Subagents)** | Return-only | None | None | None | Unlimited (via JoinSet) | [Claude Docs](https://code.claude.com/docs) |
| **Claude Code (Agent Teams)** | Shared task list + peer | Advisory (task-file) | Dependency tracking | Mailbox system | 2-16 | [Agent Teams Docs](https://code.claude.com/docs/en/agent-teams) |
| **OpenDev** | Orchestrator-worker | threading.Lock per file | Flat kanban (4 tools) | None | asyncio.gather | [arXiv:2603.05344](https://arxiv.org/abs/2603.05344) |
| **Codex CLI** | Configurable depth | None documented | None | None | agents.max_threads | [Codex Docs](https://developers.openai.com/codex/subagents) |
| **Forge Orchestrator** | Hierarchical | File locking + drift detection | Planning system | None | Configurable | [Forge GitHub](https://github.com/forge-ai/forge) |
| **Cursor 3** | Multi-agent workspace | Tried and failed (both approaches) | Implicit (shared workspace) | Implicit | Multiple | [InfoQ](https://www.infoq.com/news/2026/04/cursor-3-agent-first-interface/) |
| **Theo Code (current)** | Return-only | None | None | EventBus (events, not messages) | tokio::JoinSet | Internal |

---

## Part 7: Thresholds and Targets

### Subagent Coordination Performance Targets

| Metric | Current (Theo) | SOTA Target | Gap |
|--------|---------------|-------------|-----|
| Max depth | 1 (hardcoded) | 1 (consensus) | At parity |
| Max concurrent | Unlimited (JoinSet) | 5 (practical sweet spot) | Needs limit |
| Compute delegation | Not measured | >= 70% | Not measured |
| File conflict rate | No protection | < 5% | Missing file locking |
| Peer messaging | None (return-only) | Mailbox system (Agent Teams) | Missing |
| Task management | None | Shared task list with dependencies | Missing |
| Structured results | summary: String | Typed JSON with findings, files, confidence | Partial |
| Worktree isolation | None | Git worktree per agent | Missing |

---

## Part 8: Relevance for Theo Code

### What Theo Code Has (from sota-subagent-architectures.md)

- SubAgentManager with 4 roles (Explorer, Implementer, Verifier, Reviewer)
- CapabilityGate restricting tools per role
- PrefixedEventForwarder for observability
- spawn_parallel() with tokio::JoinSet
- Timeout per role (5-10 min)
- Depth=1 enforcement
- EventBus for event forwarding (not peer messaging)

### What Theo Code Needs to Reach 4.0+

| Priority | Gap | Approach | Complexity | Evidence |
|----------|-----|----------|------------|----------|
| **P0** | No file locking | Implement FileLockManager with advisory locks per file path (OpenDev pattern, adapted for Tokio) | Medium | Cursor's failure shows this is essential for parallel agents |
| **P0** | No max_concurrent limit | Add configurable limit (default 5) to spawn_parallel() | Low | Industry consensus: 3-5 agents is the sweet spot |
| **P1** | No shared task list | Implement lightweight kanban (write/update/complete/list) for main agent | Medium | OpenDev pattern; keep task management out of subagents |
| **P1** | No structured results | Extend AgentResult with typed fields: findings, files_changed, confidence | Low | Anthropic SDK recommendation; reduces context bloat |
| **P2** | No dependency tracking | Add dependency fields to task list (blocked_by, unblocks) | Medium | Claude Code Agent Teams pattern |
| **P2** | No peer messaging | Add mailbox system for subagent-to-subagent communication | High | Only needed for complex multi-step features |
| **P3** | No worktree isolation | Support git worktree per parallel agent | Medium | Emerging consensus for conflict-free parallelism |
| **P3** | No automatic parallelization | Detect independent tool calls and auto-parallelize | Medium | OpenDev asyncio.gather pattern |

### Architecture Recommendation

Extend SubAgentManager in two phases:

**Phase 1: Safe Parallel Execution (P0 + P1)**

```
SubAgentManager (existing)
├─ spawn() / spawn_parallel()     # Keep
├─ CapabilityGate                 # Keep
├─ PrefixedEventForwarder         # Keep
│
├─ FileLockManager (NEW)          # Advisory locks per file path
│  ├─ acquire_for_write(path)     # Returns Result<LockGuard>
│  ├─ release_write(path)         # Auto-release via Drop
│  └─ is_locked(path) -> bool     # Query lock status
│
├─ ConcurrencyLimiter (NEW)       # Limits max parallel agents
│  └─ Semaphore(max_concurrent=5) # Tokio semaphore
│
└─ TaskList (NEW)                 # Lightweight kanban
   ├─ create_task(title, files)   # Main agent only
   ├─ update_status(id, status)   # Main agent only
   ├─ complete_task(id)           # Main agent only
   └─ list_tasks() -> Vec<Task>   # Main agent only
```

**Phase 2: Rich Coordination (P2 + P3)**

```
SubAgentManager (Phase 2)
├─ ... (all Phase 1 components)
│
├─ DependencyTracker (NEW)        # Task dependency graph
│  ├─ add_dependency(task, depends_on)
│  ├─ check_unblocked() -> Vec<Task>
│  └─ on_complete(task) -> auto-unblock
│
├─ Mailbox (NEW)                  # Peer messaging
│  ├─ send(from, to, message)
│  ├─ broadcast(from, message)
│  └─ receive(agent_name) -> Vec<Message>
│
└─ WorktreeManager (NEW)          # Git worktree per agent
   ├─ create_worktree(agent_name)
   ├─ merge_worktree(agent_name)
   └─ cleanup_worktree(agent_name)
```

---

## Sources

- [Claude Code Agent Teams Documentation](https://code.claude.com/docs/en/agent-teams)
- [Claude Code Agent Teams Setup Guide (claudefast)](https://claudefa.st/blog/guide/agents/agent-teams)
- [Claude Code Agent Teams Deep Dive (MindStudio)](https://www.mindstudio.ai/blog/claude-code-agent-teams-parallel-shared-task-list)
- [Dive into Claude Code (arXiv:2604.14228)](https://arxiv.org/abs/2604.14228)
- [VILA-Lab/Dive-into-Claude-Code (GitHub)](https://github.com/VILA-Lab/Dive-into-Claude-Code)
- [Dive into Claude Code Analysis (mer.vin)](https://mer.vin/2026/04/dive-into-claude-code-in-depth-research-analysis-of-agent-harness-architecture/)
- [OpenDev -- Building AI Coding Agents for the Terminal (arXiv:2603.05344)](https://arxiv.org/html/2603.05344v2)
- [OpenDev GitHub](https://github.com/opendev-to/opendev)
- [Addy Osmani -- The Code Agent Orchestra](https://addyosmani.com/blog/code-agent-orchestra/)
- [Multi-Agent Coordination Patterns (Claude Blog)](https://claude.com/blog/multi-agent-coordination-patterns)
- [Multi-Agent Orchestration Patterns (fast.io)](https://fast.io/resources/multi-agent-orchestration-patterns/)
- [From Conductor to Orchestrator (htdocs.dev)](https://htdocs.dev/posts/from-conductor-to-orchestrator-a-practical-guide-to-multi-agent-coding-in-2026/)
- [AI Coding Agents: Coherence Through Orchestration (Mike Mason)](https://mikemason.ca/writing/ai-coding-agents-jan-2026/)
- [Multi-Agent Architecture: 8 Coordination Patterns (Tacnode)](https://tacnode.io/post/multi-agent-architecture)
- [How to Run a Multi-Agent Coding Workspace (Augment Code)](https://www.augmentcode.com/guides/how-to-run-a-multi-agent-coding-workspace)
- [Best Multi-Agent Coding Tools 2026 (Nimbalyst)](https://nimbalyst.com/blog/best-multi-agent-coding-tools-2026/)
- [LushBinary -- Claude Code Agent Teams Guide](https://lushbinary.com/blog/claude-code-agent-teams-multi-agent-development-guide/)
- [From Tasks to Swarms: Agent Teams (alexop.dev)](https://alexop.dev/posts/from-tasks-to-swarms-agent-teams-in-claude-code/)
- [BotMonster -- Claude Code Agent Teams](https://botmonster.com/posts/claude-code-agent-teams-orchestrate-multiple-ai-sessions/)
