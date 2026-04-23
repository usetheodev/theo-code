# Architectural Validation: Dynamic Sub-Agent System Plan

**Date:** 2026-04-23  
**Reviewer:** Claude Code (Arch Validator)  
**Plan:** `/home/paulo/Projetos/usetheo/theo-code/docs/plans/agents-plan.md`

---

## OVERALL ASSESSMENT

**VERDICT: APPROVE WITH CRITICAL CONCERNS**

The plan is architecturally sound with minimal boundary violations, but has **two critical placement decisions** that require immediate correction before implementation:

1. **MCP client/server placement violation** — belongs in infra, not runtime
2. **FileLockManager coupling** — needs abstract interface to avoid tooling dependency

---

## DETAILED ANALYSIS

### 1. AgentSpec in theo-domain — VALID

**Finding:** Placing `AgentSpec`, `AgentSpecSource`, `FindingSeverity`, and `IsolationMode` in theo-domain is **correct**.

**Why it works:**
- These are pure value types with zero external dependencies (String, Option, enum, PathBuf)
- `theo-domain` correctly declares only workspace deps: tokio, serde, serde_json, thiserror, async-trait, tempfile
- `IsolationMode` enum with `#[non_exhaustive]` is the right pattern for extensibility
- `AgentFinding` struct is audit-safe: serializable, immutable, no process spawning

**Risk:** None identified. Boundary is clean.

---

### 2. SubAgentRegistry in theo-agent-runtime — VALID

**Finding:** Placing registry, parser, builtins in `theo-agent-runtime` is **correct**.

**Why it works:**
- Registry is responsible for runtime initialization of agent specs (not domain logic)
- Parser (frontmatter YAML) is infrastructure-level concern, not domain
- Loading from `.theo/agents/` and `~/.theo/agents/` is runtime configuration
- `theo-agent-runtime` correctly depends on `theo-domain` only

**Risk:** None identified.

---

### 3. FileLockManager in theo-agent-runtime — CONCERN (Fixable)

**Finding:** File locking is placed in `theo-agent-runtime/src/subagent/file_lock.rs`, but the plan creates **hidden coupling to theo-tooling**.

**Current plan excerpt:**
```rust
// theo-agent-runtime/src/subagent/file_lock.rs
pub struct FileLockManager {
    locked: Mutex<HashSet<PathBuf>>,  // Pure in-process lock
}

// theo-agent-runtime/src/subagent/mod.rs (integration with SubAgentManager)
// acquire locks before spawn
// declare allowed_paths from spec
```

