# Plano: Dynamic Sub-Agent System (Built-in + On-demand + Custom)

## Context

O sistema atual de sub-agents do Theo tem 4 roles hardcoded (`Explorer`, `Implementer`, `Verifier`, `Reviewer`) como um `enum SubAgentRole` em `subagent/mod.rs`. System prompts, capability sets, timeouts e max_iterations estao todos codificados em match arms. O LLM principal invoca sub-agents via meta-tools `subagent` e `subagent_parallel` em `tool_bridge.rs`, que aceitam apenas os 4 roles fixos.

**Problema:** Nao ha como adicionar novos agentes sem recompilar, nem como usuarios definirem agentes customizados para seus projetos. A pesquisa SOTA mostra que Claude Code, OpenDev e Hermes todos suportam agent specs declarativos (markdown/YAML) com discovery automatico.

**Objetivo:** Criar um sistema unificado de sub-agents com 3 fontes:
1. **Built-in** — roles pre-definidos no codigo (os 4 atuais + novos)
2. **Custom (usuario)** — definidos em `.theo/agents/*.md` (projeto) e `~/.theo/agents/*.md` (global)
3. **On-demand (dinamicos)** — criados pelo LLM em runtime via tool `delegate_task`

---

## Evidencias das Referencias

| Referencia | Pattern | O que adotar | O que NAO adotar |
|---|---|---|---|
| **OpenDev** | `SubAgentSpec` com frontmatter YAML + body = system prompt. `SubagentManager::with_builtins_and_custom()` carrega de 3 fontes (built-in, global, projeto). Priority: projeto > global > built-in | Formato de spec, resolution order, builder pattern | Mailbox inter-agent (YAGNI — depth=1 resolve), SimpleReactRunner separado |
| **Claude Code** | Markdown agents em `.claude/agents/`. Frontmatter: name, description, tools, model. Sub-agents retornam summary-only | Formato markdown, return-only isolation, model override per agent | Agent Teams/Swarm (scope futuro, nao agora) |
| **Hermes** | `delegate_task` tool com schema: goal+context+toolsets. `_build_child_system_prompt()` simples: task + context + workspace. Heartbeat loop. Blocked tools list | Tool schema para on-demand, blocked tools pattern, credential routing | Heartbeat (nao temos gateway timeout), ThreadPoolExecutor (ja temos tokio JoinSet) |
| **Archon** | Workflows YAML com node types. Worktree isolation por conversa. Variable substitution | Worktree isolation concept (futuro) | DAG executor (over-engineering para sub-agents) |
| **Anthropic SDK** | Agent-as-tool. Opus lead + Sonnet workers. 15x token cost | Model routing per role (Theo ja tem `RoutingPhase::Subagent { role }`) | Managed Agents cloud (nao aplicavel) |
| **Aider** | Architect/Editor dual-model. Reasoning + execution separados | Confirma que model routing per role e high-ROI | Dual-model fixo (Theo permite N roles) |
| **SWE-Agent** | ACI design > model selection. 100-line window, max 50 search hits | Tool output truncation (Theo ja tem `TruncationRule`) | Single-agent (nao multi-agent) |

### Dado chave da pesquisa

> **98.4% do Claude Code e infraestrutura deterministica, nao logica de AI.** Apenas 1.6% e decisao do modelo (arXiv 2604.14228). A vantagem competitiva vem da qualidade do harness, nao do agent loop.

---

## Arquitetura Proposta

### Novo tipo central: `AgentSpec`

Substitui o `SubAgentRole` enum hardcoded por um struct flexivel em `theo-domain`:

```rust
// theo-domain/src/agent_spec.rs (NOVO)

pub struct AgentSpec {
    pub name: String,                           // ID unico (e.g. "explorer", "my-reviewer")
    pub description: String,                    // Human-readable (para tool schema)
    pub system_prompt: String,                  // Body do markdown
    pub capability_set: CapabilitySet,          // Tools permitidas/negadas
    pub model_override: Option<String>,         // Override de modelo
    pub max_iterations: usize,                  // Loop limit
    pub timeout_secs: u64,                      // Wall-clock timeout
    pub source: AgentSpecSource,                // Builtin | Project | Global | OnDemand
}

pub enum AgentSpecSource {
    Builtin,    // Hardcoded no codigo
    Project,    // .theo/agents/*.md
    Global,     // ~/.theo/agents/*.md  
    OnDemand,   // Criado pelo LLM em runtime
}
```

