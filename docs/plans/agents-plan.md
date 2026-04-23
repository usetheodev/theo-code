# Plano: Dynamic Sub-Agent System (Built-in + Custom + On-demand)

> **Versao 2.0** — Revisado apos reuniao 20260423-021723 (16 agentes, veredito REVISED).
> Escopo reduzido de 7 para 4 fases. Seguranca reforcada. TDD detalhado.

## Context

O sistema atual de sub-agents do Theo tem 4 roles hardcoded (`Explorer`, `Implementer`, `Verifier`, `Reviewer`) como um `enum SubAgentRole` em `subagent/mod.rs` (463 linhas). System prompts, capability sets, timeouts e max_iterations estao todos codificados em match arms. O LLM principal invoca sub-agents via meta-tools `subagent` e `subagent_parallel` em `tool_bridge.rs`, que aceitam apenas os 4 roles fixos.

**Problema:** Nao ha como adicionar novos agentes sem recompilar, nem como usuarios definirem agentes customizados para seus projetos.

**Objetivo:** Criar um sistema unificado de sub-agents com 3 fontes:
1. **Built-in** — roles pre-definidos no codigo (os 4 atuais + novos)
2. **Custom (usuario)** — definidos em `.theo/agents/*.md` (projeto) e `~/.theo/agents/*.md` (global)
3. **On-demand (dinamicos)** — criados pelo LLM em runtime via tool `delegate_task`

**Escopo EXCLUIDO (epicos futuros):**
- File Locking para sub-agents paralelos — nenhum cenario de conflito existe com depth=1
- Worktree Isolation (git worktrees) — nenhum usuario solicitou, baixa prioridade
- MCP Integration (client + server) — escopo separado, pertence a `theo-infra-mcp`
- AgentFinding / FindingSeverity — especulativo; sera desenhado quando houver dados reais ("measure before schema")

---

## Evidencias das Referencias

| Referencia | Pattern | O que adotar | O que NAO adotar |
|---|---|---|---|
| **OpenDev** | `SubAgentSpec` com frontmatter YAML + body = system prompt. `SubagentManager::with_builtins_and_custom()` carrega de 3 fontes. Priority: projeto > global > built-in | Formato de spec, resolution order, builder pattern | Mailbox inter-agent (YAGNI — depth=1 resolve), SimpleReactRunner separado |
| **Claude Code** | Markdown agents em `.claude/agents/`. Frontmatter: name, description, tools, model. Sub-agents retornam summary-only | Formato markdown, return-only isolation, model override per agent | Agent Teams/Swarm (scope futuro) |
| **Hermes** | `delegate_task` tool com schema: goal+context+toolsets. `_build_child_system_prompt()`: task + context + workspace. Blocked tools list | Tool schema para on-demand, blocked tools pattern | Heartbeat (nao temos gateway timeout) |
| **Anthropic SDK** | Agent-as-tool. Opus lead + Sonnet workers. 15x token cost | Model routing per role (Theo ja tem `RoutingPhase::Subagent { role }`) | Managed Agents cloud (nao aplicavel) |
| **Aider** | Architect/Editor dual-model. Reasoning + execution separados | Confirma que model routing per role e high-ROI | Dual-model fixo (Theo permite N roles) |
| **Google ADK** | Hierarchical agent tree, A2A protocol | Confirms declarative agent specs are the emerging standard | A2A protocol (premature for local sub-agents) |
| **OpenAI Agents SDK** | Typed handoffs, three-tier guardrails | Validates guardrail pattern for capability restriction | Cloud-native handoffs (not applicable) |

### Dado chave

> **98.4% do Claude Code e infraestrutura deterministica, nao logica de AI** (arXiv 2604.14228). A vantagem competitiva vem da qualidade do harness — o que justifica investir na infraestrutura de AgentSpec/Registry, mas NAO justifica construir tudo de uma vez.

---

## Decisoes de Seguranca (Reuniao 20260423)

### S1: On-demand agents — CapabilitySet::read_only() por default

`AgentSpec::on_demand()` DEVE usar `CapabilitySet::read_only()` como default. O LLM NAO pode escalar capabilities via on-demand. Agentes com capabilities de escrita exigem spec registrado (builtin, global, ou project).

**Justificativa:** Sem esta restricao, o LLM pode criar agentes arbitrarios com acesso total a bash/edit/write, bypasando o CapabilityGate.

### S2: Override de builtins — intersecao, nunca escalacao

Quando um project/global agent tem o mesmo nome de um builtin, o `CapabilitySet` resultante e a INTERSECAO do builtin com o custom. Um `.theo/agents/explorer.md` pode RESTRINGIR o Explorer (remover tools), nunca ampliar (adicionar tools que o builtin nao tinha).

**Justificativa:** Previne supply-chain attack via `.theo/agents/` em repos clonados.

**Implementacao:** `CapabilitySet::intersect(&self, other: &CapabilitySet) -> CapabilitySet` — novo metodo. `denied_tools` = uniao; `allowed_tools` = intersecao (se ambos nao-vazios); `network_access` = AND.

### S3: User confirmation para project agents (primeira carga)

Na primeira vez que `.theo/agents/` de um projeto e carregado, exibir warning listando agents encontrados e pedir confirmacao. Persistir confirmacao em `.theo/.agents-approved`.

---

## Arquitetura Proposta

### Novo tipo central: `AgentSpec`

Vive em `theo-domain` (pure value type, zero deps). Substitui o `SubAgentRole` enum hardcoded.