**The Problem:**
- File locking is coordination logic (runtime concern), so placement is correct
- But the plan doesn't specify: **who enforces which files a sub-agent can modify?**
- The spec has `allowed_paths`, but parsing and validating those paths requires:
  - Path canonicalization
  - `.gitignore` awareness (don't lock ignored files)
  - Symlink resolution
  - These are tooling concerns (`theo-tooling` has `walkdir`, `ignore` crates)

**Current Dependency:**
```
theo-agent-runtime → theo-domain, theo-governance
(does NOT depend on theo-tooling)
```

**If the plan adds FileLockManager without abstraction:**
- FileLockManager can stay in agent-runtime (coordination logic)
- But **path validation** needs an abstract interface
- Without abstraction, you'll add `theo-tooling` dependency to agent-runtime
- This violates the boundary: agent-runtime should not depend on tooling

**Recommendation:**
```rust
// Define in theo-domain (or theo-governance)
pub trait PathValidator: Send + Sync {
    fn validate_and_resolve(&self, paths: &[PathBuf]) -> Result<Vec<PathBuf>, PathError>;
}

// theo-agent-runtime uses the trait
impl SubAgentManager {
    pub fn new(
        ...,
        path_validator: Arc<dyn PathValidator>,
    ) { }
}

// theo-application or theo-tooling implements PathValidator
pub struct ToolingPathValidator { }
impl PathValidator for ToolingPathValidator { }
```

**Status:** FIXABLE — add one abstract trait, no structural change needed.

---

### 4. MCP Client/Server in theo-agent-runtime — VIOLATION

**Finding:** MCP client and server are proposed in `theo-agent-runtime/src/mcp/` but **MCP is infrastructure, not runtime logic**.

**The Problem:**

From the plan:
```rust
// theo-agent-runtime/src/mcp/client.rs (NOVO)
pub struct McpClient {
    servers: HashMap<String, McpServerConnection>,
}

impl McpClient {
    pub async fn from_config(project_dir: &Path) -> Result<Self, McpError>;
    pub async fn discover_tools(&self) -> Vec<Box<dyn Tool>>;
    pub async fn health_check(&self) -> Vec<ServerHealth>;
}

// theo-agent-runtime/src/mcp/server.rs (NOVO)
pub struct McpServer {
    registry: Arc<ToolRegistry>,
    transport: McpTransport,
}
```

**Why this violates boundaries:**

1. **MCP is protocol infrastructure** — Model Context Protocol is at the same level as:
   - LLM provider clients (`theo-infra-llm`)
   - Authentication (`theo-infra-auth` already has `McpAuth`)
   - Tool sandboxing (`theo-tooling`)

2. **Current crate structure shows pattern:**
   ```
   theo-infra-llm     → LLM client logic
   theo-infra-auth    → Auth protocols (PKCE, OAuth, MCP tokens)
   theo-tooling       → Tool registry, sandbox, execution
   ```

3. **theo-agent-runtime should orchestrate, not implement protocol:**
   ```
   theo-agent-runtime orchestrates:
   - LLM calls via theo-infra-llm
   - Auth via theo-infra-auth
   - Tool execution via theo-tooling
   - Sub-agents via registry/builtins
   ```

4. **Placing MCP in agent-runtime creates:**
   - Coupling of protocol logic to orchestration
   - Violates SRP: agent-runtime becomes responsible for transport
   - Blocks reuse: other components (like CLI) can't use MCP without importing agent-runtime

**Evidence from plan:**
- Config: `.theo/mcp.yaml` (project) + `~/.theo/mcp.yaml` (global) — infrastructure concern
- Health check, reconnect, timeout — all protocol-level concerns
- stdio/HTTP transport — protocol infrastructure
- Auto-discovery of tools — could be shared across multiple orchestrators

**Current theo-infra-auth has MCP tokens:**
```rust
// theo-infra-auth/src/mcp.rs (already exists)
pub struct McpServerAuth { ... }
pub struct McpAuthStore { ... }
```

This suggests MCP infrastructure is already partially split.

**Correct Placement:**

Option 1: Create `theo-infra-mcp` crate (cleanest)
```
theo-infra-mcp
  ├── client.rs (McpClient, discovery, health check)
  ├── server.rs (McpServer, stdio transport)
  ├── config.rs (parse .theo/mcp.yaml)
  └── error.rs

dependencies:
  theo-domain (for traits)
  theo-infra-auth (McpAuth integration)
  tokio, reqwest, serde
```

Option 2: Expand theo-tooling (less clean but acceptable)
```
theo-tooling/src/mcp/
  ├── client.rs
  ├── server.rs
  └── config.rs

dependencies:
  (same as theo-tooling)
```

Option 3: Keep in theo-agent-runtime ONLY IF:
- Remove from `mcp/` module
- Move to `subagent/mcp_integration.rs` (sub-agents only)
- Make clear it's **sub-agent specific**, not general MCP
- But plan says "Codex CLI runs as MCP server" — that's external, not sub-agent specific
- So this violates the intent

**Status:** REJECT current placement. Requires restructuring before implementation.

---

### 5. WorktreeManager in theo-agent-runtime — VALID

**Finding:** Placement in `theo-agent-runtime/src/subagent/isolation.rs` is **correct**.

**Why it works:**
- Worktree isolation is orchestration logic (runtime decision: "should this sub-agent be isolated?")
- Git operations (git worktree add/remove) are executed via bash tool (already in theo-tooling)
- Manager just coordinates lifecycle: create → spawn → destroy
- Pure coordination, zero protocol complexity

**Risk:** None identified.

---

### 6. Circular Dependencies — NONE DETECTED

**Dependency graph after plan:**

```
theo-domain
  └─ (zero deps) ✓

theo-agent-runtime
  ├─ theo-domain ✓
  ├─ theo-governance ✓
  └─ theo-tooling (implicit via FileLockManager path validation)
     └─ theo-domain ✓

theo-infra-mcp (PROPOSED, not in plan yet)
  ├─ theo-domain ✓
  ├─ theo-infra-auth ✓
  └─ external protocols ✓

theo-application
  └─ all above ✓
```

**No circular dependencies detected.** Clean DAG.

---

### 7. TDD Compliance — NOT YET EVALUATED

The plan specifies **7 implementation phases** with tests for each phase:

```bash
cargo test -p theo-domain -- agent_spec
cargo test -p theo-agent-runtime -- registry
cargo test -p theo-agent-runtime -- parser
```

**Status:** Plan shows test intent. Actual TDD compliance will be evaluated during implementation phase.

**Requirement:** Every new function and struct must have at least one test. RED-GREEN-REFACTOR mandatory.

---

## RISK ASSESSMENT

| Risk | Severity | Mitigated By | Action |
|---|---|---|---|
| `theo-agent-runtime` imports `theo-tooling` implicitly | HIGH | Abstract PathValidator trait | Fix before Phase 1 |
| MCP placed in wrong crate | CRITICAL | Create `theo-infra-mcp` or move to `theo-tooling` | FIX BEFORE PHASE 6 |
| FileLockManager deadlock (in-process) | LOW | Depth=1 guarantee, Mutex semantics | Acceptable, documented |
| Worktree cleanup on crash | LOW | `cleanup_stale()` background task | Acceptable, documented |
| Custom agent system prompt injection | LOW | CapabilityGate still enforced | No change needed |
| Frontmatter parser fragility | MEDIUM | Comprehensive tests, graceful skip | Phase 2 coverage |

---

## RECOMMENDATIONS

### Critical (Must Fix)

1. **Extract MCP to separate infra crate**
   - Create `crates/theo-infra-mcp/` OR expand theo-tooling
   - Move `mcp/client.rs` and `mcp/server.rs` there
   - Update Phase 6 plan accordingly
   - Rationale: MCP is a protocol boundary, not orchestration logic

2. **Abstract path validation**
   - Define `PathValidator` trait (in theo-domain or theo-governance)
   - Inject into `FileLockManager::new()`
   - Implement in theo-application layer
   - Prevents accidental theo-tooling dependency in agent-runtime

### High (Should Do)

3. **Add detailed error handling for MCP failures**
   - MCP reconnection strategy (current: 3 retries + backoff)
   - Circuit breaker to prevent infinite retry loops
   - Clear error messages for user-facing CLI output

4. **Extend AgentFinding with more fields**
   - Consider: `agent_name`, `executed_at`, `confidence_score`
   - Matches plan's enrichment goals in Phase 1

### Medium (Nice to Have)

5. **Benchmark FileLockManager contention**
   - Current: in-process Mutex with HashSet
   - Plan assumes depth=1 (no cascading sub-agents)
   - If depth ever increases: switch to fd-lock (already mentioned as future)

6. **Add observability to MCP health checks**
   - Log server connection state changes
   - Emit events for disconnects/reconnects
   - Helps debug IDE integration issues

---

## FINAL CHECKLIST

Before implementation begins:

- [ ] **MCP placement decision made** (theo-infra-mcp vs theo-tooling vs agent-runtime)
- [ ] **PathValidator trait drafted** in theo-domain
- [ ] **AgentSpec TDD tests written** (RED phase)
- [ ] **Cargo.toml dependencies updated** for new crates/traits
- [ ] **Architecture diagram updated** with MCP placement
- [ ] **Plan Phase 6 revised** with correct crate references

---

## CONCLUSION

**The plan is fundamentally sound.** The sub-agent system respects core boundaries:
- Domain types (AgentSpec, IsolationMode) live in theo-domain ✓
- Orchestration logic (registry, file locks, worktree) lives in theo-agent-runtime ✓
- Infrastructure protocols (MCP) need **relocation** to proper infra crate ✗

**Approval is conditional on:**
1. Resolving MCP placement before Phase 6
2. Adding PathValidator abstraction before Phase 3

**Risk level:** Low (for the relocated code; medium for current MCP placement in runtime)

**Estimated effort to fix:** 1-2 hours of restructuring before implementation.