### SubAgentRegistry (novo, substitui logica em SubAgentManager)

```rust
// theo-agent-runtime/src/subagent/registry.rs (NOVO)

pub struct SubAgentRegistry {
    specs: HashMap<String, AgentSpec>,
}

impl SubAgentRegistry {
    pub fn new() -> Self;
    pub fn with_builtins() -> Self;                              // 4 built-in
    pub fn load_custom(project_dir: &Path) -> Vec<AgentSpec>;    // .theo/agents/
    pub fn load_global() -> Vec<AgentSpec>;                      // ~/.theo/agents/
    pub fn with_all(project_dir: &Path) -> Self;                 // built-in + global + project
    pub fn register(&mut self, spec: AgentSpec);                 // On-demand
    pub fn get(&self, name: &str) -> Option<&AgentSpec>;
    pub fn names(&self) -> Vec<&str>;                            // Para tool schema enum
    pub fn build_tool_description(&self) -> String;              // Para meta-tool description
}
```

**Resolution order (evidencia: OpenDev `with_builtins_and_custom_inner`):**
1. Built-in (baseline)
2. Global `~/.theo/agents/*.md` (user override)
3. Project `.theo/agents/*.md` (project override — highest priority)

Specs com mesmo `name` sao sobrescritos (ultimo ganha).

### Formato Markdown para Custom Agents

```markdown
---
name: security-reviewer
description: "Reviews code for OWASP Top 10 vulnerabilities"
tools:
  - read
  - grep
  - glob
  - bash
denied_tools:
  - edit
  - write
  - apply_patch
model: claude-sonnet-4-7
max_iterations: 25
timeout: 300
---

You are a security-focused code reviewer. Your job is to find vulnerabilities.

Focus on:
- SQL injection, XSS, CSRF
- Hardcoded credentials
- Unsafe deserialization
- Path traversal

Report findings with severity: CRITICAL, HIGH, MEDIUM, LOW.
NEVER edit files. Only analyze and report.
```

**Frontmatter fields (evidencia: OpenDev `SubAgentSpec` + Claude Code agents):**

| Campo | Tipo | Default | Descricao |
|---|---|---|---|
| `name` | string | filename sem extensao | ID unico |
| `description` | string | obrigatorio | Para tool schema |
| `tools` | string[] | [] (= all) | Allowed tools |
| `denied_tools` | string[] | [] | Denied tools (precedencia) |
| `model` | string | None (herda parent) | Model override |
| `max_iterations` | u32 | 30 | Loop limit |
| `timeout` | u32 | 300 | Seconds |

Body (apos `---`) = system prompt.

### Meta-tool `delegate_task` (substitui `subagent` + `subagent_parallel`)

**Evidencia: Hermes `delegate_task` schema + Claude Code `subagent`**

Nova tool unificada que suporta os 3 modos:

```json
{
  "name": "delegate_task",
  "parameters": {
    "agent": "explorer | implementer | ... | <custom-name>",
    "objective": "What the agent should accomplish",
    "context": "Optional background info, file paths, constraints",
    "parallel": [
      {"agent": "explorer", "objective": "..."},
      {"agent": "verifier", "objective": "..."}
    ]
  }
}
```

**Modos de uso:**
1. **Named agent** (built-in ou custom): `agent` = nome registrado
2. **On-demand**: `agent` = nome nao registrado → cria `AgentSpec::on_demand()` com defaults
3. **Parallel**: `parallel` array com multiplos agents

**Schema dinamico:** A description da tool lista os agents disponiveis (gerada por `registry.build_tool_description()`), atualizada quando registry muda.

### Mudancas no SubAgentManager