```rust
// theo-domain/src/agent_spec.rs (NOVO)

use serde::{Deserialize, Serialize};
use crate::capability::CapabilitySet;
use crate::routing::SubAgentRoleId;
use std::borrow::Cow;

/// Source of an agent specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AgentSpecSource {
    Builtin,    // Hardcoded no codigo
    Project,    // .theo/agents/*.md
    Global,     // ~/.theo/agents/*.md  
    OnDemand,   // Criado pelo LLM em runtime
}

/// Specification for a sub-agent.
///
/// Pure value type — no I/O, no async, no runtime deps.
/// Loaded from markdown frontmatter (custom) or constructed in code (builtin/on-demand).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub name: String,                           // ID unico (e.g. "explorer", "my-reviewer")
    pub description: String,                    // Human-readable (para tool schema)
    pub system_prompt: String,                  // Body do markdown
    pub capability_set: CapabilitySet,          // Tools permitidas/negadas
    pub model_override: Option<String>,         // Override de modelo
    pub max_iterations: usize,                  // Loop limit
    pub timeout_secs: u64,                      // Wall-clock timeout
    pub source: AgentSpecSource,                // Origem da spec
}

impl AgentSpec {
    /// Bridge to routing: generates SubAgentRoleId from spec name.
    /// Enables model routing per agent via RoutingPhase::Subagent { role }.
    pub fn role_id(&self) -> SubAgentRoleId {
        SubAgentRoleId(Cow::Owned(self.name.clone()))
    }

    /// Create an on-demand agent with RESTRICTED defaults.
    /// Security: on-demand agents are read-only by default (decisao S1).
    pub fn on_demand(name: &str, objective: &str) -> Self {
        Self {
            name: name.to_string(),
            description: format!("On-demand agent: {}", objective),
            system_prompt: format!(
                "You are an on-demand sub-agent. Your objective:\n{}\n\n\
                 You have READ-ONLY access. Analyze and report findings.\n\
                 NEVER attempt to edit, write, or execute commands.",
                objective
            ),
            capability_set: CapabilitySet::read_only(),  // SEGURANCA: read-only
            model_override: None,
            max_iterations: 10,  // Cap reduzido (cost guard)
            timeout_secs: 120,   // 2 min (read-only e rapido)
            source: AgentSpecSource::OnDemand,
        }
    }
}
```

### SubAgentRegistry

Usa `IndexMap` para preservar insertion order e garantir determinismo em `build_tool_description()`.

```rust
// theo-agent-runtime/src/subagent/registry.rs (NOVO)

use indexmap::IndexMap;
use theo_domain::agent_spec::{AgentSpec, AgentSpecSource};
use theo_domain::capability::CapabilitySet;
use std::path::Path;

pub struct SubAgentRegistry {
    specs: IndexMap<String, AgentSpec>,
}

impl SubAgentRegistry {
    pub fn new() -> Self {
        Self { specs: IndexMap::new() }
    }

    /// Registry com os 4 agents built-in.
    pub fn with_builtins() -> Self {
        let mut reg = Self::new();
        for spec in super::builtins::all_builtins() {
            reg.specs.insert(spec.name.clone(), spec);
        }
        reg
    }

    /// Carrega custom agents de um diretorio (.theo/agents/).
    /// Specs com frontmatter invalido sao ignorados com warning (pattern OpenDev).
    pub fn load_from_dir(&mut self, dir: &Path) -> Vec<String> {
        // Retorna Vec<warning_message> para specs ignorados.
        // Implementacao na Fase 2.
        vec![]
    }

    /// Carrega de todas as fontes: built-in < global < project.
    /// Seguranca S2: overrides de builtins usam intersecao de capabilities.
    pub fn load_all(&mut self, project_dir: &Path) {
        // 1. Builtins ja carregados (ou carregar agora)
        // 2. Global: ~/.theo/agents/*.md
        // 3. Project: {project_dir}/.theo/agents/*.md
        // Override com mesmo nome: custom.capability_set = builtin.intersect(custom)
    }

    /// Registra um agent (para on-demand).
    pub fn register(&mut self, spec: AgentSpec) {
        self.specs.insert(spec.name.clone(), spec);
    }

    /// Lookup por nome.
    pub fn get(&self, name: &str) -> Option<&AgentSpec> {
        self.specs.get(name)
    }

    /// Nomes disponiveis (preserva insertion order).
    pub fn names(&self) -> Vec<&str> {
        self.specs.keys().map(|s| s.as_str()).collect()
    }

    /// Gera descricao para o schema da tool delegate_task.
    /// Deterministico: mesma ordem sempre (IndexMap).
    pub fn build_tool_description(&self) -> String {
        let agents: Vec<String> = self.specs.values().map(|s| {
            format!("- {}: {}", s.name, s.description)
        }).collect();
        format!(
            "Delegate work to a specialized sub-agent.\n\nAvailable agents:\n{}\n\n\
             Use any name not listed above to create an on-demand read-only agent.",
            agents.join("\n")
        )
    }
}
```

### Formato Markdown para Custom Agents

```markdown
---
name: security-reviewer
description: "Reviews code for OWASP Top 10 vulnerabilities"
tools:
  - read
  - grep
  - glob
denied_tools:
  - edit
  - write
  - apply_patch
  - bash
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

**Frontmatter fields:**

| Campo | Tipo | Default | Descricao |
|---|---|---|---|
| `name` | string | filename sem extensao | ID unico |
| `description` | string | obrigatorio | Para tool schema |
| `tools` | string[] | [] (= all allowed) | Allowed tools |
| `denied_tools` | string[] | [] | Denied tools (precedencia sobre allowed) |
| `model` | string | None (herda parent) | Model override |
| `max_iterations` | u32 | 30 | Loop limit |
| `timeout` | u32 | 300 | Seconds |

Body (apos `---`) = system prompt.

**Seguranca S2:** Se `name` coincide com um builtin, o `CapabilitySet` do custom e interseccionado com o do builtin (nunca escalado). Warning e logado.

### Frontmatter Parser (compartilhado com Skills)

O sistema de skills (`skill/mod.rs:124-173`) ja tem um parser de frontmatter simples (key-value). Agents precisam de YAML para arrays (`tools`, `denied_tools`).

**Decisao:** Extrair modulo compartilhado `frontmatter.rs` que suporta tanto key-value simples (skills) quanto YAML (agents). O parser de agents usa `serde_yaml` para o bloco frontmatter.

```rust
// theo-agent-runtime/src/frontmatter.rs (NOVO)