```rust
// subagent/mod.rs — REFATORADO

pub struct SubAgentManager {
    config: AgentConfig,
    event_bus: Arc<EventBus>,
    project_dir: PathBuf,
    registry: Arc<SubAgentRegistry>,  // NOVO
    depth: usize,
}

impl SubAgentManager {
    pub fn new(
        config: AgentConfig,
        event_bus: Arc<EventBus>,
        project_dir: PathBuf,
        registry: Arc<SubAgentRegistry>,  // NOVO
    ) -> Self;

    // spawn agora recebe &AgentSpec em vez de SubAgentRole
    pub fn spawn(
        &self,
        spec: &AgentSpec,
        objective: &str,
        context: Option<&str>,          // NOVO: context string
    ) -> Pin<Box<dyn Future<Output = AgentResult> + Send + '_>>;

    pub async fn spawn_parallel(
        &self,
        tasks: Vec<(&AgentSpec, String, Option<String>)>,
    ) -> Vec<AgentResult>;
}
```

### Model Routing per Agent

**Evidencia: Aider Architect/Editor, Anthropic Opus+Sonnet, OpenDev LLM bindings**

`SubAgentRoleId` ja existe em `theo-domain/src/routing.rs` como `Cow<'static, str>`. Ele suporta IDs dinamicos naturalmente:

```rust
// Ja funciona — zero mudancas no domain:
SubAgentRoleId(Cow::Owned("security-reviewer".to_string()))
```

O `ModelRouter::route()` recebe `RoutingPhase::Subagent { role }` e ja pode despachar por role_id. Basta que custom agents gerem `SubAgentRoleId` a partir do `spec.name`.

### AgentResult Enriquecido

**Evidencia: Hermes tool_trace, Anthropic structured results**

```rust
// agent_loop.rs — ESTENDER AgentResult

pub struct AgentResult {
    // ... campos existentes ...
    pub agent_name: String,              // NOVO: qual agent executou
    pub context_used: Option<String>,    // NOVO: context passado
}
```

### Structured Results Completo

**Evidencia: Hermes `tool_trace` com status/args_bytes/result_bytes, Anthropic SDK recomenda JSON**

```rust
// theo-domain/src/agent_spec.rs — junto com AgentSpec

/// Structured finding from a sub-agent execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentFinding {
    pub severity: FindingSeverity,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
    pub category: Option<String>,       // e.g. "security", "performance", "bug"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}
```

```rust
// agent_loop.rs — AgentResult completo

pub struct AgentResult {
    // ... campos existentes ...
    pub agent_name: String,                     // NOVO: qual agent executou
    pub context_used: Option<String>,           // NOVO: context passado
    pub findings: Vec<AgentFinding>,            // NOVO: structured findings
}
```

O `summary` continua existindo como fallback human-readable. `findings` e parseado do output do sub-agent quando possivel (pattern matching em linhas com severity markers). Para sub-agents que nao emitem findings estruturados, `findings` fica vazio — zero breaking change.

### File Locking para Sub-Agents Paralelos

**Evidencia: OpenDev `file_locks.rs` (fd-lock + sidecar .lock), OpenCode flock (heartbeat + stale detection), Claude Code Agent Teams (file locking built-in)**

```rust
// theo-agent-runtime/src/subagent/file_lock.rs (NOVO)

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

/// Advisory file lock manager for parallel sub-agent execution.
///
/// Sub-agents declare intent-to-modify files BEFORE starting.
/// Manager rejects conflicts at declaration time (fail-fast).
/// NOT OS-level locking — in-process coordination via shared Arc<FileLockManager>.
///
/// Evidence: OpenDev fd-lock sidecar pattern + OpenCode heartbeat flock.
/// Simplified for depth=1 (all sub-agents share parent process).
pub struct FileLockManager {
    locked: Mutex<HashSet<PathBuf>>,
}

impl FileLockManager {
    pub fn new() -> Self;

    /// Attempt to lock files for exclusive write access.
    /// Returns Ok(FileLockGuard) if all files are available.
    /// Returns Err with conflicting paths if any file is already locked.
    pub fn acquire(&self, paths: &[PathBuf]) -> Result<FileLockGuard, FileLockConflict>;

    /// Check if a file is currently locked (read-only query).
    pub fn is_locked(&self, path: &Path) -> bool;
}

/// RAII guard — releases locks on drop.
pub struct FileLockGuard {
    manager: Arc<FileLockManager>,
    paths: Vec<PathBuf>,
}

impl Drop for FileLockGuard {
    fn drop(&mut self) {
        // Release all locked paths
    }
}

pub struct FileLockConflict {
    pub requested: Vec<PathBuf>,
    pub conflicting: Vec<PathBuf>,
}
```

**Integracao com SubAgentManager:**
- `spawn_parallel()` recebe `Arc<FileLockManager>` compartilhado
- Implementer sub-agents declaram `allowed_paths` do spec como intent
- Se conflito detectado → sub-agent recebe erro antes de iniciar (fail-fast)
- Read-only agents (Explorer, Reviewer) nao precisam de lock

**Simplificacao deliberada vs OpenCode:** Nao usamos heartbeat/stale detection porque todos os sub-agents rodam no mesmo processo tokio (depth=1). Advisory lock in-process com `Mutex<HashSet>` e suficiente. Se futuramente tivermos sub-agents cross-process, migramos para fd-lock.

### Worktree Isolation

**Evidencia: Archon `IIsolationProvider` com 7-step resolution, cleanup background, branch naming. Claude Code Agent Teams usa worktree isolation.**

```rust
// theo-agent-runtime/src/subagent/isolation.rs (NOVO)

/// Isolation mode for sub-agent execution.
///
/// Evidence: Archon 7-step resolution + cleanup service.
/// Theo starts with None (default) and Worktree.
/// Container/VM are future extensions (non_exhaustive).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum IsolationMode {
    /// Shared working directory (current behavior). Default.
    None,
    /// Git worktree — isolated copy of the repo.
    /// Sub-agent operates on a temporary branch.
    Worktree {
        /// Base branch to create worktree from. None = current HEAD.
        base_branch: Option<String>,
    },
}

/// Manages git worktree lifecycle for isolated sub-agents.
pub struct WorktreeManager {
    project_dir: PathBuf,
    max_worktrees: usize,          // Default: 10
}

impl WorktreeManager {
    pub fn new(project_dir: PathBuf) -> Self;

    /// Create an isolated worktree for a sub-agent.
    /// Branch name: theo/agent-{agent_name}-{short_hash}
    /// Path: {project_dir}/.theo/worktrees/{branch_name}/
    pub async fn create(
        &self,
        agent_name: &str,
        base_branch: Option<&str>,
    ) -> Result<WorktreeEnv, WorktreeError>;

    /// Destroy a worktree. Best-effort, never panics.
    /// If worktree has uncommitted changes, returns warning (does NOT force delete).
    pub async fn destroy(&self, env: &WorktreeEnv) -> DestroyResult;

    /// Cleanup stale worktrees (no activity for > 1 hour).
    pub async fn cleanup_stale(&self) -> Vec<DestroyResult>;

    /// List active worktrees.
    pub async fn list(&self) -> Vec<WorktreeEnv>;
}

pub struct WorktreeEnv {
    pub path: PathBuf,
    pub branch_name: String,
    pub created_at: std::time::Instant,
}

pub struct DestroyResult {
    pub worktree_removed: bool,
    pub branch_deleted: bool,
    pub warnings: Vec<String>,
}
```

**Integracao com AgentSpec:**
```rust
// agent_spec.rs — campo adicional
pub struct AgentSpec {
    // ... campos existentes ...
    pub isolation: IsolationMode,       // Default: None
}
```

**Frontmatter:**
```yaml
---
name: risky-implementer
isolation: worktree
---
```

**Integracao com SubAgentManager:**
- Se `spec.isolation == Worktree { .. }`:
  1. `WorktreeManager::create()` antes de `spawn()`
  2. `sub_config.project_dir` aponta para o worktree
  3. `WorktreeManager::destroy()` apos `spawn()` retornar
  4. Se sub-agent editou arquivos: retorna `worktree_path` no `AgentResult` para o parent decidir se faz merge