/// Parse markdown frontmatter delimitado por ---.
/// Retorna (frontmatter_str, body) ou None se formato invalido.
pub fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let content = content.trim();
    if !content.starts_with("---") {
        return None;
    }
    let after_first = &content[3..];
    let end = after_first.find("---")?;
    let frontmatter = &after_first[..end];
    let body = after_first[end + 3..].trim();
    Some((frontmatter, body))
}
```

O `subagent/parser.rs` usa `split_frontmatter()` + `serde_yaml::from_str()` para o bloco YAML. O `skill/mod.rs` migra para usar `split_frontmatter()` para o split, mantendo seu key-value parser para os campos simples.

### Meta-tool `delegate_task`

Substitui `subagent` + `subagent_parallel`. Schema com `oneOf` para evitar ambiguidade.

```json
{
  "name": "delegate_task",
  "description": "<gerado por registry.build_tool_description()>",
  "parameters": {
    "oneOf": [
      {
        "type": "object",
        "description": "Delegate to a single agent",
        "properties": {
          "agent": {
            "type": "string",
            "description": "Agent name (registered or on-demand)"
          },
          "objective": {
            "type": "string",
            "description": "What the agent should accomplish"
          },
          "context": {
            "type": "string",
            "description": "Optional background info, file paths, constraints"
          }
        },
        "required": ["agent", "objective"]
      },
      {
        "type": "object",
        "description": "Delegate to multiple agents in parallel",
        "properties": {
          "parallel": {
            "type": "array",
            "items": {
              "type": "object",
              "properties": {
                "agent": { "type": "string" },
                "objective": { "type": "string" },
                "context": { "type": "string" }
              },
              "required": ["agent", "objective"]
            }
          }
        },
        "required": ["parallel"]
      }
    ]
  }
}
```

**Modos de uso:**
1. **Named agent** (built-in ou custom): `agent` = nome registrado no registry
2. **On-demand**: `agent` = nome nao registrado → cria `AgentSpec::on_demand(name, objective)` com `CapabilitySet::read_only()` (seguranca S1)
3. **Parallel**: `parallel` array com multiplos agents

**Validacao:** Se o JSON contem ambos `agent` e `parallel`, retorna erro explicito.

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

    /// Backward-compat: cria manager com registry de builtins apenas.
    pub fn with_builtins(
        config: AgentConfig,
        event_bus: Arc<EventBus>,
        project_dir: PathBuf,
    ) -> Self {
        Self::new(config, event_bus, project_dir, Arc::new(SubAgentRegistry::with_builtins()))
    }

    /// Spawn agora recebe &AgentSpec em vez de SubAgentRole.
    /// Context mantem Vec<Message> para preservar structured history.
    pub fn spawn(
        &self,
        spec: &AgentSpec,
        objective: &str,
        context: Option<Vec<Message>>,
    ) -> Pin<Box<dyn Future<Output = AgentResult> + Send + '_>>;

    /// Helper para delegate_task: converte string context em Vec<Message>.
    pub fn spawn_with_text_context(
        &self,
        spec: &AgentSpec,
        objective: &str,
        context: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = AgentResult> + Send + '_>> {
        let messages = context.map(|c| vec![Message::user(c)]);
        self.spawn(spec, objective, messages)
    }

    /// Parallel spawn com struct nomeado (evita tuple ambigua).
    pub async fn spawn_parallel(
        &self,
        tasks: Vec<ParallelTask>,
    ) -> Vec<AgentResult>;
}

/// Named struct para tarefas paralelas (evita tuple ordering ambiguity).
pub struct ParallelTask {
    pub spec: AgentSpec,
    pub objective: String,
    pub context: Option<String>,
}
```

### AgentResult Estendido (minimo)

```rust
// agent_loop.rs — ESTENDER AgentResult

pub struct AgentResult {
    // ... campos existentes (12 campos) ...
    pub agent_name: String,              // NOVO: qual agent executou
    pub context_used: Option<String>,    // NOVO: context passado
}
```

Apenas 2 campos novos. `AgentFinding`/`FindingSeverity` sao DEFERIDOS — serao desenhados quando houver dados reais de output para basear o schema.

### Eventos Frontend para Sub-Agents

```rust
// theo-domain/src/event.rs — ESTENDER DomainEvent variants

/// Emitido quando um sub-agent inicia.
SubagentStarted {
    agent_name: String,
    agent_source: String,  // "builtin", "project", "global", "on_demand"
    objective: String,
}

/// Emitido quando um sub-agent termina.
/// Inclui metricas de custo para observabilidade per-agent (D4).
SubagentCompleted {
    agent_name: String,
    agent_source: String,     // "builtin", "project", "global", "on_demand"
    success: bool,
    summary: String,
    duration_ms: u64,
    tokens_used: u64,
    input_tokens: u64,
    output_tokens: u64,
    llm_calls: u64,
    iterations_used: usize,
}
```

O frontend (`theo-ui`) pode ouvir estes eventos para mostrar qual agent esta ativo, seu progresso e resultado. Sem estes eventos, a UI fica cega durante `delegate_task`.