- Se `spec.isolation == None`: comportamento atual (zero mudanca)

**Safety (Archon rules):**
- NUNCA `git clean -fd`
- NUNCA `git checkout --force` no worktree
- Se tem uncommitted changes, retorna warning (nao deleta)
- Usa `git worktree add` / `git worktree remove` (confia nos guardrails do git)

### MCP Integration para Sub-Agents

**Evidencia: OpenDev `McpManager` com health monitoring + schema caching. Hermes `mcp_tool.py` com stdio/HTTP transport + auto-reconnect. Theo ja tem `McpAuth` em `theo-infra-auth/src/mcp.rs`. Codex CLI roda como MCP server.**

#### Parte A: MCP Client — Sub-agents consomem MCP servers

```rust
// theo-agent-runtime/src/mcp/client.rs (NOVO)

/// MCP client that discovers and invokes tools from external MCP servers.
///
/// Evidence: OpenDev McpManager + Hermes mcp_tool.py
/// Transport: stdio (subprocess) or HTTP (remote server)
pub struct McpClient {
    servers: HashMap<String, McpServerConnection>,
}

impl McpClient {
    /// Load MCP server configs from project + global config.
    /// Config path: .theo/mcp.yaml (project) + ~/.theo/mcp.yaml (global)
    pub async fn from_config(project_dir: &Path) -> Result<Self, McpError>;

    /// Discover tools from all connected servers.
    /// Returns Tool implementations that can be registered in ToolRegistry.
    pub async fn discover_tools(&self) -> Vec<Box<dyn Tool>>;

    /// Health check all servers. Unhealthy servers are restarted (max 3 retries).
    pub async fn health_check(&self) -> Vec<ServerHealth>;
}

/// Configuration format (.theo/mcp.yaml)
/// ```yaml
/// servers:
///   filesystem:
///     command: "npx"
///     args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
///     timeout: 120
///   remote:
///     url: "https://my-mcp.example.com/mcp"
///     headers:
///       Authorization: "Bearer ${MCP_API_KEY}"
/// ```
```

**Integracao com sub-agents:**
- MCP tools sao registrados no `ToolRegistry` como tools normais
- Sub-agents herdam MCP tools do parent (mesma ToolRegistry)
- `CapabilityGate` controla quais MCP tools cada agent pode usar
- Nenhuma mudanca no sub-agent protocol — MCP tools sao transparentes

#### Parte B: MCP Server — Theo como MCP server

```rust
// theo-agent-runtime/src/mcp/server.rs (NOVO)

/// Expose Theo's tools as an MCP server.
///
/// Evidence: Codex CLI dual-mode (agent + MCP server), fff.nvim rmcp server.
/// Transport: stdio (default, for IDE integration)
pub struct McpServer {
    registry: Arc<ToolRegistry>,
    transport: McpTransport,
}

impl McpServer {
    /// Start MCP server on stdio.
    /// Exposes all non-deferred tools from ToolRegistry.
    pub async fn serve_stdio(registry: Arc<ToolRegistry>) -> Result<(), McpError>;

    /// Handle JSON-RPC request.
    /// Dispatches to ToolRegistry for tool execution.
    async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse;
}
```

**CLI integration:**
```bash
# Theo como agent (default)
theo-cli "fix the bug"