### CapabilitySet::intersect (Seguranca S2)

```rust
// theo-domain/src/capability.rs — NOVO metodo

impl CapabilitySet {
    /// Intersect two capability sets (most restrictive wins).
    /// Used when a custom agent overrides a builtin name (S2).
    ///
    /// Rules:
    /// - denied_tools = union (if either denies, it's denied)
    /// - allowed_tools = intersection (if both allow, it's allowed; empty = all)
    /// - network_access = AND (both must allow)
    /// - max_file_size_bytes = MIN
    /// - allowed_paths = intersection (if both non-empty)
    pub fn intersect(&self, other: &CapabilitySet) -> CapabilitySet {
        // ...
    }
}
```

### Model Routing per Agent

`SubAgentRoleId` ja existe em `theo-domain/src/routing.rs` como `Cow<'static, str>`. Suporta IDs dinamicos naturalmente:

```rust
// Zero mudancas no domain — ja funciona:
SubAgentRoleId(Cow::Owned("security-reviewer".to_string()))
```

`AgentSpec::role_id()` gera o `SubAgentRoleId` a partir de `spec.name`. O `ModelRouter::route()` recebe `RoutingPhase::Subagent { role }` e despacha por role_id.

---

## Design Decisions SOTA

### D1: Streaming de Output do Sub-Agent

**Status:** Ja funciona. `PrefixedEventForwarder` (subagent/mod.rs:304-316) forwarda TODOS os eventos do sub-agent para o parent bus, incluindo `ContentDelta` e `ReasoningDelta`. O `entity_id` e prefixado com `[RoleName]`.

**O que falta:** Identificacao estruturada. Hoje o prefix e uma string `[Explorer]` concatenada ao `entity_id`. O frontend nao consegue parsear programaticamente qual sub-agent esta emitindo.

**Decisao:** Os novos eventos `SubagentStarted` e `SubagentCompleted` (ja no plano, Fase 3) resolvem a identificacao. Entre `SubagentStarted` e `SubagentCompleted`, todos os `ContentDelta` com entity_id prefixado `[agent_name]` pertencem ao sub-agent ativo. O frontend usa `SubagentStarted.agent_name` para rotular o streaming.

**Implementacao (Fase 3, zero codigo extra alem do ja planejado):**

```rust
// spawn() — ja planejado:
self.event_bus.publish(SubagentStarted { agent_name, agent_source, objective });

// PrefixedEventForwarder ja forwards ContentDelta/ReasoningDelta
// → Frontend sabe que deltas entre Started/Completed pertencem ao sub-agent

self.event_bus.publish(SubagentCompleted { agent_name, success, summary, duration_ms });
```

**Observability:** O `ObservabilityListener` (listener.rs:69) filtra streaming events do trajectory. Isso e correto — ContentDelta de sub-agents NAO devem entrar no trajectory do parent (volume excessivo). Os eventos `SubagentStarted`/`SubagentCompleted` (nao-streaming) ENTRAM no trajectory, registrando que o sub-agent executou.

### D2: Estrategia de Prompting — Quando Delegar

O plano define a infra (`delegate_task`, registry) mas precisa definir **quando** o LLM principal deve delegar vs fazer ele mesmo. Sem heuristicas, o LLM sub-utiliza ou sobre-utiliza delegacao.

**Decisao:** Adicionar heuristicas ao system prompt do agent principal. NAO e logica de codigo — e instrucao no prompt.

**Prompt additions (integrado no system prompt principal na Fase 4):**

```text
## When to use delegate_task

DELEGATE when:
- The task requires exploring MORE than 5 files to understand context
  → delegate to explorer first, then act on findings
- The task has independent sub-problems that can run in parallel
  → use delegate_task with parallel array
- The task requires code review or security analysis
  → delegate to reviewer or custom security agent
- The user explicitly asks for parallel work or agent delegation

DO NOT delegate when:
- The task is a single file edit or simple question
- You already have enough context from previous tool calls
- The task requires sequential decisions where each step depends on the previous

COST AWARENESS:
- Each sub-agent consumes a full agent loop (iterations + tokens)
- On-demand agents are limited to 10 iterations and read-only access
- Prefer named agents (explorer, implementer, verifier, reviewer) over on-demand
- Use parallel delegation only when tasks are truly independent
```

**Evidencia:** Claude Code usa heuristicas similares internamente (arXiv 2604.14228 §4: "the orchestrator decides delegation based on task complexity signals"). Aider documenta "use architect mode when changes span multiple files" (aider.chat/architect).

**Mecanismo de refinamento:** Estas heuristicas sao instrucoes no prompt, nao codigo. Podem ser ajustadas sem recompilar. Se a telemetria (via observability) mostrar que o LLM delega em excesso ou de menos, ajustamos o prompt.

### D3: Output Protocol — Contrato Minimo de Output

O plano deferiu `AgentFinding` (correto, YAGNI). Mas sub-agents precisam de um contrato minimo de output para que o parent consiga processar resultados programaticamente.

**Decisao:** O contrato de output e definido no system prompt do sub-agent, NAO em tipos Rust. Zero codigo novo.

**Formato (adicionado ao final de cada system prompt de builtin):**

```text
When you call `done`, structure your summary as:

RESULT: <one-line summary of what was accomplished>
FILES: <comma-separated list of files examined or modified, or "none">
CONFIDENCE: <HIGH | MEDIUM | LOW>
DETAILS:
<detailed findings, one per line>
```

**Implementacao:**

1. `builtins.rs` — cada builtin's system_prompt termina com o formato acima
2. Custom agents (`.theo/agents/*.md`) podem seguir ou ignorar — o formato e instrucao, nao enforcement
3. `AgentResult.summary` contem o output raw. O parent LLM parseia o formato se presente
4. Se o sub-agent nao segue o formato, `summary` continua sendo free-text (zero breaking change)