# Theo como MCP server (novo)
theo-cli --mcp-server
# → IDE ou outro agent conecta via stdio e chama tools do Theo
```

---

## Arquivos a Modificar

### Novos arquivos

| Arquivo | Descricao |
|---|---|
| `crates/theo-domain/src/agent_spec.rs` | `AgentSpec`, `AgentSpecSource`, `AgentFinding`, `FindingSeverity`, `IsolationMode` |
| `crates/theo-agent-runtime/src/subagent/registry.rs` | `SubAgentRegistry` — registration, lookup, loading |
| `crates/theo-agent-runtime/src/subagent/parser.rs` | Markdown frontmatter parser |
| `crates/theo-agent-runtime/src/subagent/builtins.rs` | 4 built-in specs (extraidos de `mod.rs`) |
| `crates/theo-agent-runtime/src/subagent/file_lock.rs` | `FileLockManager`, `FileLockGuard`, `FileLockConflict` |
| `crates/theo-agent-runtime/src/subagent/isolation.rs` | `WorktreeManager`, `WorktreeEnv`, `DestroyResult` |
| `crates/theo-agent-runtime/src/mcp/mod.rs` | Module root para MCP client + server |
| `crates/theo-agent-runtime/src/mcp/client.rs` | `McpClient` — consume external MCP servers, discovers tools |
| `crates/theo-agent-runtime/src/mcp/server.rs` | `McpServer` — expose Theo tools via MCP stdio |

### Arquivos modificados

| Arquivo | Mudanca |
|---|---|
| `crates/theo-domain/src/lib.rs` | `pub mod agent_spec;` |
| `crates/theo-domain/src/routing.rs` | Nenhuma — `SubAgentRoleId(Cow::Owned)` ja funciona |
| `crates/theo-agent-runtime/src/subagent/mod.rs` | `SubAgentManager` recebe `Arc<SubAgentRegistry>` + `Arc<FileLockManager>`, `spawn()` recebe `&AgentSpec`, remove `SubAgentRole` enum |
| `crates/theo-agent-runtime/src/tool_bridge.rs` | Substituir `subagent`/`subagent_parallel` por `delegate_task` com schema dinamico |
| `crates/theo-agent-runtime/src/config.rs` | Nenhuma mudanca necessaria |
| `crates/theo-agent-runtime/src/run_engine.rs` | Dispatch de `delegate_task` no lugar de `subagent`/`subagent_parallel` |
| `crates/theo-agent-runtime/src/agent_loop.rs` | `AgentResult` ganha `agent_name`, `context_used`, `findings`. AgentLoop recebe registry |
| `crates/theo-agent-runtime/src/lib.rs` | `pub mod mcp;` |
| `apps/theo-cli/src/main.rs` | Flag `--mcp-server` para modo MCP server |

---

## Fases de Implementacao

### Fase 1: Domain types + Registry (sem quebrar nada)

1. Criar `theo-domain/src/agent_spec.rs` com `AgentSpec`, `AgentSpecSource`, `AgentFinding`, `FindingSeverity`, `IsolationMode`
2. Criar `subagent/builtins.rs` — converter os 4 roles hardcoded para `AgentSpec`
3. Criar `subagent/registry.rs` — `SubAgentRegistry` com `with_builtins()` + `get()` + `register()`
4. Testes unitarios para cada tipo

### Fase 2: Markdown parser + Custom loading

1. Criar `subagent/parser.rs` — parse frontmatter YAML + body
2. Implementar `SubAgentRegistry::load_custom()` e `load_global()`
3. Implementar `SubAgentRegistry::with_all(project_dir)`
4. Testes: parse valido, parse invalido, resolution order, override por nome

### Fase 3: Refatorar SubAgentManager + File Locking

1. Criar `subagent/file_lock.rs` — `FileLockManager` com `Mutex<HashSet>`
2. `SubAgentManager::new()` recebe `Arc<SubAgentRegistry>` + `Arc<FileLockManager>`
3. `spawn()` recebe `&AgentSpec` em vez de `SubAgentRole`, com `context: Option<&str>`
4. `spawn_parallel()` adquire file locks antes de spawnar (fail-fast em conflito)
5. `AgentResult` ganha `agent_name`, `context_used`, `findings`
6. Manter backward compat temporaria: `SubAgentRole` delega para registry lookup
7. Testes: file lock acquire/release, conflito, RAII guard, parallel com lock

### Fase 4: Nova meta-tool `delegate_task`

1. Substituir `subagent` + `subagent_parallel` por `delegate_task` em `tool_bridge.rs`
2. Schema dinamico com lista de agents do registry
3. On-demand mode: nome nao encontrado → cria `AgentSpec::on_demand(name, objective)`
4. Atualizar dispatch em `run_engine.rs`
5. Atualizar `registry_to_definitions()` e `registry_to_definitions_for_subagent()`
6. Testes: dispatch named, dispatch custom, dispatch on-demand, dispatch parallel

### Fase 5: Worktree Isolation

1. Criar `subagent/isolation.rs` — `WorktreeManager` com create/destroy/cleanup
2. Integrar com `SubAgentManager::spawn()`: se `spec.isolation == Worktree`, criar worktree antes, destruir depois
3. `AgentResult` ganha `worktree_path: Option<PathBuf>` para sub-agents isolados
4. `AgentSpec` frontmatter suporta `isolation: worktree`
5. Testes: create worktree, destroy clean, destroy with changes (warning), cleanup stale
6. Safety: nunca `git clean -fd`, nunca force delete

### Fase 6: MCP Integration

1. Criar `mcp/client.rs` — `McpClient` com stdio/HTTP transport, tool discovery, health check
2. Criar `mcp/server.rs` — `McpServer` expoe ToolRegistry via stdio JSON-RPC
3. MCP tools registrados no `ToolRegistry` como tools normais (transparente para sub-agents)
4. Config: `.theo/mcp.yaml` (projeto) + `~/.theo/mcp.yaml` (global)
5. CLI: `theo-cli --mcp-server` para modo servidor
6. Testes: discover tools from mock MCP server, serve tools via mock stdio, health check

### Fase 7: Cleanup + Integration Final

1. Remover `SubAgentRole` enum (agora redundante)
2. Atualizar system prompt do agent principal com novos agents
3. Integration test end-to-end: custom agent + file lock + worktree + MCP
4. Atualizar CHANGELOG

---

## Invariantes Preservados

- **depth=1** — sub-agents NAO spawnam sub-agents (sem mudanca)
- **return-only** — sub-agents retornam `AgentResult` ao parent (sem mudanca)
- **EventBus forwarding** — `PrefixedEventForwarder` tageia eventos por `spec.name` (sem mudanca)
- **CapabilityGate** — continua funcionando, agora alimentado por `spec.capability_set` (sem mudanca)
- **`is_subagent = true`** — continua bloqueando meta-tools de delegacao (sem mudanca)
- **Budget enforcement** — tokens do sub-agent contam para o parent (sem mudanca)
- **Dependency direction** — `AgentSpec` vive em `theo-domain` (zero deps), registry e infra vivem em `theo-agent-runtime`
- **Worktree safety** — git guardrails respeitados, nunca force-delete, uncommitted changes geram warning

---

## Riscos e Mitigacoes

| Risco | Mitigacao |
|---|---|
| Custom agent com system prompt malicioso | CapabilityGate continua enforcing. Sem tool: sem dano. Denied tools tem precedencia |
| Parser de frontmatter fragil | Testes extensivos. Frontmatter invalido → skip com warning (pattern OpenDev) |
| Breaking change na tool API | `delegate_task` substitui `subagent` + `subagent_parallel` atomicamente. Nao ha API publica externa |
| Performance de loading | Lazy: so parseia `.theo/agents/` uma vez no startup. Registry e `Arc` — clonavel sem custo |
| Worktree leak (crash antes de cleanup) | `cleanup_stale()` roda no startup e periodicamente. Worktrees sem atividade por >1h sao removidos |
| Worktree com uncommitted changes | NUNCA force-delete. Retorna warning. Parent decide se faz merge ou descarta |
| File lock deadlock | Impossivel — depth=1, locks sao in-process (`Mutex`), sub-agents nao se bloqueiam mutuamente (fail-fast na aquisicao) |
| MCP server malicioso | MCP tools passam pelo mesmo CapabilityGate. Timeout per-tool (default 120s). Health check desconecta servers instáveis |
| MCP server crash | Auto-reconnect com backoff (3 tentativas). Apos 3 falhas consecutivas, marca como unhealthy e desabilita |

---

## Verificacao

```bash
# Fase 1-2: domain types, registry, parser
cargo test -p theo-domain -- agent_spec
cargo test -p theo-agent-runtime -- registry
cargo test -p theo-agent-runtime -- parser
cargo test -p theo-agent-runtime -- builtins