**Por que NAO enforcement em codigo:**
- LLM output e nao-deterministico — enforcement por regex seria fragil
- O parent LLM e capaz de extrair informacao de free-text (e o que faz hoje)
- Quando houver dados reais mostrando que structured output melhora qualidade, ai sim implementamos `AgentFinding` com parsing

**Evidencia:** Hermes usa free-text summary + optional `tool_trace` metadata. OpenDev retorna `SubAgentResult { summary, files_changed }` sem structured findings. Ambos priorizam robustez sobre estrutura.

### D4: Observabilidade de Custos per-Agent

O `MetricsCollector` (observability/metrics.rs) ja coleta tokens, LLM calls, e custo USD por run. `AgentResult` ja tem `tokens_used`, `input_tokens`, `output_tokens`. O gap e agregacao por tipo de agent.

**Decisao:** Emitir metricas por sub-agent via `SubagentCompleted` event (ja planejado). O `ObservabilityListener` persiste estes eventos no trajectory. Agregacao e responsabilidade do observability dashboard, nao do runtime.

**Implementacao (Fase 3, estendendo SubagentCompleted ja planejado):**

```rust
/// Emitido quando um sub-agent termina.
SubagentCompleted {
    agent_name: String,
    agent_source: String,     // "builtin", "project", "global", "on_demand"
    success: bool,
    summary: String,
    duration_ms: u64,
    // Metricas de custo — extraidas de AgentResult
    tokens_used: u64,
    input_tokens: u64,
    output_tokens: u64,
    llm_calls: u64,
    iterations_used: usize,
}
```

**Fluxo:**
1. Sub-agent completa → `AgentResult` com metricas preenchidas (ja funciona)
2. `SubAgentManager::spawn()` extrai metricas do `AgentResult` → emite `SubagentCompleted` com campos de custo
3. `ObservabilityListener` persiste no trajectory JSONL
4. Dashboard/CLI pode agregar: "explorer: 15k tokens avg, implementer: 45k tokens avg, security-reviewer: 8k tokens avg"

**Zero codigo novo alem do ja planejado.** Apenas estende o payload do `SubagentCompleted` com campos do `AgentResult`.

### D5: Retry e Fallback per-Agent

O `AgentRunEngine` ja tem retry loop com `ModelRouter::fallback()` (run_engine.rs:825-876) para falhas de LLM (429, 5xx, context overflow). Sub-agents herdam este retry porque usam o mesmo `AgentRunEngine`.

**O gap:** Quando o sub-agent INTEIRO falha (timeout, max_iterations atingido, panic), o `SubAgentManager` retorna `AgentResult { success: false }` e o parent decide o que fazer. Nao ha retry automatico a nivel de sub-agent.

**Decisao: NAO adicionar retry automatico a nivel de sub-agent.** Justificativa:

1. **O parent LLM e o melhor decisor.** Quando um sub-agent falha, o parent ve o `summary` (e.g. "timed out after 300s") e pode decidir: retry com escopo menor, delegar para outro agent, ou resolver manualmente. Retry automatico cego desperdicaria tokens sem diagnosticar a causa.

2. **O retry de LLM ja existe.** Se um sub-agent falha por 429 ou context overflow, o retry loop interno do `AgentRunEngine` ja trata (3 retries com backoff). O gap e apenas para falhas de alto nivel (timeout, max_iterations).

3. **YAGNI.** Nenhum cenario real exige retry automatico de sub-agent hoje.

**O que fazemos em vez disso — informacao para o parent decidir:**

```rust
// AgentResult ja tem:
pub success: bool,
pub summary: String,  // Contem razao da falha

// SubagentCompleted event (D4) adiciona:
pub duration_ms: u64,      // Parent ve se foi timeout
pub iterations_used: usize, // Parent ve se atingiu max
```

**Prompt addition (integrado no system prompt principal, Fase 4):**

```text
## When a sub-agent fails

If delegate_task returns success=false:
1. READ the summary to understand WHY it failed
2. If timeout: re-delegate with a more focused objective or smaller scope
3. If max_iterations: the task may be too complex — break it into smaller sub-tasks
4. If error: investigate the error, fix the issue, then re-delegate
5. Do NOT blindly retry the same delegation — diagnose first
```

**Evidencia:** Claude Code nao faz retry automatico de sub-agents. Hermes tem retry mas so para falhas de transport (nao logicas). A decisao de "o parent decide" e o padrao dominante em orchestrator-worker architectures (Anthropic multi-agent paper §3.2).

---

## Arquivos a Modificar

### Novos arquivos

| Arquivo | Descricao |
|---|---|
| `crates/theo-domain/src/agent_spec.rs` | `AgentSpec`, `AgentSpecSource` |
| `crates/theo-agent-runtime/src/frontmatter.rs` | Parser compartilhado de frontmatter (split) |
| `crates/theo-agent-runtime/src/subagent/registry.rs` | `SubAgentRegistry` com IndexMap |
| `crates/theo-agent-runtime/src/subagent/parser.rs` | Parse YAML frontmatter → AgentSpec |
| `crates/theo-agent-runtime/src/subagent/builtins.rs` | 4 built-in specs (extraidos de mod.rs) |

### Arquivos modificados

| Arquivo | Mudanca |
|---|---|
| `crates/theo-domain/src/lib.rs` | `pub mod agent_spec;` |
| `crates/theo-domain/src/capability.rs` | `CapabilitySet::intersect()` (seguranca S2) |
| `crates/theo-domain/src/event.rs` | Variantes `SubagentStarted`, `SubagentCompleted` |
| `crates/theo-agent-runtime/src/subagent/mod.rs` | `SubAgentManager` recebe `Arc<SubAgentRegistry>`, `spawn()` recebe `&AgentSpec`, emite eventos. `with_builtins()` para backward compat. |
| `crates/theo-agent-runtime/src/tool_bridge.rs` | Substituir `subagent`/`subagent_parallel` por `delegate_task` com schema dinamico |
| `crates/theo-agent-runtime/src/run_engine.rs` | Dispatch de `delegate_task` no lugar de `subagent`/`subagent_parallel` |
| `crates/theo-agent-runtime/src/agent_loop.rs` | `AgentResult` ganha `agent_name`, `context_used` |
| `crates/theo-agent-runtime/src/skill/mod.rs` | Migrar para usar `frontmatter::split_frontmatter()`. `SkillMode::SubAgent` usa registry lookup em vez de `SubAgentRole::from_str()`. |

### Dependencias novas

| Crate | Dependencia | Motivo |
|---|---|---|
| `theo-agent-runtime` | `serde_yaml` (workspace) | Parse YAML frontmatter |
| `theo-agent-runtime` | `indexmap` (workspace) | SubAgentRegistry preserva ordem |

Adicionar ambas a `[workspace.dependencies]` no root `Cargo.toml`.

---

## Fases de Implementacao

### Fase 1: Domain Types + Builtins + Registry

**Objetivo:** AgentSpec como tipo central, 4 builtins extraidos, registry funcional.

**TDD Sequence:**

```
RED:
  #[test] fn test_agent_spec_on_demand_is_read_only()
  #[test] fn test_agent_spec_role_id_returns_correct_id()
  #[test] fn test_agent_spec_source_serde_roundtrip()
  #[test] fn test_capability_set_intersect_denied_tools_union()
  #[test] fn test_capability_set_intersect_network_access_and()
  #[test] fn test_capability_set_intersect_max_file_size_min()
  → cargo test -p theo-domain → FAIL (tipos nao existem)

GREEN:
  1. Criar theo-domain/src/agent_spec.rs (AgentSpec, AgentSpecSource)
  2. Adicionar pub mod agent_spec em theo-domain/src/lib.rs
  3. Implementar CapabilitySet::intersect() em capability.rs
  4. Cargo test → PASS

RED:
  #[test] fn test_builtin_explorer_has_read_only_capabilities()
  #[test] fn test_builtin_implementer_has_write_capabilities()
  #[test] fn test_builtin_verifier_cannot_edit()
  #[test] fn test_builtin_reviewer_is_read_only()
  #[test] fn test_all_builtins_returns_4_specs()
  → cargo test -p theo-agent-runtime → FAIL (builtins.rs nao existe)

GREEN:
  1. Criar subagent/builtins.rs — extrair os 4 roles como AgentSpec
  2. fn all_builtins() -> Vec<AgentSpec>
  → cargo test → PASS

RED:
  #[test] fn test_registry_with_builtins_has_4_agents()
  #[test] fn test_registry_get_returns_none_for_missing()
  #[test] fn test_registry_register_adds_agent()
  #[test] fn test_registry_names_preserves_insertion_order()
  #[test] fn test_registry_build_tool_description_is_deterministic()
  → cargo test → FAIL (registry.rs nao existe)

GREEN:
  1. Criar subagent/registry.rs com IndexMap
  2. with_builtins(), get(), register(), names(), build_tool_description()
  → cargo test → PASS

REFACTOR:
  - Verificar derives (Debug, Clone, Serialize, Deserialize) em todos os tipos
  - Verificar que theo-domain continua com zero deps externas
```

**Verify:** `cargo test -p theo-domain -- agent_spec && cargo test -p theo-domain -- capability::tests::intersect && cargo test -p theo-agent-runtime -- builtins && cargo test -p theo-agent-runtime -- registry`

### Fase 2: Frontmatter Parser + Custom Loading

**Objetivo:** Carregar agents de `.theo/agents/*.md` com YAML frontmatter.

**TDD Sequence:**

```
RED:
  #[test] fn test_split_frontmatter_valid()
  #[test] fn test_split_frontmatter_no_delimiter_returns_none()
  #[test] fn test_split_frontmatter_missing_closing_returns_none()
  #[test] fn test_split_frontmatter_empty_body_allowed()
  → cargo test → FAIL (frontmatter.rs nao existe)

GREEN:
  1. Criar frontmatter.rs com split_frontmatter()
  → cargo test → PASS

RED:
  #[test] fn test_parse_agent_spec_valid_all_fields()
  #[test] fn test_parse_agent_spec_minimal_fields_uses_defaults()
  #[test] fn test_parse_agent_spec_missing_description_returns_error()
  #[test] fn test_parse_agent_spec_invalid_yaml_returns_error()
  #[test] fn test_parse_agent_spec_unknown_fields_ignored()
  #[test] fn test_parse_agent_spec_denied_tools_populates_capability_set()
  #[test] fn test_parse_agent_spec_tools_array_populates_allowed_tools()
  #[test] fn test_parse_agent_spec_name_defaults_to_filename()
  → cargo test → FAIL (parser.rs nao existe)

GREEN:
  1. Criar subagent/parser.rs — fn parse_agent_spec(content: &str, filename: &str) -> Result<AgentSpec, ParseError>
  2. Usa split_frontmatter() + serde_yaml para o bloco YAML
  → cargo test → PASS

RED:
  #[test] fn test_load_from_dir_finds_md_files()                    // tempdir
  #[test] fn test_load_from_dir_skips_invalid_with_warning()         // tempdir
  #[test] fn test_load_all_resolution_order_project_overrides_global()
  #[test] fn test_load_all_builtin_override_intersects_capabilities() // S2
  #[test] fn test_load_all_builtin_override_logs_warning()            // S2
  → cargo test → FAIL

GREEN:
  1. Implementar SubAgentRegistry::load_from_dir() e load_all()
  2. Intersecao de capabilities para overrides de builtins (S2)
  → cargo test → PASS

REFACTOR:
  - Migrar skill/mod.rs para usar frontmatter::split_frontmatter()
  - Verificar que testes de skills continuam passando
```