# Fase 3: SubAgentManager + file locking
cargo test -p theo-agent-runtime -- file_lock
cargo test -p theo-agent-runtime -- subagent

# Fase 4: delegate_task meta-tool
cargo test -p theo-agent-runtime -- delegate_task

# Fase 5: worktree isolation
cargo test -p theo-agent-runtime -- isolation
cargo test -p theo-agent-runtime -- worktree

# Fase 6: MCP
cargo test -p theo-agent-runtime -- mcp

# Smoke test: custom agent
mkdir -p .theo/agents
cat > .theo/agents/test-agent.md << 'EOF'
---
name: test-explorer
description: "Test agent for validation"
denied_tools:
  - edit
  - write
max_iterations: 5
timeout: 60
---
You are a test agent. Read one file and call done with a summary.
EOF

# Smoke test: MCP server mode
theo-cli --mcp-server &
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | nc localhost 19877
kill %1

# Build completo
cargo build
cargo clippy -- -D warnings
cargo test  # all workspace
```

## Gap Coverage Matrix

| Gap | Severidade | Fase | Status |
|---|---|---|---|
| Custom agent specs (markdown) | Alta | Fase 1-2 | COBERTO |
| Model routing per role ATIVO | Alta | Fase 1,3 | COBERTO |
| delegate_task tool (on-demand) | Alta | Fase 4 | COBERTO |
| Context passado ao sub-agent | Media | Fase 3-4 | COBERTO |
| Structured results | Media | Fase 1,3 | COBERTO (AgentFinding + FindingSeverity) |
| File locking para parallel | Media | Fase 3 | COBERTO (FileLockManager advisory) |
| MCP integration (client) | Media | Fase 6 | COBERTO (McpClient + tool discovery) |
| Agent-as-MCP-server | Baixa | Fase 6 | COBERTO (McpServer + --mcp-server flag) |
| Worktree isolation | Baixa | Fase 5 | COBERTO (WorktreeManager + IsolationMode) |

---

## Referencias da Pesquisa

| # | Fonte | URL |
|---|---|---|
| 1 | Claude Code Docs — Agent Teams | https://code.claude.com/docs/en/agent-teams |
| 2 | arXiv 2604.14228 — Dive into Claude Code | https://arxiv.org/abs/2604.14228 |
| 3 | OpenAI Codex Subagents | https://developers.openai.com/codex/subagents |
| 4 | Anthropic — Multi-Agent Research System | https://www.anthropic.com/engineering/multi-agent-research-system |
| 5 | Aider — Architect Mode | https://aider.chat/2024/09/26/architect.html |
| 6 | Google A2A Protocol | https://developers.googleblog.com/en/a2a-a-new-era-of-agent-interoperability/ |
| 7 | arXiv — OpenSage | https://arxiv.org/html/2602.16891v1 |

### Projetos locais analisados (`referencias/`)

- **hermes-agent** — `tools/delegate_tool.py` (1200 lines): delegate_task tool, child construction, heartbeat, interrupt propagation
- **opendev** — `crates/opendev-agents/src/subagents/`: SubAgentSpec, custom_loader, runner trait, mailbox, permission system
- **Archon** — `packages/core/src/orchestrator/`: DAG workflows, worktree isolation, prompt builder
- **opencode** — `AGENTS.md`: build/plan/general 3-agent system
- **pi-mono** — Multi-provider LLM API, agent-core, coding-agent

### Relatorio completo da pesquisa SOTA

- `outputs/reports/sota-subagent-architectures.md` — 47 fontes, 8 production systems, 7 frameworks, gap analysis
- `outputs/insights/insight-orchestrator-worker-dominant.md`
- `outputs/insights/insight-model-routing-per-role.md`
- `outputs/insights/insight-infrastructure-over-ai.md`
- `outputs/insights/insight-mcp-a2a-convergence.md`