**Verify:** `cargo test -p theo-agent-runtime -- frontmatter && cargo test -p theo-agent-runtime -- parser && cargo test -p theo-agent-runtime -- registry && cargo test -p theo-agent-runtime -- skill`

### Fase 3: Refatorar SubAgentManager

**Objetivo:** SubAgentManager usa AgentSpec/Registry. Eventos emitidos. Backward compat.

**Risco principal:** 530+ testes existentes podem quebrar. Mitigacao: `with_builtins()` preserva assinatura antiga.

**TDD Sequence:**

```
RED:
  #[test] fn test_spawn_with_agent_spec_sets_system_prompt()
  #[test] fn test_spawn_with_text_context_creates_user_message()
  #[test] fn test_agent_result_has_agent_name()
  #[test] fn test_with_builtins_preserves_old_behavior()
  #[test] fn test_subagent_started_event_emitted()
  #[test] fn test_subagent_completed_event_emitted_with_metrics()  // D4: tokens, llm_calls
  #[test] fn test_subagent_completed_has_agent_source()             // D4: builtin/project/etc
  #[test] fn test_max_depth_prevents_recursion_with_spec()
  → cargo test → FAIL

GREEN:
  1. Adicionar SubagentStarted/SubagentCompleted a DomainEvent
  2. Refatorar SubAgentManager:
     - new() recebe Arc<SubAgentRegistry>
     - with_builtins() para backward compat
     - spawn() recebe &AgentSpec (+ context: Option<Vec<Message>>)
     - spawn_with_text_context() helper
     - Emitir SubagentStarted antes do spawn, SubagentCompleted apos
  3. AgentResult ganha agent_name, context_used (com Default)
  4. Atualizar spawn_parallel() para usar ParallelTask struct
  → cargo test → PASS

REFACTOR:
  - Bulk-replace nos testes existentes:
    - SubAgentManager::new(c,e,p) → SubAgentManager::with_builtins(c,e,p)
    - spawn(SubAgentRole::Explorer, obj, ctx) → spawn(&AgentSpec::from builtins, obj, ctx)
  - Verificar: cargo test (workspace inteiro)
```

**Verify:** `cargo test` (workspace inteiro — regressao)

### Fase 4: delegate_task + Cleanup

**Objetivo:** Nova tool unificada. Remocao de SubAgentRole. Skill migration. Integration test.

**TDD Sequence:**

```
RED:
  #[test] fn test_delegate_task_dispatches_named_builtin()
  #[test] fn test_delegate_task_dispatches_custom_agent()
  #[test] fn test_delegate_task_on_demand_uses_read_only_capabilities()
  #[test] fn test_delegate_task_on_demand_max_iterations_10()
  #[test] fn test_delegate_task_parallel_spawns_concurrent()
  #[test] fn test_delegate_task_rejects_both_agent_and_parallel()
  #[test] fn test_delegate_task_schema_lists_available_agents()
  #[test] fn test_delegate_task_unknown_agent_creates_on_demand()
  → cargo test → FAIL

GREEN:
  1. Substituir subagent + subagent_parallel por delegate_task em tool_bridge.rs
  2. Schema dinamico via registry.build_tool_description()
  3. Dispatch em run_engine.rs:
     - Se agent no registry → spawn com spec
     - Se agent NAO no registry → AgentSpec::on_demand() (read_only, S1)
     - Se parallel → spawn_parallel
     - Se ambos agent+parallel → erro
  4. Atualizar registry_to_definitions() e registry_to_definitions_for_subagent()
  → cargo test → PASS

RED:
  #[test] fn test_skill_subagent_mode_uses_registry()
  → cargo test → FAIL (SkillMode::SubAgent ainda usa SubAgentRole)

GREEN:
  1. Migrar SkillMode::SubAgent para usar agent name + registry lookup
  → cargo test → PASS

CLEANUP:
  1. Remover SubAgentRole enum (agora redundante — builtins.rs e o source of truth)
  2. Remover timeout_for_role() (agora vive em AgentSpec.timeout_secs)
  3. Atualizar system prompt do agent principal:
     - Adicionar heuristicas de delegacao (D2: quando delegar vs fazer direto)
     - Adicionar instrucoes de fallback (D5: quando sub-agent falha)
  4. Atualizar system prompts dos builtins com output protocol (D3: RESULT/FILES/CONFIDENCE)
  5. Remover "subagent" e "subagent_parallel" da blocked list em batch_execute
  6. Adicionar "delegate_task" a blocked list em batch_execute
  7. Limpar imports orfaos
  → cargo test && cargo clippy -- -D warnings

INTEGRATION TEST:
  1. Criar .theo/agents/test-agent.md em tempdir
  2. Carregar registry com load_all()
  3. delegate_task com agent="test-agent" → verifica que spec correto e usado
  4. delegate_task com agent="unknown" → verifica on_demand read_only
```

**Verify:** `cargo test && cargo clippy -- -D warnings`

---

## Invariantes Preservados

- **depth=1** — sub-agents NAO spawnam sub-agents (sem mudanca)
- **return-only** — sub-agents retornam `AgentResult` ao parent (sem mudanca)
- **EventBus forwarding** — `PrefixedEventForwarder` tageia eventos por `spec.name` (sem mudanca)
- **CapabilityGate** — continua funcionando, agora alimentado por `spec.capability_set` (sem mudanca)
- **Seguranca S1** — on-demand agents sao read-only por default
- **Seguranca S2** — overrides de builtins usam intersecao de capabilities (nunca escalacao)
- **`is_subagent = true`** — continua bloqueando meta-tools de delegacao (sem mudanca)
- **Budget enforcement** — tokens do sub-agent contam para o parent (sem mudanca)
- **Dependency direction** — `AgentSpec` vive em `theo-domain` (zero deps), registry/parser vivem em `theo-agent-runtime`
- **Sub-agents via grep/glob NAO passam por RRF** — decisao consciente, nao oversight
- **`.theo/agents/` excluido de retrieval e wiki index** — runtime config, nao knowledge
- **Streaming D1** — PrefixedEventForwarder ja forwards ContentDelta; SubagentStarted/Completed delimitam o fluxo
- **Prompting D2** — heuristicas de delegacao vivem no prompt, nao em codigo
- **Output protocol D3** — contrato minimo via prompt instruction, nao enforcement em tipos Rust
- **Observability D4** — custos per-agent emitidos via SubagentCompleted, agregacao no dashboard
- **Retry D5** — NAO ha retry automatico de sub-agents; o parent LLM decide baseado no summary de falha

---

## Riscos e Mitigacoes

| Risco | Mitigacao |
|---|---|
| Custom agent com system prompt malicioso | S2: capabilities interseccionadas com builtin. S3: user confirmation na primeira carga. CapabilityGate enforcing |
| Parser de frontmatter fragil | Testes extensivos (8 test cases). Frontmatter invalido → skip com warning (pattern OpenDev) |
| Breaking change na tool API | `delegate_task` substitui `subagent` + `subagent_parallel` atomicamente na Fase 4 |
| 530+ testes quebram na Fase 3 | `with_builtins()` preserva assinatura antiga. Bulk-replace em commit dedicado |
| On-demand agent burn tokens | S1: read_only + max_iterations=10 + timeout=120s. Cost guard efetivo |
| Project agents de repo malicioso | S3: user confirmation na primeira carga. `.theo/.agents-approved` persiste |
| YAML parser como nova dependencia | `serde_yaml` e madura, bem mantida. Impacto em compile time: ~5s |
| Skill system breakage | Fase 4 migra SkillMode::SubAgent para registry lookup. Testes existentes validam |
| Frontend cego durante sub-agents | SubagentStarted/SubagentCompleted eventos emitidos na Fase 3 |

---

## Verificacao Final

```bash
# Fase 1: domain types, builtins, registry
cargo test -p theo-domain -- agent_spec
cargo test -p theo-domain -- capability::tests
cargo test -p theo-agent-runtime -- builtins
cargo test -p theo-agent-runtime -- registry

# Fase 2: frontmatter parser, custom loading
cargo test -p theo-agent-runtime -- frontmatter
cargo test -p theo-agent-runtime -- parser
cargo test -p theo-agent-runtime -- skill

# Fase 3: SubAgentManager refactor
cargo test  # workspace inteiro — regressao

# Fase 4: delegate_task, cleanup, integration
cargo test
cargo clippy -- -D warnings

# Smoke test: custom agent
mkdir -p .theo/agents
cat > .theo/agents/test-agent.md << 'EOF'
---
name: test-explorer
description: "Test agent for validation"
denied_tools:
  - edit
  - write
  - bash
max_iterations: 5
timeout: 60
---
You are a test agent. Read one file and call done with a summary.
EOF

# Build completo
cargo build
```

---

## Epicos Futuros (fora deste plano)

| Epic | Quando | Pre-requisito |
|---|---|---|
| **File Locking** (FileLockManager advisory) | Quando parallel writers causarem conflitos observados em producao | Este plano (Fase 3 — spawn_parallel com AgentSpec) |
| **Worktree Isolation** (WorktreeManager) | Quando multi-agent patterns exigirem isolamento | Este plano + File Locking |
| **MCP Client** (consumir MCP servers externos) | Plano separado com design de transport/config | Crate `theo-infra-mcp` (nao `theo-agent-runtime`) |
| **MCP Server** (Theo como MCP server) | Quando IDE integrations exigirem | MCP Client + CLI flag `--mcp-server` |
| **AgentFinding** (structured results) | Quando houver dados reais de output para basear schema | Este plano (Fase 4 — delegate_task funcionando) |

---

## Referencias

| # | Fonte | URL |
|---|---|---|
| 1 | Claude Code Docs — Agent Teams | https://code.claude.com/docs/en/agent-teams |
| 2 | arXiv 2604.14228 — Dive into Claude Code | https://arxiv.org/abs/2604.14228 |
| 3 | OpenAI Codex Subagents | https://developers.openai.com/codex/subagents |
| 4 | Anthropic — Multi-Agent Research System | https://www.anthropic.com/engineering/multi-agent-research-system |
| 5 | Aider — Architect Mode | https://aider.chat/2024/09/26/architect.html |
| 6 | Google ADK | https://google.github.io/adk-docs/ |
| 7 | OpenAI Agents SDK | https://openai.github.io/openai-agents-python/ |

### Projetos locais analisados (`referencias/`)

- **hermes-agent** — `tools/delegate_tool.py`: delegate_task tool, child construction, blocked tools
- **opendev** — `crates/opendev-agents/src/subagents/`: SubAgentSpec, custom_loader, runner trait
- **opencode** — `AGENTS.md`: build/plan/general 3-agent system

### Reuniao de revisao

- `.claude/meetings/20260423-021723-review-agents-plan.md` — 16 agentes, veredito REVISED, 13 decisoes
