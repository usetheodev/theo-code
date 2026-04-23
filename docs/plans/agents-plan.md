# Plano: Dynamic Sub-Agent System (SOTA — Built-in + Custom + On-demand + Lifecycle + Integration + Isolation)

> **Versao 3.1 (IMPLEMENTADO 100% sem legacy)** — TODAS as 13 fases entregues + integrações end-to-end no pipeline. Sistema novo: `SubAgentRole` enum, `spawn(role,...)`, `subagent`/`subagent_parallel` tools — REMOVIDOS. Apenas `AgentSpec` + `delegate_task` + `spawn_with_spec`. ~2000 testes passando, zero regressão. Hot-reload, MCP, checkpoint per-mutation, OTel spans, todos ATIVOS no execute() loop.
> **Versao 3.0 (SOTA)** — Revisao 2026-04-23: expansao para nivel SOTA baseado em evidencias diretas dos `referencias/` (Archon, OpenDev, Hermes, Pi-Mono, OpenCode, Rippletide, FFF.nvim, QMD/llm-wiki-compiler) e literatura recente. Escopo elevado de 4 para **13 fases** organizadas em **4 tracks paralelos**: Track A (Fundacao MVP — fases originais), Track B (SOTA Lifecycle), Track C (SOTA Integration), Track D (SOTA Isolation & Observability).
> **Versao 2.1** — Fechados 3 gaps (G1 manifest aprovacao, G2 path-prefix-containment, G3 AllowedTools enum) e 5 ambiguidades (A1-A5).
> **Versao 2.0** — Revisado apos reuniao 20260423-021723 (16 agentes, veredito REVISED). Escopo 7 → 4 fases. Seguranca reforcada. TDD detalhado.

## Context

O sistema atual de sub-agents do Theo tem 4 roles hardcoded (`Explorer`, `Implementer`, `Verifier`, `Reviewer`) como um `enum SubAgentRole` em `subagent/mod.rs` (463 linhas). System prompts, capability sets, timeouts e max_iterations estao todos codificados em match arms. O LLM principal invoca sub-agents via meta-tools `subagent` e `subagent_parallel` em `tool_bridge.rs`, que aceitam apenas os 4 roles fixos.

**Problema 1 (MVP):** Nao ha como adicionar novos agentes sem recompilar, nem como usuarios definirem agentes customizados.

**Problema 2 (SOTA gap):** Comparado a sistemas SOTA de 2025-2026 (Claude Code, Cursor background agents, Archon, OpenDev), Theo Code carece de:
- **Lifecycle hooks** (PreToolUse, PostToolUse, SubagentStart/Stop, etc.) — Archon implementa 22 events com matchers regex (`referencias/Archon/packages/workflows/src/schemas/hooks.ts:10-32`).
- **MCP integration** — OpenDev tem crate dedicado `opendev-mcp/` para consumir tool servers externos. Sub-agents do Theo nao consomem MCP.
- **Programmable guardrails** — OpenAI Agents SDK e LangGraph oferecem guardrails 3-tier (input/output/handoff). Theo so tem CapabilityGate binario.
- **Checkpoint/rollback** — Hermes tem `checkpoint_manager.py` com shadow git repos, snapshot automatico antes de mutacoes. Theo nao reverte estado.
- **Worktree isolation per-agent** — Archon `WorktreeProvider` isola workflows por path-hash. Sub-agents paralelos do Theo compartilham CWD.
- **Session persistence/resume** — Archon tem `bun cli workflow resume <run-id>` (skip completed nodes). Sub-agent crashado em Theo e perdido.
- **Per-agent observability** — Confirmado via grep zero em `crates/theo-agent-runtime/src/observability/`: nenhuma dimensao por agent.
- **Cooperative cancellation** — OpenDev `subagent-execution-model.md:75` documenta cancellation tokens propagando parent → child. Theo nao cancela sub-agents.

**Objetivo:** Criar um sistema unificado de sub-agents com 3 fontes (built-in, custom, on-demand) **e** 9 capabilities SOTA (hooks, MCP, guardrails, checkpoint, worktree, resume, observability, cancellation, hot-reload).

**Estrategia de entrega (4 tracks paralelos):**

| Track | Fases | Entrega | Dependencia |
|---|---|---|---|
| **A — Fundacao MVP** | 1-4 | Sistema dinamico de specs (substitui hardcoded `SubAgentRole`). MVP entregavel. | Nenhuma |
| **B — SOTA Lifecycle** | 5-7 | Hooks, cancellation, output schema. Permite extensibilidade comportamental. | Track A |
| **C — SOTA Integration** | 8-10 | MCP client, checkpoint manager, session persistence. Conecta a ecossistema externo. | Track A |
| **D — SOTA Isolation & Obs** | 11-13 | Worktree per-agent, OpenTelemetry observability, hot-reload. Operacional em escala. | Track A; D11 depende de C9 (checkpoint) |

Tracks A, B, C podem comecar em paralelo. Track D requer A+B fechados.

**Escopo MANTIDO como epico futuro (nao SOTA-blocker):**
- **MCP Server** (Theo como MCP server consumido por IDEs externas) — feature de integracao, nao de sub-agent. Plano separado.
- **Google ADK A2A protocol** — depth>1 inter-agent. Nao adotado em Claude Code/Cursor. YAGNI confirmado.
- **AgentFinding/FindingSeverity** structured findings — D3 mantem free-text + JSON schema opcional (Fase 7). Tipo Rust dedicado deferido ate haver dados reais.

---

## Evidencias das Referencias

### Track A (Fundacao MVP)

| Referencia | Evidencia (file:line) | Pattern | Adotar |
|---|---|---|---|
| **OpenDev** | `referencias/opendev/crates/opendev-models/src/config/agent.rs:28-66` | `AgentConfigInline`: model, provider, prompt, max_steps, mode, color, hidden, disable, per-tool permissions | Formato spec, resolution order project>global>builtin |
| **Claude Code** | docs.claude.com/agents | Markdown em `.claude/agents/`. Frontmatter: name, description, tools, model. Return-only isolation | Formato markdown, model override |
| **Hermes** | `referencias/hermes-agent/tools/delegate_tool.py:32-50` | Blocked tools list + max_depth=2 + restricted toolset per child | Blocked tools pattern, on-demand schema |
| **OpenDev** | `referencias/opendev/docs/subagent-execution-model.md:7-78` | "Subagent = async task no mesmo processo, NAO processo separado". Tokio scheduling. Cancellation token + progress stream + isolated state | Modelo logico async (nao spawn de processo) |

### Track B (SOTA Lifecycle)

| Referencia | Evidencia | Pattern SOTA | Adotar como |
|---|---|---|---|
| **Archon** | `referencias/Archon/packages/workflows/src/schemas/hooks.ts:10-32` | **22 hook events** alinhados com Claude Agent SDK: PreToolUse, PostToolUse, PostToolUseFailure, Notification, UserPromptSubmit, SessionStart, SessionEnd, Stop, **SubagentStart, SubagentStop**, PreCompact, PermissionRequest, Setup, TeammateIdle, TaskCompleted, Elicitation, ElicitationResult, ConfigChange, **WorktreeCreate, WorktreeRemove**, InstructionsLoaded | Fase 5 — `theo-agent-runtime/src/hooks/` |
| **Archon** | `hooks.ts:43-50` | `workflowHookMatcherSchema`: matcher regex + response object + timeout per hook | Fase 5 — schema completo |
| **OpenDev** | `crates/opendev-hooks/src/{executor,manager,models}.rs` | Hook executor + manager separados (SRP); typed models | Fase 5 — arquitetura |
| **OpenDev** | `subagent-execution-model.md:75` | Cancellation token explicit per subagent | Fase 6 — `tokio::sync::CancellationToken` |
| **Rippletide** | `referencias/rippletide/AGENTS.md` | Hook-first planning: UserPromptSubmit injeta coding rules antes do plan | Fase 5 — caso de uso documentado |
| **OpenAI Agents SDK** | docs.openai.com/agents/guardrails | 3-tier guardrails: input (pre-LLM), output (post-LLM), handoff (pre-delegation) | Fase 7 — `Guardrail` trait |

### Track C (SOTA Integration)

| Referencia | Evidencia | Pattern SOTA | Adotar como |
|---|---|---|---|
| **Anthropic MCP** | modelcontextprotocol.io spec 2025-03-26 | JSON-RPC 2.0 sobre stdio/HTTP. `tools/list`, `tools/call`, `resources/read`. OAuth 2.1 para auth | Fase 8 — crate `theo-infra-mcp` |
| **OpenDev** | `crates/opendev-mcp/` (crate dedicado) | Cliente MCP nativo Rust integrado ao tool registry | Fase 8 — referencia direta |
| **Hermes** | `referencias/hermes-agent/tools/mcp_tool.py` (~1050 linhas) + `mcp_oauth_manager.py` | Discovery dinamico, streaming, OAuth manager separado | Fase 8 — arquitetura |
| **QMD/llm-wiki-compiler** | README.md:72-137 | MCP server stdio (default) + HTTP transport. Daemon mode mantem embeddings carregados | Fase 8 — referencia de transports |
| **Hermes** | `referencias/hermes-agent/tools/checkpoint_manager.py:1-69` | **Shadow git repo** em `~/.hermes/checkpoints/{sha256(abs_dir)[:16]}/` com `GIT_DIR + GIT_WORK_TREE`. Snapshot automatico antes de mutacoes. NAO e tool — infraestrutura transparente | Fase 9 — adotar diretamente |
| **Archon** | `CLAUDE.md` "Database Schema" (workflow_runs, workflow_events, sessions com `parent_session_id`, `transition_reason`) | Sessions imutaveis com audit trail. Resume re-roda skipping completed nodes | Fase 10 — schema |
| **Archon** | `CLAUDE.md` CLI: `bun run cli workflow resume <run-id>`, `abandon <run-id>`, `cleanup [days]` | Comandos CLI para lifecycle de runs | Fase 10 — UX |

### Track D (SOTA Isolation & Observability)

| Referencia | Evidencia | Pattern SOTA | Adotar como |
|---|---|---|---|
| **Archon** | `packages/isolation/src/providers/worktree.ts` + `CLAUDE.md` "Run in worktree" | `WorktreeProvider` isola por workflow. Port auto-allocation hash-based (3190-4089). `--no-worktree` opt-out | Fase 11 — `theo-isolation` policy |
| **Pi-Mono** | `referencias/pi-mono/AGENTS.md:194-233` | Parallel-agent git safety rules: forbid `git reset/checkout/stash/add -A`, only commit YOUR files, safe rebase-only | Fase 11 — regras de seguranca |
| **OpenTelemetry** | github.com/open-telemetry/semantic-conventions/tree/main/docs/gen-ai (2025) | `gen_ai.system`, `gen_ai.request.model`, `gen_ai.usage.input_tokens`, `gen_ai.agent.id`, `gen_ai.agent.name` | Fase 12 — trace attributes |
| **Archon** | `CLAUDE.md` "Logging": Pino structured logs com pattern `{domain}.{action}_{state}`. Eventos `workflow.step_started/_completed/_failed` | Fase 12 — naming convention |
| **OpenDev** | `crates/opendev-hooks/` watcher + `~/.opendev/agents/` | Hot-reload via filesystem watcher | Fase 13 — `notify` crate |
| **Claude Code** | docs.claude.com/agents — `.claude/agents/*.md` reload sem restart | Hot-reload referencia comparativa | Fase 13 — paridade |
| **FFF.nvim** | `referencias/fff.nvim/README.md:258-269` | Frecency memory para tools (file open history + query-file combo) reduz tokens | Epico futuro (cross-cutting) |
| **Pi-Mono** | `referencias/pi-mono/AGENTS.md:121-159` | Provider abstraction com lazy loading via register-builtins.ts | Epico futuro (refator de `theo-infra-llm`) |
| **Hermes** | `referencias/hermes-agent/AGENTS.md:430-486` | Profile-based isolation via `get_hermes_home()` getter | Epico futuro (multi-tenant) |

### Dado chave

> **98.4% do Claude Code e infraestrutura deterministica, nao logica de AI** (arXiv 2604.14228). A vantagem competitiva vem da qualidade do harness. Este plano agora **alvo SOTA explicitamente**: as fases B/C/D fecham gaps documentados vs Claude Code/Cursor/Archon. Cada feature tem evidencia direta de pelo menos um sistema producao.

> **Cobertura SOTA estimada apos plano completo: 11/12 features SOTA mapeadas** (review anterior identificou 6/12). MCP Server e A2A protocol permanecem como epicos futuros.

---

## Decisoes de Seguranca (Reuniao 20260423)

### S1: On-demand agents — CapabilitySet::read_only() por default

`AgentSpec::on_demand()` DEVE usar `CapabilitySet::read_only()` como default. O LLM NAO pode escalar capabilities via on-demand. Agentes com capabilities de escrita exigem spec registrado (builtin, global, ou project).

**Justificativa:** Sem esta restricao, o LLM pode criar agentes arbitrarios com acesso total a bash/edit/write, bypasando o CapabilityGate.

**Cap por sessao (A5):** alem do per-agent cap (max_iterations=10, timeout=120s), aplicar cap GLOBAL `max_on_demand_per_session: usize` em `AgentConfig` (default `20`). Evita runaway: LLM criando 50 on-demand sequencialmente custaria ate 6000 iteracoes. Quando o limite e atingido, `delegate_task` com nome desconhecido retorna erro tipado `DelegateError::OnDemandQuotaExceeded { used, limit }` e o LLM e instruido (no system prompt da Fase 4) a reusar agents ja registrados. Counter persiste no estado do `AgentRunEngine` (zera a cada sessao nova).

### S2: Override de builtins — intersecao, nunca escalacao

Quando um project/global agent tem o mesmo nome de um builtin, o `CapabilitySet` resultante e a INTERSECAO do builtin com o custom. Um `.theo/agents/explorer.md` pode RESTRINGIR o Explorer (remover tools), nunca ampliar (adicionar tools que o builtin nao tinha).

**Justificativa:** Previne supply-chain attack via `.theo/agents/` em repos clonados.

**Implementacao:** `CapabilitySet::intersect(&self, other: &CapabilitySet) -> CapabilitySet` — novo metodo. Semantica completa em "CapabilitySet::intersect" abaixo.

**Pre-requisito (G3):** O campo `allowed_tools` muda de `HashSet<String>` (com convencao "vazio = todas") para um enum explicito `AllowedTools::All | Only(BTreeSet<String>)`. Sem essa mudanca, a intersecao e ambigua: hoje `intersect({read}, {edit})` produziria `{}` que colide com a convencao "vazio = todas permitidas", abrindo um caminho de escalacao acidental. Ver "AllowedTools enum" abaixo.

### S3: User confirmation para project agents (primeira carga)

Na primeira vez que `.theo/agents/` de um projeto e carregado, exibir warning listando agents encontrados e pedir confirmacao. Persistir confirmacao em `.theo/.agents-approved`.

**Mecanismo (G1):**

1. **Computar fingerprint** — Para cada `.md` em `.theo/agents/`, calcular `SHA-256(content)`. O conjunto de fingerprints e o "agents manifest".
2. **Verificar manifest** — Ler `.theo/.agents-approved` (JSON: `{ "approved": [{"file": "name.md", "sha256": "..."}] }`). Se ausente ou desatualizado (qualquer fingerprint nao bate ou e novo), entrar em modo de confirmacao.
3. **Confirmar com usuario** — Listar agents pendentes (nome, descricao, capabilities efetivas pos-intersect) e perguntar:
   - CLI: prompt interativo (`y/N`). Em modo nao-interativo (CI, `--no-confirm`), agents nao-aprovados sao IGNORADOS com warning.
   - Desktop: dialog modal listando os agents.
4. **Persistir** — Em caso de aprovacao, gravar fingerprints atuais em `.theo/.agents-approved` (chmod 600).
5. **Invalidacao** — Qualquer modificacao no conteudo de uma spec gera novo SHA-256 → manifest invalida → nova confirmacao requerida na proxima carga.

**Flag de override:** `--trust-project-agents` para CI/automation aprova sem prompt (registra warning no log).

**O que NAO fazer:** Confirmacao por agent individual (UX ruim — usuario aprova em batch). Confirmacao por sessao (perde-se na proxima execucao). Hash do path em vez do conteudo (renomear bypassa).

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
///
/// Convenção de tipos numéricos (A1):
/// - Frontmatter sempre desserializa como `u32` (ver `RawAgentFrontmatter`).
/// - Conversão para `usize`/`u64` acontece em `AgentSpec::from_frontmatter()`.
/// - Justificativa: `u32` é portátil e suficiente para os ranges (max_iterations < 10k, timeout < 24h).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub name: String,                           // ID unico (e.g. "explorer", "my-reviewer")
    pub description: String,                    // Human-readable (para tool schema)
    pub system_prompt: String,                  // Body do markdown
    pub capability_set: CapabilitySet,          // Tools permitidas/negadas
    pub model_override: Option<String>,         // Override de modelo
    pub max_iterations: usize,                  // Loop limit (parsed from u32)
    pub timeout_secs: u64,                      // Wall-clock timeout (parsed from u32)
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

### AllowedTools enum (Seguranca G3)

O campo `allowed_tools` em `CapabilitySet` hoje e `HashSet<String>` com convencao implicita "vazio = todas permitidas". Isso quebra a intersecao da S2: `intersect({read}, {edit})` resulta em `HashSet vazio` que e (re-)interpretado como "todas permitidas" — exatamente o oposto do desejado.

**Mudanca:** substituir por enum explicito.

```rust
// theo-domain/src/capability.rs — MUDANCA INCOMPATIVEL (mas isolada)

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AllowedTools {
    /// All tools are permitted (subject to denied_tools).
    All,
    /// Only the listed tools are permitted (subject to denied_tools).
    Only(BTreeSet<String>),
}

impl AllowedTools {
    pub fn intersect(&self, other: &AllowedTools) -> AllowedTools {
        match (self, other) {
            (AllowedTools::All, x) | (x, AllowedTools::All) => x.clone(),
            (AllowedTools::Only(a), AllowedTools::Only(b)) => {
                AllowedTools::Only(a.intersection(b).cloned().collect())
            }
        }
    }

    pub fn contains(&self, tool: &str) -> bool {
        match self {
            AllowedTools::All => true,
            AllowedTools::Only(set) => set.contains(tool),
        }
    }
}

pub struct CapabilitySet {
    pub allowed_tools: AllowedTools,        // mudou: era HashSet<String>
    pub denied_tools: BTreeSet<String>,     // mudou: BTree para ordem deterministica
    pub allowed_categories: BTreeSet<String>,
    pub max_file_size_bytes: u64,
    pub allowed_paths: Vec<PathBuf>,
    pub network_access: bool,
}
```

**Migracao:** todos os call sites que faziam `allowed_tools: HashSet::new()` viram `allowed_tools: AllowedTools::All`. Sites que populavam o set viram `AllowedTools::Only(...)`. Esta mudanca e parte da Fase 1 (RED-GREEN antes de qualquer coisa).

**Frontmatter:** o campo YAML continua sendo `tools: []` (compat). Parser converte:
- `tools` ausente ou `[]` → `AllowedTools::All`
- `tools: [read, grep]` → `AllowedTools::Only({read, grep})`

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

**Frontmatter fields (A1: tipos sao u32 no YAML, convertidos no parser):**

| Campo | Tipo YAML | Tipo Rust apos parse | Default | Descricao |
|---|---|---|---|---|
| `name` | string | `String` | filename sem extensao | ID unico |
| `description` | string | `String` | obrigatorio | Para tool schema |
| `tools` | string[] | `AllowedTools::All` ou `Only(BTreeSet<String>)` | omitido ou `[]` → `All` | Allowed tools (G3) |
| `denied_tools` | string[] | `BTreeSet<String>` | `{}` | Denied tools (precedencia sobre allowed) |
| `model` | string | `Option<String>` | `None` (herda parent) | Model override |
| `max_iterations` | `u32` | `usize` (cast) | `30` | Loop limit |
| `timeout` | `u32` | `u64` (cast, em segundos) | `300` | Wall-clock timeout |

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

### Caminho de Injecao do Registry (A3)

O `SubAgentRegistry` precisa estar disponivel em **todo lugar que hoje cria `SubAgentManager::new(...)` localmente**. Hoje sao 3 call sites em `run_engine.rs`:

| Local | Caso de uso atual | Mudanca |
|---|---|---|
| `run_engine.rs:1361` | dispatch de `subagent` | recebe `Arc<SubAgentRegistry>` via `self.subagent_registry` |
| `run_engine.rs:1427` | dispatch de `subagent_parallel` | idem |
| `run_engine.rs:1509` | dispatch de `skill` no modo `SkillMode::SubAgent` | idem |

**Caminho de propagacao:**

```
apps/theo-cli + apps/theo-desktop
  ↓ constroi SubAgentRegistry::with_builtins().load_all(project_dir)
theo-application (use case Run/Chat)
  ↓ injeta Arc<SubAgentRegistry> em AgentRunEngine::new(...)
theo-agent-runtime::AgentRunEngine
  ↓ guarda self.subagent_registry: Arc<SubAgentRegistry>
SubAgentManager::new(config, event_bus, project_dir, registry.clone())
```

**Mudanca em `AgentRunEngine`:**

```rust
pub struct AgentRunEngine {
    // ... campos existentes ...
    subagent_registry: Arc<SubAgentRegistry>,  // NOVO
}

impl AgentRunEngine {
    pub fn new(
        // ... params existentes ...
        subagent_registry: Arc<SubAgentRegistry>,  // NOVO
    ) -> Self;
}
```

**Mudanca em `SkillMode::SubAgent`:**

```rust
// ANTES: tipo do role
pub enum SkillMode {
    InContext,
    SubAgent { role: SubAgentRole },
}

// DEPOIS: nome resolvido contra o registry em runtime
pub enum SkillMode {
    InContext,
    SubAgent { agent_name: String },
}
```

`bundled.rs` migra de `SubAgentRole::Verifier` para `agent_name: "verifier".to_string()`. Skill files usuario continuam com `mode: subagent` + `subagent_role: <name>` no frontmatter — apenas a string e armazenada agora.

**Risco coberto:** se `agent_name` nao existe no registry no momento do dispatch (ex: usuario removeu o `.md`), retornar erro tipado `SkillError::AgentNotFound { skill, agent }` em vez de fallback silencioso. Test de regressao na Fase 4: `test_skill_with_unknown_agent_returns_typed_error`.

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

Apenas 2 campos novos visíveis ao MVP. **Status v3.1**: `AgentResult` agora carrega `agent_name`, `context_used`, `structured` (Phase 7), `cancelled` (Phase 6) e `worktree_path` (Phase 11). `AgentFinding`/`FindingSeverity` permanecem como evolucao futura — `structured` aceita `serde_json::Value` arbitrario, suficiente para todos os schemas atuais.

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
    /// Semantica completa:
    /// - denied_tools  = UNION                  (se qualquer um nega, esta negado)
    /// - allowed_tools = AllowedTools::intersect (G3 — All is identity, Only intersecta)
    /// - allowed_categories = INTERSECTION
    /// - network_access = AND                    (ambos devem permitir)
    /// - max_file_size_bytes = MIN               (mais restritivo)
    /// - allowed_paths = path-prefix-containment intersection (ver abaixo)
    ///
    /// Path-prefix-containment intersection (G2):
    /// - Se ambos sao vazios → vazio (= "all paths" pela convencao atual)
    /// - Se apenas um e vazio → o outro vence (mais restritivo)
    /// - Se ambos nao-vazios → para cada path P em `other`, manter P se houver
    ///   algum prefixo em `self.allowed_paths` que o contenha (ou seja, se
    ///   `self` ja autorizava acesso a P, o intersect mantem). Implementacao:
    ///   `path.starts_with(prefix)` para cada par.
    ///
    /// Exemplo:
    ///   self.allowed_paths  = ["/repo/src"]
    ///   other.allowed_paths = ["/repo/src/lib", "/tmp"]
    ///   intersect           = ["/repo/src/lib"]   // /tmp nao esta sob /repo/src
    pub fn intersect(&self, other: &CapabilitySet) -> CapabilitySet {
        // Pseudocodigo de referencia:
        let denied_tools  = self.denied_tools.union(&other.denied_tools).cloned().collect();
        let allowed_tools = self.allowed_tools.intersect(&other.allowed_tools);
        let allowed_paths = match (self.allowed_paths.is_empty(), other.allowed_paths.is_empty()) {
            (true, true)   => Vec::new(),
            (true, false)  => other.allowed_paths.clone(),
            (false, true)  => self.allowed_paths.clone(),
            (false, false) => other.allowed_paths.iter()
                .filter(|p| self.allowed_paths.iter().any(|prefix| p.starts_with(prefix)))
                .cloned().collect(),
        };
        // ... demais campos
    }
}
```

**Convencao path-prefix-containment justificada:** Paths sao restricoes hierarquicas, nao membership pura. A semantica "manter apenas o que ambos autorizavam" e a unica que respeita a regra "intersect e sempre <= self". Uniao seria escalacao (proibida pela S2). Intersecao set-theoretic literal seria sempre vazia para paths concretos diferentes.

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

### D6: Schema Regeneration — Estatica por Sessao (A2)

`registry.build_tool_description()` e chamado uma unica vez quando o tool bridge e construido (inicio da sessao). On-demand agents criados em runtime via `delegate_task` sao registrados no registry, mas a descricao da tool **nao** e regenerada e enviada novamente ao LLM.

**Decisao consciente:** schema estatico por sessao. Justificativa:

1. **Custo de regeneracao** — re-emitir o schema implica re-priming do contexto do LLM (cache invalidation). Nao vale o custo para listar agents que o LLM acabou de criar.
2. **LLM ainda pode invocar** — `delegate_task` aceita qualquer string em `agent`. Se o nome nao esta no registry, vira on-demand (rota S1). O nome de um agent recem-criado funciona sem schema update.
3. **Hot-reload de `.theo/agents/`** — fora de escopo (YAGNI). Adicionar agents requer reiniciar a sessao.

**Trade-off aceito:** A "lista de agents disponiveis" no schema reflete apenas builtins + custom carregados na partida. On-demand criados durante a sessao nao aparecem la. Documentar no system prompt.

**Quando reavaliar:** se telemetria mostrar que >10% das invocacoes on-demand sao re-uses do mesmo nome dentro da mesma sessao, considerar regeneracao incremental.

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
| `crates/theo-domain/src/capability.rs` | `AllowedTools` enum (G3); `BTreeSet` para denied_tools; `CapabilitySet::intersect()` (S2/G2) |
| `crates/theo-domain/src/event.rs` | Variantes `SubagentStarted`, `SubagentCompleted` |
| `crates/theo-agent-runtime/src/config.rs` | `AgentConfig.max_on_demand_per_session: usize` (default 20) — A5 |
| `crates/theo-agent-runtime/src/subagent/mod.rs` | `SubAgentManager` com 7 builders (registry, run_store, hooks, cancellation, checkpoint, worktree_provider, metrics, mcp_registry). Apenas `spawn_with_spec` — `spawn(role)` e `spawn_parallel` REMOVIDOS na v3.1. |
| `crates/theo-agent-runtime/src/tool_bridge.rs` | Substituir `subagent`/`subagent_parallel` por `delegate_task` com schema dinamico |
| `crates/theo-agent-runtime/src/run_engine.rs` | `AgentRunEngine.subagent_registry: Arc<SubAgentRegistry>`. Dispatch de `delegate_task` nos 3 sites antigos (linhas 1361/1427/1509). Counter de on-demand por sessao (A5). |
| `crates/theo-agent-runtime/src/agent_loop.rs` | `AgentResult` ganha `agent_name`, `context_used` |
| `crates/theo-agent-runtime/src/skill/mod.rs` | Migrar para usar `frontmatter::split_frontmatter()`. `SkillMode::SubAgent { agent_name: String }` usa registry lookup. |
| `crates/theo-agent-runtime/src/skill/bundled.rs` | `SubAgentRole::Verifier` → `agent_name: "verifier".into()` (A3) |
| `crates/theo-application/src/...` | Construir `Arc<SubAgentRegistry>` e injetar em `AgentRunEngine::new()` (A3) |
| `apps/theo-cli/src/...` | `SubAgentRegistry::with_builtins().load_all(project_dir)` na inicializacao (A3) |
| `apps/theo-desktop/src/...` | Idem CLI (A3) |

### Dependencias novas

| Crate | Dependencia | Motivo |
|---|---|---|
| `theo-agent-runtime` | `serde_yaml` (workspace) | Parse YAML frontmatter |
| `theo-agent-runtime` | `indexmap` (workspace) | SubAgentRegistry preserva ordem |

Adicionar ambas a `[workspace.dependencies]` no root `Cargo.toml`.

---

## Fases de Implementacao

# TRACK A — Fundacao MVP (Fases 1-4)

> **Objetivo do track:** Substituir `SubAgentRole` hardcoded por sistema dinamico de specs (built-in + custom + on-demand). MVP entregavel sozinho.
> **Pre-requisito:** Nenhum.
> **Evidencia direta:** OpenDev `AgentConfigInline`, Claude Code `.claude/agents/*.md`, Hermes `delegate_task`.

### Fase 1: Domain Types + Builtins + Registry

**Objetivo:** AgentSpec como tipo central, 4 builtins extraidos, registry funcional.

**TDD Sequence:**

```
RED (AllowedTools — G3, PRE-REQUISITO):
  #[test] fn test_allowed_tools_all_intersect_only_returns_only()
  #[test] fn test_allowed_tools_only_intersect_only_returns_set_intersection()
  #[test] fn test_allowed_tools_only_disjoint_returns_empty_only()
  #[test] fn test_allowed_tools_contains_all_returns_true_for_any_tool()
  #[test] fn test_allowed_tools_contains_only_respects_set()
  → cargo test -p theo-domain → FAIL

GREEN (G3):
  1. Criar AllowedTools enum em theo-domain/src/capability.rs
  2. Migrar CapabilitySet.allowed_tools: HashSet<String> → AllowedTools
  3. Migrar CapabilitySet.denied_tools: HashSet → BTreeSet (determinismo)
  4. Atualizar CapabilitySet::read_only() e ::unrestricted() para o novo enum
  5. Atualizar todos os call sites em theo-tooling, theo-agent-runtime
  → cargo test -p theo-domain && cargo test → PASS

RED (AgentSpec):
  #[test] fn test_agent_spec_on_demand_is_read_only()
  #[test] fn test_agent_spec_on_demand_max_iterations_capped_at_10()
  #[test] fn test_agent_spec_on_demand_timeout_120s()
  #[test] fn test_agent_spec_role_id_returns_correct_id()
  #[test] fn test_agent_spec_source_serde_roundtrip()
  → cargo test -p theo-domain → FAIL (tipos nao existem)

GREEN:
  1. Criar theo-domain/src/agent_spec.rs (AgentSpec, AgentSpecSource)
  2. Adicionar pub mod agent_spec em theo-domain/src/lib.rs
  3. Cargo test → PASS

RED (intersect — S2, G2):
  #[test] fn test_capability_set_intersect_denied_tools_union()
  #[test] fn test_capability_set_intersect_allowed_tools_uses_allowedtools_intersect()
  #[test] fn test_capability_set_intersect_network_access_and()
  #[test] fn test_capability_set_intersect_max_file_size_min()
  #[test] fn test_capability_set_intersect_paths_both_empty_returns_empty()       // G2
  #[test] fn test_capability_set_intersect_paths_other_empty_returns_self()       // G2
  #[test] fn test_capability_set_intersect_paths_self_empty_returns_other()       // G2
  #[test] fn test_capability_set_intersect_paths_prefix_containment()             // G2
  #[test] fn test_capability_set_intersect_paths_disjoint_returns_empty()         // G2
  → cargo test -p theo-domain → FAIL

GREEN:
  1. Implementar CapabilitySet::intersect() com semantica completa documentada
  2. Path-prefix-containment para allowed_paths
  → cargo test -p theo-domain → PASS

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

RED (S3 / G1 — .theo/.agents-approved):
  #[test] fn test_compute_agents_manifest_returns_sha256_per_file()
  #[test] fn test_load_approved_manifest_returns_empty_when_file_absent()
  #[test] fn test_load_approved_manifest_parses_valid_json()
  #[test] fn test_load_approved_manifest_returns_error_on_invalid_json()
  #[test] fn test_unapproved_specs_are_filtered_out_when_no_confirm()      // CI mode
  #[test] fn test_modified_spec_invalidates_previous_approval()             // hash change
  #[test] fn test_persist_approved_manifest_writes_chmod_600()
  #[test] fn test_trust_project_agents_flag_bypasses_prompt_with_warning()
  #[test] fn test_load_project_agents_unmodified_skips_prompt()             // happy path
  → cargo test → FAIL

GREEN:
  1. Criar subagent/approval.rs:
     - struct AgentManifest { approved: Vec<ApprovedAgent> }
     - struct ApprovedAgent { file: String, sha256: String }
     - fn compute_manifest(dir: &Path) -> Result<AgentManifest, IoError>
     - fn load_approved(project_dir: &Path) -> Result<AgentManifest, ApprovalError>
     - fn persist_approved(project_dir: &Path, manifest: &AgentManifest, mode: 0o600)
     - fn diff_manifest(current, approved) -> Vec<UnapprovedSpec>
  2. Adicionar enum ApprovalMode { Interactive, NonInteractive, TrustAll }
     ao SubAgentRegistry::load_all(project_dir, mode)
  3. Em modo Interactive sem aprovacao previa, registry NAO carrega specs pendentes
     (callers — CLI/Desktop — fazem o prompt e chamam persist_approved)
  → cargo test → PASS

REFACTOR:
  - Migrar skill/mod.rs para usar frontmatter::split_frontmatter()
  - Verificar que testes de skills continuam passando
```

**Nota sobre integracao do prompt (S3):** A logica de PROMPT (CLI vs Desktop) vive nos apps, nao no registry. O registry apenas:
1. Computa o manifest atual.
2. Carrega o manifest aprovado.
3. Retorna `(loaded_specs, pending_specs)`.

O `theo-cli` faz prompt textual; `theo-desktop` abre dialog Tauri. Em ambos, apos confirmacao, chamam `persist_approved()` e re-carregam o registry. Mantem `theo-agent-runtime` puro de UI.

**Verify:** `cargo test -p theo-agent-runtime -- frontmatter && cargo test -p theo-agent-runtime -- parser && cargo test -p theo-agent-runtime -- registry && cargo test -p theo-agent-runtime -- skill`

### Fase 3: Refatorar SubAgentManager

**Objetivo:** SubAgentManager usa AgentSpec/Registry. Eventos emitidos. Backward compat.

**Risco principal:** 530+ testes existentes podem quebrar. **Status v3.1**: TODOS os testes legacy migrados para spec-based API. `SubAgentRole`, `spawn(role)`, `spawn_parallel`, handlers `subagent`/`subagent_parallel` REMOVIDOS. ~2000 testes passando, zero regressão.

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
     - with_builtins() — convenience constructor (com 4 builtins pre-loaded)
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
  #[test] fn test_delegate_task_on_demand_quota_exceeded_returns_typed_error() // A5
  #[test] fn test_delegate_task_on_demand_counter_resets_per_session()         // A5
  → cargo test → FAIL

GREEN:
  1. Substituir subagent + subagent_parallel por delegate_task em tool_bridge.rs
  2. Schema dinamico via registry.build_tool_description() (decisao D6: estatico por sessao)
  3. Dispatch em run_engine.rs (3 sites: 1361/1427/1509 antigos consolidados):
     - Se agent no registry → spawn com spec
     - Se agent NAO no registry → AgentSpec::on_demand() (read_only, S1)
       - Verificar counter < AgentConfig.max_on_demand_per_session (A5) antes
       - Incrementar counter; se >= limite, retornar DelegateError::OnDemandQuotaExceeded
     - Se parallel → spawn_parallel
     - Se ambos agent+parallel → erro
  4. Atualizar registry_to_definitions() e registry_to_definitions_for_subagent()
  → cargo test → PASS

RED (A3 — Skill migration):
  #[test] fn test_skill_subagent_mode_uses_registry()
  #[test] fn test_skill_with_unknown_agent_returns_typed_error()  // A3 — SkillError::AgentNotFound
  #[test] fn test_skill_bundled_test_skill_resolves_verifier_from_registry()
  → cargo test → FAIL (SkillMode::SubAgent ainda usa SubAgentRole)

GREEN:
  1. Migrar SkillMode::SubAgent { role: SubAgentRole } → { agent_name: String }
  2. Em skill/bundled.rs: substituir SubAgentRole::Verifier → "verifier".to_string()
  3. Em run_engine.rs:1509: lookup no self.subagent_registry, retornar SkillError::AgentNotFound se nao achar
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

# TRACK B — SOTA Lifecycle (Fases 5-7)

> **Objetivo do track:** Tornar o sistema de sub-agents observavel, controlavel e extensivel via callbacks externos.
> **Pre-requisito:** Track A (Fases 1-4) concluido.
> **Evidencia direta:** Archon `hooks.ts` (22 events Claude SDK), OpenDev `opendev-hooks/`, OpenAI Agents SDK guardrails.

### Fase 5: Lifecycle Hooks System

**Objetivo:** Sistema de hooks alinhado com Claude Agent SDK (22 events). Permite extensao comportamental SEM modificar codigo do runtime.

**Evidencia:** `referencias/Archon/packages/workflows/src/schemas/hooks.ts:10-88` define 22 events e `workflowHookMatcherSchema { matcher: regex, response: object, timeout: u32 }`. OpenDev separa `HookExecutor` + `HookManager` (SRP).

**Eventos a suportar (matching Claude SDK):**

| Categoria | Events |
|---|---|
| Tool lifecycle | `PreToolUse`, `PostToolUse`, `PostToolUseFailure` |
| Session lifecycle | `SessionStart`, `SessionEnd`, `Stop` |
| Sub-agent lifecycle | `SubagentStart`, `SubagentStop` |
| Prompting | `UserPromptSubmit`, `Notification`, `PreCompact` |
| Permissions | `PermissionRequest` |
| Worktree (Track D) | `WorktreeCreate`, `WorktreeRemove` |
| Misc | `Setup`, `TaskCompleted`, `ConfigChange`, `InstructionsLoaded` |

**Arquitetura:**

```rust
// theo-agent-runtime/src/hooks/mod.rs (NOVO)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookEvent {
    PreToolUse { tool_name: String, args: Value },
    PostToolUse { tool_name: String, args: Value, result: Value },
    SubagentStart { agent_name: String, objective: String },
    SubagentStop { agent_name: String, success: bool, summary: String },
    // ... 18 outros
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookMatcher {
    pub matcher: Option<String>,    // regex (e.g. "^edit|write$" para tool names)
    pub response: HookResponse,     // o que retornar quando dispara
    pub timeout_secs: Option<u32>,  // default 60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookResponse {
    Allow,                              // continue
    Block { reason: String },           // bloqueia tool/operacao
    Replace { value: Value },           // substitui input
    InjectContext { content: String },  // injeta no contexto (UserPromptSubmit pattern do Rippletide)
}

pub struct HookManager {
    hooks: BTreeMap<String, Vec<HookMatcher>>, // event_name → matchers
}

impl HookManager {
    pub async fn dispatch(&self, event: &HookEvent) -> HookResponse;
    pub fn from_agent_spec(spec: &AgentSpec) -> Self;  // hooks per-agent
    pub fn from_global_config(config: &AgentConfig) -> Self;  // hooks globais
}
```

**Hooks no frontmatter de custom agents:**

```yaml
---
name: security-reviewer
description: "..."
hooks:
  PreToolUse:
    - matcher: "^bash$"
      response: { type: block, reason: "bash forbidden in security review" }
  PostToolUse:
    - matcher: "^edit|write$"
      response: { type: block, reason: "this agent is read-only" }
  UserPromptSubmit:
    - response: { type: inject_context, content: "Focus on OWASP Top 10." }
---
```

**TDD Sequence:**

```
RED:
  #[test] fn test_hook_event_serde_roundtrip_for_all_22_variants()
  #[test] fn test_hook_matcher_regex_matches_tool_name()
  #[test] fn test_hook_response_block_prevents_tool_execution()
  #[test] fn test_hook_response_inject_context_prepends_to_messages()
  #[test] fn test_hook_response_replace_substitutes_args()
  #[test] fn test_hook_dispatch_pretooluse_called_before_tool_execution()
  #[test] fn test_hook_dispatch_subagent_start_emitted_with_correct_payload()
  #[test] fn test_hook_dispatch_subagent_stop_includes_metrics()
  #[test] fn test_hook_dispatch_timeout_returns_default_allow()
  #[test] fn test_hook_per_agent_overrides_global_for_same_event()
  #[test] fn test_hook_pretooluse_block_records_in_trajectory()
  #[test] fn test_hook_yaml_frontmatter_parses_into_hookmanager()
  → cargo test → FAIL

GREEN:
  1. Criar hooks/{mod,event,matcher,manager,executor}.rs
  2. Estender AgentSpec com hooks: Option<HookManager>
  3. Estender AgentConfig com global_hooks: HookManager
  4. AgentRunEngine.run_step():
     - Antes de cada tool dispatch → hook_manager.dispatch(PreToolUse)
     - Apos cada tool dispatch → hook_manager.dispatch(PostToolUse)
  5. SubAgentManager.spawn(): emite SubagentStart antes, SubagentStop apos
  6. parser.rs: estende para parsear bloco hooks: do YAML
  → cargo test → PASS

REFACTOR:
  - HookExecutor separado de HookManager (SRP)
  - Tipo HookError com variantes (Timeout, RegexInvalid, ResponseSchemaMismatch)
```

**Decisoes:**
- **Hooks sao deterministicos** (response estatica no YAML), NAO sao tool calls. LLM nao decide hooks. Justificativa: previne loops e custos.
- **Hooks per-agent** sao mesclados com globais. Per-agent ganha em conflito.
- **Timeout default 60s** (alinhado Claude SDK). Timeout = response Allow (fail-open para evitar deadlock).

**Verify:** `cargo test -p theo-agent-runtime -- hooks`

### Fase 6: Cooperative Cancellation (parent → child)

**Objetivo:** Quando o parent cancela (Ctrl+C, timeout, erro), todos os sub-agents ativos devem terminar limpamente.

**Evidencia:** `referencias/opendev/docs/subagent-execution-model.md:75` — "Each subagent has its own cancellation token". Cancellation token cooperativa e padrao em Tokio.

**Hoje:** quando o parent retorna, sub-agents continuam rodando ate timeout natural ou max_iterations. Desperdiça tokens, polui logs, pode escrever em CWD apos parent terminar.

**Arquitetura:**

```rust
// theo-agent-runtime/src/cancellation.rs (NOVO)

use tokio_util::sync::CancellationToken;

pub struct CancellationTree {
    root: CancellationToken,
    children: DashMap<String, CancellationToken>,  // agent_name → token
}

impl CancellationTree {
    pub fn root() -> Self;
    pub fn child(&self, agent_name: &str) -> CancellationToken {
        let token = self.root.child_token();
        self.children.insert(agent_name.into(), token.clone());
        token
    }
    pub fn cancel_all(&self) { self.root.cancel(); }
    pub fn cancel_agent(&self, name: &str);  // cancel especifico
}
```

**Pontos de check (cooperativo):**
- Inicio de cada iteracao em `AgentLoop::run()`
- Antes de cada LLM call (`tokio::select!` token vs request)
- Antes de cada tool dispatch
- Apos cada tool result

**TDD Sequence:**

```
RED:
  #[test] async fn test_cancellation_root_propagates_to_all_children()
  #[test] async fn test_cancellation_specific_agent_does_not_affect_siblings()
  #[test] async fn test_cancelled_subagent_returns_with_cancelled_status()
  #[test] async fn test_cancellation_during_llm_call_aborts_request()
  #[test] async fn test_cancellation_during_tool_execution_aborts_tool()
  #[test] async fn test_cancellation_emits_subagent_stop_event()
  → cargo test → FAIL

GREEN:
  1. Criar cancellation.rs com CancellationTree
  2. AgentRunEngine.cancellation_tree: Arc<CancellationTree>
  3. SubAgentManager.spawn(): cria child token, passa pra child loop
  4. AgentLoop.run(): tokio::select! { _ = token.cancelled() => return Cancelled, ... }
  5. AgentResult ganha variante Cancelled (alem de Success/Failure)
  → cargo test → PASS
```

**Verify:** `cargo test -p theo-agent-runtime -- cancellation`

### Fase 7: Output Format Schema (D3 enforcement opcional)

**Objetivo:** Permitir specs declararem JSON schema para output estruturado. Parser tenta validar; se falha, mantem free-text (D3 fallback).

**Evidencia:** Archon `dag-node.ts` `output_format` field — Claude/Codex via SDK enforcement, Pi via prompt augmentation. OpenAI Agents SDK `response_format`.

**Frontmatter extension:**

```yaml
---
name: code-reviewer
output_format:
  schema:
    type: object
    required: [findings]
    properties:
      findings:
        type: array
        items:
          type: object
          required: [severity, file, message]
          properties:
            severity: { enum: [critical, high, medium, low] }
            file: { type: string }
            line: { type: integer }
            message: { type: string }
  enforcement: best_effort  # 'strict' | 'best_effort'
---
```

**Comportamento:**
- `strict`: parser falha se output nao bate. Sub-agent retorna erro.
- `best_effort`: parser tenta JSON.parse + jsonschema valida; se falha, `AgentResult.structured = None` e `summary` mantem free-text. **Default.**

**TDD Sequence:**

```
RED:
  #[test] fn test_parse_output_format_strict_valid_returns_structured()
  #[test] fn test_parse_output_format_strict_invalid_returns_error()
  #[test] fn test_parse_output_format_best_effort_invalid_falls_back_to_text()
  #[test] fn test_agent_result_structured_field_typed()
  → cargo test → FAIL

GREEN:
  1. AgentSpec ganha output_format: Option<OutputFormat>
  2. AgentResult ganha structured: Option<Value>
  3. Em SubAgentManager.spawn(): apos done, parsear summary contra schema
  4. Usar `jsonschema` crate (workspace dep)
  → cargo test → PASS
```

**Decisao:** NAO criar tipo Rust dedicado para findings. Usa `serde_json::Value`. Quando tivermos 3 specs distintas com schemas similares, ai tipamos (regra de 3 / DRY).

**Verify:** `cargo test -p theo-agent-runtime -- output_format`

---

# TRACK C — SOTA Integration (Fases 8-10)

> **Objetivo do track:** Conectar sub-agents ao ecossistema externo (MCP) e adicionar resiliencia operacional (checkpoints, resume).
> **Pre-requisito:** Track A. Independente de Track B.
> **Evidencia direta:** OpenDev `opendev-mcp/`, Hermes `checkpoint_manager.py`, Archon `workflow_runs` schema.

### Fase 8: MCP Client Integration

**Objetivo:** Sub-agents podem consumir MCP servers externos como tools. Permite plugar databases, APIs, ferramentas IDE.

**Evidencia:**
- Spec oficial: `modelcontextprotocol.io` (Anthropic, 2024-11) — JSON-RPC 2.0 sobre stdio/HTTP
- OpenDev: crate dedicado `referencias/opendev/crates/opendev-mcp/`
- Hermes: `mcp_tool.py` (~1050 LOC) + `mcp_oauth_manager.py`
- QMD: README.md:72-137 — stdio (default) + HTTP daemon mode

**Por que e SOTA-blocker:** Claude Code, Cursor, Continue todos consomem MCP. Sem MCP, Theo Code esta fora do ecossistema 2025-2026 de tool integrations.

**Arquitetura:**

```
crates/theo-infra-mcp/                  (NOVO crate, segue regra de bounded context)
  Cargo.toml
  src/
    lib.rs
    client.rs           # McpClient trait (stdio, http transports)
    transport_stdio.rs  # JSON-RPC over stdio (subprocess)
    transport_http.rs   # JSON-RPC over HTTP (Streamable HTTP)
    discovery.rs        # tools/list, resources/list
    auth.rs             # OAuth 2.1 manager
    config.rs           # McpServerConfig (name, command, env, args)
    error.rs            # McpError typed
```

**Dependencia (ADR alignment):**

```
theo-infra-mcp → theo-domain        (puro, segue regra)
theo-agent-runtime → theo-infra-mcp (NOVO — adicionar a ADR-016 dependency bound)
```

**Configuracao MCP servers:**

```yaml
# .theo/mcp.yaml
servers:
  - name: github
    transport: stdio
    command: npx
    args: [-y, "@modelcontextprotocol/server-github"]
    env:
      GITHUB_TOKEN: "${env:GITHUB_TOKEN}"
  - name: postgres
    transport: http
    url: http://localhost:8080
    auth:
      type: oauth2
      client_id: "..."
```

**MCP tools no frontmatter de agents:**

```yaml
---
name: db-explorer
description: "Query databases via MCP"
mcp_servers: [postgres, github]   # quais MCP servers este agent pode usar
tools: []                          # tools normais
denied_tools: [edit, write, bash]  # apenas MCP + read
---
```

**TDD Sequence:**

```
RED:
  #[test] async fn test_stdio_transport_sends_jsonrpc_request()
  #[test] async fn test_stdio_transport_parses_jsonrpc_response()
  #[test] async fn test_http_transport_streams_responses()
  #[test] async fn test_discovery_lists_tools_from_server()
  #[test] async fn test_discovery_lists_resources_from_server()
  #[test] async fn test_oauth_manager_refreshes_expired_token()
  #[test] async fn test_mcp_tool_call_returns_result()
  #[test] async fn test_mcp_tool_call_handles_server_error()
  #[test] async fn test_mcp_server_subprocess_killed_on_drop()
  → cargo test → FAIL

GREEN:
  1. cargo new --lib crates/theo-infra-mcp + adicionar ao workspace
  2. Implementar transports (stdio via tokio::process, http via reqwest)
  3. Implementar discovery (cache tools/list result com TTL)
  4. Implementar auth manager (OAuth 2.1 flow + token refresh)
  5. Estender ToolRegistry para incluir MCP tools como ToolDefinition dinamicas
  6. AgentSpec.mcp_servers: Vec<String> filtra quais servers o agent ve
  → cargo test → PASS

INTEGRATION:
  1. Spawn @modelcontextprotocol/server-filesystem como teste
  2. Listar tools, chamar uma, validar resposta
```

**Decisoes:**
- **OAuth 2.1 obrigatorio** para HTTP transport (spec MCP). Stdio transport assume confianca local.
- **Tool name disambiguation**: MCP tools sao prefixadas `mcp:server_name:tool_name` para evitar colisao com tools nativas
- **Capability gating**: MCP tools passam pelo CapabilityGate normal. `denied_tools: [mcp:github:*]` funciona com glob.
- **NAO** implementar MCP Server (Theo como server) nesta fase. Epico futuro.

**Verify:** `cargo test -p theo-infra-mcp && cargo test -p theo-agent-runtime -- mcp_integration`

### Fase 9: Checkpoint Manager (Shadow Git Repos)

**Objetivo:** Snapshot automatico do CWD antes de mutacoes (write/edit/patch). Permite rollback de qualquer ponto da sessao.

**Evidencia:** `referencias/hermes-agent/tools/checkpoint_manager.py:1-90`. Shadow git repo em `~/.theo/checkpoints/{sha256(abs_dir)[:16]}/` usando `GIT_DIR + GIT_WORK_TREE`. NAO e tool — infraestrutura transparente. Disparado uma vez por turno.

**Por que SOTA:** Hermes implementa, Cursor tem checkpoint UI. Diferencia ferramenta confiavel de "destruidora de codigo".

**Arquitetura:**

```rust
// theo-agent-runtime/src/checkpoint/mod.rs (NOVO)

pub struct CheckpointManager {
    base_dir: PathBuf,           // ~/.theo/checkpoints
    workdir: PathBuf,            // CWD do user
    shadow_dir: PathBuf,         // {base}/{sha16(workdir)}
    excludes: Vec<String>,       // node_modules/, .git/, .env, etc.
    max_files: usize,            // 50_000 (mesma protecao Hermes)
}

impl CheckpointManager {
    pub fn for_workdir(workdir: &Path) -> Result<Self, CheckpointError>;

    /// Snapshot atual do workdir. Retorna commit SHA.
    pub async fn snapshot(&self, label: &str) -> Result<String, CheckpointError>;

    /// Lista checkpoints (commit SHA + label + timestamp).
    pub fn list(&self) -> Result<Vec<Checkpoint>, CheckpointError>;

    /// Restore para um checkpoint. Valida hash com regex.
    pub async fn restore(&self, commit: &str) -> Result<(), CheckpointError>;

    /// Cleanup: remove checkpoints > N dias.
    pub fn cleanup(&self, max_age_days: u32) -> Result<usize, CheckpointError>;
}
```

**Trigger automatico:** Em `AgentRunEngine`, antes do PRIMEIRO `edit`/`write`/`apply_patch`/`bash` write de cada turno, chama `checkpoint_manager.snapshot(format!("turn-{}", turn_id))`. Default ON; CLI flag `--no-checkpoints` para opt-out.

**TDD Sequence:**

```
RED:
  #[test] async fn test_checkpoint_creates_shadow_repo_at_correct_path()
  #[test] async fn test_checkpoint_uses_sha256_16_chars_of_workdir()
  #[test] async fn test_checkpoint_snapshot_creates_commit()
  #[test] async fn test_checkpoint_excludes_node_modules_and_dotenv()
  #[test] async fn test_checkpoint_skips_if_more_than_max_files()
  #[test] async fn test_checkpoint_restore_reverts_workdir()
  #[test] async fn test_checkpoint_restore_rejects_invalid_commit_hash()
  #[test] async fn test_checkpoint_restore_rejects_dash_prefix_injection()
  #[test] async fn test_checkpoint_cleanup_removes_old_only()
  #[test] async fn test_checkpoint_does_not_leak_git_state_into_workdir()
  → cargo test → FAIL

GREEN:
  1. Criar checkpoint/{mod,manager,git_shadow,validate}.rs
  2. Validacao de commit hash (regex 4-64 hex) — patterns Hermes
  3. Subprocess git com GIT_DIR + GIT_WORK_TREE explicitos
  4. Excludes via info/exclude do shadow repo (nao polui .gitignore do user)
  5. AgentRunEngine.checkpoint_before_mutation()
  6. CLI: theo checkpoints {list, restore <sha>, cleanup [days]}
  → cargo test → PASS

INTEGRATION:
  1. Tempdir com files
  2. Snapshot → editar → snapshot → editar → restore primeiro snapshot
  3. Verificar files restaurados
```

**Decisoes:**
- **NAO e tool** (LLM nao ve, nao chama). Infraestrutura transparente.
- **Validacao de commit hash com regex** (Hermes `_COMMIT_HASH_RE`) previne `git checkpoint --` style injection.
- **Excludes deterministicos** (lista fixa, nao read do .gitignore do user — evita exfiltrar segredos).
- **Storage:** ~50KB-5MB por turno. Cleanup default 30 dias.

**Verify:** `cargo test -p theo-agent-runtime -- checkpoint`

### Fase 10: Session Persistence + Resume

**Objetivo:** Sub-agent runs persistidos em SQLite. CLI `theo run resume <run-id>` retoma de onde parou.

**Evidencia:** Archon `CLAUDE.md` schema "Database Schema" — `workflow_runs`, `workflow_events` (step transitions, artifacts, errors), `sessions` com `parent_session_id` + `transition_reason` (audit trail). CLI: `bun cli workflow resume <run-id>` (skip completed nodes).

**Hoje:** sub-agent crash/Ctrl+C = perda total. Tokens gastos perdidos.

**Schema (SQLite):**

```sql
-- migrations/001_subagent_runs.sql

CREATE TABLE subagent_runs (
    run_id TEXT PRIMARY KEY,           -- UUID
    parent_run_id TEXT,                -- run do parent (null se root)
    agent_name TEXT NOT NULL,
    agent_source TEXT NOT NULL,        -- builtin|project|global|on_demand
    objective TEXT NOT NULL,
    status TEXT NOT NULL,              -- running|completed|failed|cancelled|abandoned
    started_at INTEGER NOT NULL,       -- unix epoch
    finished_at INTEGER,
    iterations_used INTEGER DEFAULT 0,
    tokens_used INTEGER DEFAULT 0,
    summary TEXT,                       -- AgentResult.summary se concluido
    structured_output TEXT,             -- JSON se output_format usado (Fase 7)
    cwd TEXT NOT NULL,
    checkpoint_before TEXT,             -- commit SHA pre-run (Fase 9)
    config_snapshot TEXT NOT NULL       -- AgentSpec serializada
);

CREATE TABLE subagent_events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES subagent_runs(run_id),
    event_type TEXT NOT NULL,           -- iteration_started|tool_call|llm_call|done
    payload TEXT NOT NULL,              -- JSON
    timestamp INTEGER NOT NULL
);

CREATE INDEX idx_subagent_runs_status ON subagent_runs(status);
CREATE INDEX idx_subagent_runs_parent ON subagent_runs(parent_run_id);
CREATE INDEX idx_subagent_events_run ON subagent_events(run_id, timestamp);
```

**CLI:**

```bash
theo run list                    # lista runs ativos + recentes
theo run status <run-id>         # detalhes de um run
theo run resume <run-id>         # retoma run failed/cancelled
theo run abandon <run-id>        # marca como abandonado
theo run cleanup [--days 7]      # remove runs terminais antigos
```

**Resume semantics (Archon-style):** Se um sub-agent fez 5 iteracoes e crashou, resume reconstroi: history completa de events + arquivos modificados ate o ponto de falha. Continua iteracao 6 com mesmo AgentSpec.

**TDD Sequence:**

```
RED:
  #[test] fn test_subagent_run_persisted_on_start()
  #[test] fn test_subagent_run_status_updated_on_completion()
  #[test] fn test_subagent_event_appended_per_iteration()
  #[test] async fn test_resume_reconstructs_history_from_events()
  #[test] async fn test_resume_continues_from_last_iteration()
  #[test] async fn test_abandon_marks_run_cancelled()
  #[test] fn test_cleanup_removes_terminal_runs_older_than_days()
  #[test] fn test_cleanup_preserves_running_status_regardless_of_age()
  #[test] async fn test_orphan_detection_warns_but_does_not_mutate()  // Archon principle
  → cargo test → FAIL

GREEN:
  1. Adicionar sqlx (workspace dep) — feature sqlite
  2. Criar persistence/{mod,store,migrations}.rs
  3. Migration runner (sqlx::migrate!)
  4. SubagentRunStore trait + SqliteSubagentRunStore impl
  5. AgentRunEngine emite eventos persistidos via store
  6. Theo CLI: novo subcomando run com clap
  → cargo test → PASS
```

**Decisao critical (Archon `CLAUDE.md` "No Autonomous Lifecycle Mutation"):** Quando o processo nao consegue distinguir "ativo em outro processo" de "orphan", NAO marca como failed automaticamente. Surface ambiguous state para user com one-click action. Heuristicas para retry/timeout em operacoes recuperaveis sao OK.

**Verify:** `cargo test -p theo-agent-runtime -- persistence && theo run list`

---

# TRACK D — SOTA Isolation & Observability (Fases 11-13)

> **Objetivo do track:** Tornar o sistema operacional em escala — isolamento concorrente seguro, telemetria padronizada, evolucao sem restart.
> **Pre-requisito:** Track A + B. Fase 11 depende de Fase 9 (checkpoint).
> **Evidencia direta:** Archon worktree provider, Pi-Mono parallel git rules, OpenTelemetry GenAI semantic conventions.

### Fase 11: Worktree Isolation per Sub-agent

**Objetivo:** Sub-agents paralelos podem rodar em git worktrees isolados. Default OFF (preserva UX simples), opt-in via spec ou CLI flag.

**Evidencia:**
- Archon `packages/isolation/src/providers/worktree.ts` — `WorktreeProvider`, port auto-allocation hash-based, `--no-worktree` opt-out
- Archon `CLAUDE.md` "Run in worktree" — workflows sao isolados por default no production
- Pi-Mono `AGENTS.md:194-233` — regras de seguranca para parallel agents (forbid `git reset/checkout/stash/add -A`, only commit YOUR files)

**Por que SOTA:** Cursor background agents, Aider, Archon todos usam worktrees. Sem isso, parallel writers em mesmo CWD causam conflitos imprevisiveis.

**Arquitetura:**

```
crates/theo-isolation/                 (NOVO crate)
  src/
    lib.rs
    worktree.rs        # WorktreeProvider (cria, lista, remove)
    resolver.rs        # IsolationResolver (request → environment)
    safety.rs          # Pi-Mono parallel-agent rules enforcement
    error.rs           # IsolationError (BlockedByUncommitted, etc.)
    store.rs           # IIsolationStore (SQLite persistence)
```

**Dependencia:** `theo-isolation → theo-domain` (puro). `theo-agent-runtime → theo-isolation` (atualizar ADR-016).

**Spec extension:**

```yaml
---
name: parallel-implementer
isolation:
  mode: worktree         # 'shared' | 'worktree'
  base_branch: main      # de qual branch criar
  cleanup: on_success    # 'always' | 'on_success' | 'never'
---
```

**Safety rules (Pi-Mono pattern):** Quando `isolation: worktree`, o sub-agent recebe regras INJETADAS no system prompt:

```text
PARALLEL-SAFETY RULES (active because you run in an isolated worktree):
- ONLY commit files you yourself created/modified in this worktree
- NEVER run: git reset, git checkout (other branches), git stash pop, git add -A
- Use safe rebase only — git rebase --abort if conflicts
- If you need files from another worktree, ASK the parent — do not pull/fetch
```

**Port auto-allocation (Archon pattern):** Para sub-agents que iniciam servidores HTTP de teste, port = `3190 + (sha1(worktree_path) % 900)`.

**TDD Sequence:**

```
RED:
  #[test] async fn test_worktree_provider_creates_isolated_branch()
  #[test] async fn test_worktree_provider_removes_on_success()
  #[test] async fn test_worktree_provider_preserves_on_failure_for_inspection()
  #[test] async fn test_worktree_port_allocation_deterministic_per_path()
  #[test] async fn test_worktree_blocks_uncommitted_changes_in_main()
  #[test] fn test_safety_rules_injected_into_system_prompt()
  #[test] async fn test_parallel_subagents_do_not_conflict_in_separate_worktrees()
  #[test] async fn test_isolation_disabled_uses_shared_cwd()
  #[test] async fn test_classify_isolation_error_returns_user_message()
  → cargo test → FAIL

GREEN:
  1. Criar crate theo-isolation
  2. Implementar WorktreeProvider via git2 ou subprocess git
  3. SubAgentManager.spawn(): if spec.isolation.is_some(), criar worktree, executar, cleanup
  4. Hooks WorktreeCreate/WorktreeRemove disparam (Fase 5 integration)
  5. Atualizar AgentResult com worktree_path: Option<PathBuf>
  → cargo test → PASS
```

**Decisao:** Default `isolation: shared` (compat). Worktree e opt-in. Justificativa: maioria dos use cases nao precisa, complexidade significativa.

**Verify:** `cargo test -p theo-isolation && cargo test -p theo-agent-runtime -- isolation_integration`

### Fase 12: OpenTelemetry Per-Agent Observability

**Objetivo:** Telemetria estruturada por agent, alinhada com OpenTelemetry GenAI semantic conventions. Resolve A4 (gap confirmado por grep zero em `observability/`).

**Evidencia:**
- OpenTelemetry GenAI semantic conventions (2025): `gen_ai.system`, `gen_ai.request.model`, `gen_ai.usage.input_tokens`, `gen_ai.agent.id`, `gen_ai.agent.name`, `gen_ai.operation.name`
- Archon Pino structured logs com pattern `{domain}.{action}_{state}`
- LangGraph Studio per-agent traces

**Span hierarchy:**

```
session (trace root)
  └─ agent.run (parent)
       ├─ agent.iteration[0]
       │    ├─ llm.call (gen_ai.operation.name=chat)
       │    └─ tool.call (tool.name=read)
       ├─ agent.iteration[1]
       │    └─ delegate_task (subagent.name=explorer)
       │         └─ agent.run (CHILD — nova subarvore)
       │              ├─ agent.iteration[0]
       │              │    └─ llm.call
       │              └─ ...
```

**Arquitetura:**

```rust
// theo-agent-runtime/src/observability/otel.rs (NOVO)

use opentelemetry::trace::{Tracer, SpanBuilder, SpanKind};

pub fn agent_run_span(spec: &AgentSpec, run_id: &str) -> SpanBuilder {
    Tracer::span_builder("agent.run")
        .with_kind(SpanKind::Internal)
        .with_attributes(vec![
            KeyValue::new("gen_ai.agent.id", run_id),
            KeyValue::new("gen_ai.agent.name", spec.name.clone()),
            KeyValue::new("theo.agent.source", spec.source.to_string()),
            KeyValue::new("theo.agent.builtin", matches!(spec.source, AgentSpecSource::Builtin)),
        ])
}

pub fn llm_call_span(model: &str, provider: &str) -> SpanBuilder {
    Tracer::span_builder("llm.call")
        .with_attributes(vec![
            KeyValue::new("gen_ai.system", provider),
            KeyValue::new("gen_ai.request.model", model),
            KeyValue::new("gen_ai.operation.name", "chat"),
        ])
}
```

**Logs estruturados (Archon `{domain}.{action}_{state}`):**

```rust
log::info!(
    target: "theo.subagent",
    agent_name = %spec.name,
    agent_source = %spec.source,
    objective = %objective,
    "subagent.spawn_started"
);
```

**TDD Sequence:**

```
RED:
  #[test] fn test_agent_run_span_has_otel_attributes()
  #[test] fn test_llm_call_span_has_gen_ai_attributes()
  #[test] fn test_subagent_spawn_creates_child_span_of_parent()
  #[test] fn test_metrics_breakdown_by_agent_name()
  #[test] fn test_metrics_breakdown_by_agent_source()
  #[test] fn test_log_event_naming_follows_domain_action_state()
  → cargo test → FAIL

GREEN:
  1. Adicionar opentelemetry, opentelemetry-otlp (workspace deps, feature flag)
  2. Refatorar observability/listener.rs para emitir spans
  3. MetricsCollector ganha .by_agent: BTreeMap<String, AgentMetrics>
  4. Endpoint OTLP configuravel via OTEL_EXPORTER_OTLP_ENDPOINT
  → cargo test → PASS
```

**Backward compat:** OpenTelemetry e feature flag (default OFF para compile time). Tracking interno (`MetricsCollector`) continua funcionando standalone.

**Verify:** `cargo test -p theo-agent-runtime --features otel -- observability::otel`

### Fase 13: Hot-reload de `.theo/agents/`

**Objetivo:** Agents adicionados/modificados em `.theo/agents/` sao reload automatico sem restart de sessao.

**Evidencia:** Claude Code recarrega `.claude/agents/`. OpenDev tem watcher em `~/.opendev/agents/`. Resolve gotcha do D6 (schema estatico por sessao).

**Arquitetura:**

```rust
// theo-agent-runtime/src/subagent/watcher.rs (NOVO)

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

pub struct RegistryWatcher {
    registry: Arc<RwLock<SubAgentRegistry>>,
    _watcher: RecommendedWatcher,  // dropped → para de observar
}

impl RegistryWatcher {
    pub fn watch(registry: Arc<RwLock<SubAgentRegistry>>, dirs: Vec<PathBuf>) -> Result<Self, NotifyError>;
    // On change: re-load_all(), aplicar S3 manifest check, atualizar registry
}
```

**Decisoes:**
- Re-aprovacao automatica via S3 manifest (G1) — modificacao de spec invalida hash → re-prompt na proxima delegacao
- Debounce 500ms (evita reload em meio a edicao)
- Hot-reload e opt-in (CLI flag `--watch-agents` ou config)

**TDD Sequence:**

```
RED:
  #[test] fn test_watcher_detects_new_agent_file()
  #[test] fn test_watcher_detects_modified_agent_file()
  #[test] fn test_watcher_detects_deleted_agent_file()
  #[test] fn test_watcher_debounce_coalesces_rapid_changes()
  #[test] fn test_modified_spec_triggers_re_approval_via_s3_manifest()
  #[test] fn test_watcher_ignores_non_md_files()
  → cargo test → FAIL

GREEN:
  1. Adicionar notify (workspace dep)
  2. Implementar RegistryWatcher
  3. CLI flag --watch-agents
  → cargo test → PASS
```

**Verify:** `cargo test -p theo-agent-runtime -- watcher`

---

## Invariantes Preservados

### Fundacao (Track A)
- **depth=1** — sub-agents NAO spawnam sub-agents (sem mudanca; A2A protocol fica em epico futuro)
- **return-only** — sub-agents retornam `AgentResult` ao parent (sem mudanca)
- **EventBus forwarding** — `PrefixedEventForwarder` tageia eventos por `spec.name` (sem mudanca)
- **CapabilityGate** — continua funcionando, agora alimentado por `spec.capability_set` (sem mudanca)
- **Seguranca S1** — on-demand agents sao read-only por default + cap por sessao (A5: max_on_demand_per_session)
- **Seguranca S2** — overrides de builtins usam intersecao de capabilities (nunca escalacao); G3 garante semantica nao-ambigua via `AllowedTools` enum
- **Seguranca S3** — project agents requerem aprovacao explicita do usuario (G1: SHA-256 manifest em `.theo/.agents-approved`)
- **Path-prefix-containment (G2)** — intersect de allowed_paths usa contencao hierarquica, nunca uniao
- **`is_subagent = true`** — continua bloqueando meta-tools de delegacao (sem mudanca)
- **Budget enforcement** — tokens do sub-agent contam para o parent (sem mudanca)
- **Dependency direction** — `AgentSpec` vive em `theo-domain` (zero deps), registry/parser vivem em `theo-agent-runtime`. Novos crates `theo-infra-mcp` (Fase 8) e `theo-isolation` (Fase 11) seguem ADR-016 (atualizar bound).
- **Sub-agents via grep/glob NAO passam por RRF** — decisao consciente, nao oversight
- **`.theo/agents/` excluido de retrieval e wiki index** — runtime config, nao knowledge
- **Streaming D1** — PrefixedEventForwarder ja forwards ContentDelta; SubagentStarted/Completed delimitam o fluxo
- **Prompting D2** — heuristicas de delegacao vivem no prompt, nao em codigo
- **Output protocol D3** — contrato minimo via prompt instruction (Fase 4) + JSON schema opcional best_effort (Fase 7). NUNCA enforcement obrigatorio em tipos Rust.
- **Observability D4** — custos per-agent emitidos via SubagentCompleted (Fase 3), agregacao via OTel spans (Fase 12)
- **Retry D5** — NAO ha retry automatico de sub-agents; o parent LLM decide baseado no summary de falha
- **Schema regeneration D6** — estatico por sessao em runtime padrao; hot-reload e opt-in (Fase 13)

### SOTA (Tracks B/C/D)
- **Hooks deterministicos** (Fase 5) — hooks tem response estatica, NUNCA invocam LLM. Previne loops infinitos.
- **Hooks fail-open** (Fase 5) — timeout de hook = response Allow. Justificativa: deadlock-prevention. Trade-off: hook quebrado nao bloqueia operacao.
- **Hooks per-agent overridem globais** (Fase 5) em conflito de mesmo evento — mais especifico vence.
- **Cancellation cooperativa** (Fase 6) — pontos de check explicitos no AgentLoop. NAO ha kill -9. Sub-agent termina limpamente.
- **Output schema best_effort por default** (Fase 7) — strict e opt-in. Free-text fallback preserva resiliencia.
- **MCP capability gating** (Fase 8) — MCP tools passam pelo CapabilityGate normal. Prefix `mcp:server:tool` evita colisao.
- **MCP stdio = trust local** (Fase 8) — apenas HTTP transport exige OAuth 2.1.
- **Checkpoint NAO e tool** (Fase 9) — LLM nao ve, nao chama. Infraestrutura transparente. Validacao de commit hash via regex.
- **Checkpoint excludes deterministicos** (Fase 9) — lista fixa, nao read do .gitignore (evita exfiltracao de segredos via spec maliciosa).
- **No Autonomous Lifecycle Mutation** (Fase 10, principio Archon) — quando o processo nao distingue "running elsewhere" de "orphan", NAO marca failed automaticamente. Surface ambiguity ao user.
- **Worktree default OFF** (Fase 11) — `isolation: shared` e o padrao. Worktree e opt-in. Justificativa: maioria nao precisa, complexidade alta.
- **Pi-Mono safety rules injetadas** (Fase 11) — quando worktree ativo, regras anti-`git reset/checkout/stash/add -A` vao no system prompt do sub-agent.
- **OpenTelemetry feature-gated** (Fase 12) — default OFF para compile time. MetricsCollector interno funciona standalone.
- **Hot-reload opt-in** (Fase 13) — `--watch-agents` flag. Modificacao de spec → re-aprovacao via S3 manifest (G1).

---

## Riscos e Mitigacoes

| Risco | Mitigacao |
|---|---|
| Custom agent com system prompt malicioso | S2: capabilities interseccionadas com builtin. S3: user confirmation na primeira carga. CapabilityGate enforcing |
| Escalacao acidental por convencao "vazio = todas" | G3: `AllowedTools::All \| Only(...)` enum elimina ambiguidade |
| Path traversal via intersect | G2: path-prefix-containment garante intersect <= self |
| Parser de frontmatter fragil | Testes extensivos (8 test cases). Frontmatter invalido → skip com warning (pattern OpenDev) |
| Breaking change na tool API | `delegate_task` substitui `subagent` + `subagent_parallel` atomicamente na Fase 4 |
| 530+ testes quebram na Fase 3 | RESOLVIDO v3.1: TODOS migrados para spec-based API. Legacy enum/spawn REMOVIDOS. ~2000 testes 100% passando. |
| On-demand agent burn tokens (per-agent) | S1: read_only + max_iterations=10 + timeout=120s. Cost guard efetivo |
| On-demand burn por volume (sessao inteira) | A5: AgentConfig.max_on_demand_per_session (default 20). Erro tipado quando excedido |
| Project agents de repo malicioso | S3/G1: SHA-256 manifest em `.theo/.agents-approved`. Modificacao de spec → re-aprovacao |
| YAML parser como nova dependencia | `serde_yaml` e madura, bem mantida. Impacto em compile time: ~5s |
| Skill system breakage | Fase 4 migra SkillMode::SubAgent para `agent_name: String` + registry lookup. SkillError::AgentNotFound como erro tipado |
| Frontend cego durante sub-agents | SubagentStarted/SubagentCompleted eventos emitidos na Fase 3 |
| Dashboard nao agrega por agent | A4 → Fase 12 (OTel spans com `gen_ai.agent.id`/`gen_ai.agent.name` + MetricsCollector.by_agent) |
| Schema desatualizado mid-session | D6: estatico por sessao. Hot-reload via Fase 13 (`--watch-agents`) |
| **Hook executa codigo malicioso** (B5) | Hooks tem response ESTATICA no YAML — nao executam codigo. S3 manifest aprovacao protege contra spec maliciosa adicionando hooks |
| **Cancellation deadlock** (B6) | Pontos de check cooperativos explicitos. Timeout backstop (max_iterations + timeout_secs) garante progresso |
| **MCP server malicioso** (C8) | OAuth 2.1 obrigatorio para HTTP. Stdio = trust local. CapabilityGate aplica `mcp:*` patterns. Spec do agent declara quais servers via `mcp_servers` (allowlist) |
| **MCP server crash/hang** (C8) | Subprocess kill on drop. Timeout per tool call. Fallback: tool indisponivel → erro tipado, parent LLM decide |
| **Checkpoint inflar disco** (C9) | Cleanup default 30 dias. Excludes node_modules/etc. Max 50k files (skip se exceder). CLI `theo checkpoints cleanup [--days N]` |
| **Checkpoint commit injection** (C9) | Regex `^[0-9a-fA-F]{4,64}$` para commit hash + reject `-` prefix (Hermes pattern) |
| **Resume com config divergente** (C10) | `config_snapshot` na tabela `subagent_runs` armazena AgentSpec serializada. Resume usa snapshot, nao registry atual |
| **Orphan detection mal-comportada** (C10) | Archon principle: NAO mutar autonomamente. Surface ambiguity. CLI `theo run abandon` requer acao explicita |
| **Worktree leak** (D11) | `cleanup: on_success` default. CLI `theo isolation cleanup [--merged]`. Worktree com uncommitted nao e removido (git natural guardrail) |
| **Worktree race em port allocation** (D11) | Hash deterministico por path. Mesmo path → mesma porta (idempotente) |
| **Pi-Mono safety violation** (D11) | Regras INJETADAS no prompt + hooks PreToolUse bloqueiam `git reset/checkout/stash/add -A` em modo worktree |
| **OTel overhead** (D12) | Feature-gated. OFF por default. Producao opt-in via `OTEL_EXPORTER_OTLP_ENDPOINT` |
| **Watcher loop em filesystem rapido** (D13) | Debounce 500ms. Apenas `*.md` em `.theo/agents/` e `~/.theo/agents/` |

---

## Verificacao Final

```bash
# Fase 1: domain types, builtins, registry
cargo test -p theo-domain -- allowed_tools          # G3
cargo test -p theo-domain -- agent_spec
cargo test -p theo-domain -- capability::tests::intersect  # S2/G2
cargo test -p theo-agent-runtime -- builtins
cargo test -p theo-agent-runtime -- registry

# Fase 2: frontmatter parser, custom loading, S3 approval
cargo test -p theo-agent-runtime -- frontmatter
cargo test -p theo-agent-runtime -- parser
cargo test -p theo-agent-runtime -- approval         # G1 (S3 manifest)
cargo test -p theo-agent-runtime -- skill

# Fase 3: SubAgentManager refactor
cargo test  # workspace inteiro — regressao

# Fase 4: delegate_task, cleanup, integration
cargo test
cargo clippy -- -D warnings

# === TRACK A FECHADO === MVP entregavel.

# Track B — SOTA Lifecycle
# Fase 5: Hooks
cargo test -p theo-agent-runtime -- hooks

# Fase 6: Cooperative cancellation
cargo test -p theo-agent-runtime -- cancellation

# Fase 7: Output format schema
cargo test -p theo-agent-runtime -- output_format

# === TRACK B FECHADO === Lifecycle SOTA.

# Track C — SOTA Integration
# Fase 8: MCP client
cargo test -p theo-infra-mcp
cargo test -p theo-agent-runtime -- mcp_integration

# Fase 9: Checkpoint manager
cargo test -p theo-agent-runtime -- checkpoint
theo checkpoints list  # smoke test

# Fase 10: Session persistence
cargo test -p theo-agent-runtime -- persistence
theo run list  # smoke test

# === TRACK C FECHADO === Integration SOTA.

# Track D — SOTA Isolation & Observability
# Fase 11: Worktree isolation
cargo test -p theo-isolation
cargo test -p theo-agent-runtime -- isolation_integration

# Fase 12: OpenTelemetry
cargo test -p theo-agent-runtime --features otel -- observability::otel

# Fase 13: Hot-reload
cargo test -p theo-agent-runtime -- watcher

# === TRACK D FECHADO === SOTA completo.

# Smoke test final: custom agent SOTA
mkdir -p .theo/agents
cat > .theo/agents/sota-agent.md << 'EOF'
---
name: security-reviewer
description: "Reviews code for OWASP Top 10 with structured output"
denied_tools: [edit, write, bash]
mcp_servers: [github]
isolation:
  mode: worktree
  base_branch: main
  cleanup: on_success
hooks:
  PreToolUse:
    - matcher: "^(edit|write|bash)$"
      response: { type: block, reason: "this agent is read-only" }
  UserPromptSubmit:
    - response: { type: inject_context, content: "Focus on OWASP Top 10." }
output_format:
  enforcement: best_effort
  schema:
    type: object
    required: [findings]
    properties:
      findings:
        type: array
        items:
          type: object
          required: [severity, file, message]
          properties:
            severity: { enum: [critical, high, medium, low] }
            file: { type: string }
            line: { type: integer }
            message: { type: string }
max_iterations: 25
timeout: 300
---
You are a security-focused code reviewer. Find vulnerabilities. Report findings.
EOF

# Aprovar agent (S3)
theo agents approve

# Smoke run com SOTA features
theo run --watch-agents --enable-checkpoints --features otel \
  "review the auth module for security issues using security-reviewer"

# Build completo
cargo build --all-features
cargo clippy --all-features -- -D warnings
```

---

## Epicos Futuros (fora deste plano)

| Epic | Quando | Pre-requisito | Justificativa |
|---|---|---|---|
| **MCP Server** (Theo como MCP server) | Quando IDE integrations (VS Code, Cursor) exigirem | Fase 8 (MCP Client) | Inversao da direcao: este plano consome MCP, server expoe Theo. Plano separado em `apps/theo-mcp-server/` ou flag `--mcp-server` no CLI. |
| **A2A Protocol / depth>1** | Quando workflows multi-agent complexos surgirem (raro hoje) | Track A + B | Google ADK e LangGraph implementam. Claude Code e Cursor explicitamente NAO. YAGNI confirmado para 2026. |
| **AgentFinding tipado** (structured Rust types) | Quando 3+ agents tiverem schemas similares (regra de 3 / DRY) | Fase 7 (Output schema best_effort) | Hoje usa `serde_json::Value`. Quando houver dados reais de uso, tipamos. |
| **File Locking advisory** | Quando parallel writers em mesmo CWD causarem conflitos observados | Fase 11 | Worktree isolation resolve a maioria. File locking so necessario se compartilhar CWD intencionalmente. |
| **Frecency memory para tools** (FFF.nvim pattern) | Cross-cutting feature de runtime, nao especifica de sub-agents | Track A | `referencias/fff.nvim/README.md:258-269` — file open history reduz tokens. Aplicavel ao parent tambem. |
| **Provider abstraction com lazy loading** (Pi-Mono pattern) | Refator de `theo-infra-llm` para reduzir compile time | Independente | `referencias/pi-mono/AGENTS.md:121-159` — register-builtins.ts evita imports estaticos. |
| **Profile-based isolation** (Hermes pattern) | Multi-tenant, multi-user em workstation compartilhada | Fase 10 (persistence) | `get_hermes_home()` getter — isolar instancias por `THEO_HOME`. Util em CI farms. |
| **Skin/theme engine** (Hermes pattern) | UX polish — customizacao de spinners, cores, labels | Independente | Pure data, baixa prioridade. |
| **Centralized command registry** (Hermes pattern) | Quando numero de slash commands > 20 | Independente | Hoje Theo CLI usa clap nativo, suficiente. |
| **Hybrid search (RRF + LLM rerank)** (QMD/llm-wiki-compiler) | Para context retrieval em sub-agents que precisam navegar codebase grande | Independente | Theo ja tem RRF em `theo-engine-retrieval/`. Falta reranker LLM. |
| **Variable substitution `$ARGUMENTS, $ARTIFACTS_DIR`** (Archon pattern) | Quando workflows com command nodes forem implementados | Track A | Hoje delegate_task aceita `objective` raw. Substitution e feature de workflow engine, nao de sub-agent. |

---

## Referencias

### Documentacao oficial

| # | Fonte | URL | Usado em |
|---|---|---|---|
| 1 | Claude Code Docs — Agents | https://docs.claude.com/en/docs/claude-code/agents | Track A, Fase 13 |
| 2 | Claude Code Docs — Hooks | https://docs.claude.com/en/docs/claude-code/hooks | Fase 5 (22 events) |
| 3 | arXiv 2604.14228 — Dive into Claude Code | https://arxiv.org/abs/2604.14228 | Dado chave 98.4% |
| 4 | OpenAI Codex Subagents | https://developers.openai.com/codex/subagents | Track A |
| 5 | OpenAI Agents SDK — Guardrails | https://openai.github.io/openai-agents-python/guardrails/ | Fase 7 (3-tier) |
| 6 | Anthropic — Multi-Agent Research System | https://www.anthropic.com/engineering/multi-agent-research-system | D5 (parent decides retry) |
| 7 | Anthropic MCP Spec | https://modelcontextprotocol.io/ | Fase 8 |
| 8 | OpenTelemetry GenAI Semantic Conventions | https://github.com/open-telemetry/semantic-conventions/tree/main/docs/gen-ai | Fase 12 |
| 9 | Aider — Architect Mode | https://aider.chat/2024/09/26/architect.html | Track A |
| 10 | Google ADK | https://google.github.io/adk-docs/ | Epico futuro (A2A) |
| 11 | LangGraph — Checkpointing | https://langchain-ai.github.io/langgraph/concepts/persistence/ | Fase 10 (resume pattern) |
| 12 | Cursor Background Agents | https://docs.cursor.com/background-agent | Fase 11 (worktree) |

### Projetos locais analisados (`referencias/`)

| Projeto | Tipo | Patterns extraidos | Usado em |
|---|---|---|---|
| **opendev** | Rust workspace 21 crates (clone direto do que Theo aspira ser) | `subagent-execution-model.md` (async tasks model), `crates/opendev-mcp/`, `crates/opendev-hooks/`, `crates/opendev-sandbox/`, `crates/opendev-models/src/config/agent.rs` (AgentConfigInline), `crates/opendev-plugins/`, `crates/opendev-docker/` | Track A (Fase 1-4), B5 (hooks arch), C8 (MCP), D11 (sandbox) |
| **Archon** | TypeScript+Bun, Remote Agentic Coding Platform | `packages/workflows/src/schemas/hooks.ts:10-88` (22 hook events Claude SDK), `packages/isolation/src/providers/worktree.ts` (WorktreeProvider), CLI resume/abandon/cleanup, `dag-node.ts` (per-node mcp/agents/hooks/skills/output_format/effort/thinking/maxBudgetUsd/fallbackModel/sandbox), `workflow_runs`+`workflow_events` schema, `IIsolationStore`, "No Autonomous Lifecycle Mutation" principle | B5 (hooks 22 events), C10 (sessions), D11 (worktree), D12 (logging conventions) |
| **hermes-agent** | Python, multi-platform agent | `tools/delegate_tool.py:32-50` (blocked tools, max_depth=2, _HEARTBEAT_INTERVAL), `tools/checkpoint_manager.py:1-90` (shadow git repos, sha256 path-hash, GIT_DIR+GIT_WORK_TREE), `tools/mcp_tool.py` (~1050 LOC), `tools/mcp_oauth_manager.py`, `tools/interrupt.py`, profile isolation via `get_hermes_home()`, skin engine | Track A (delegate schema), C8 (MCP+OAuth), C9 (checkpoint adotado diretamente), epico futuro (skin/profile) |
| **pi-mono** | TypeScript, ~20 LLM backends | Provider abstraction (`packages/ai/src/providers/`), lazy loading via register-builtins.ts, parallel-agent git safety rules (`AGENTS.md:194-233`: forbid `git reset/checkout/stash/add -A`, only commit YOUR files), intent-based tool restrictions | D11 (Pi-Mono safety rules), epico futuro (provider abstraction refator) |
| **opencode** | TypeScript agent harness | 3-agent system (build/plan/general) em AGENTS.md | Track A (modelo de roles minimo) |
| **rippletide** | TypeScript agent | Hook-first planning: UserPromptSubmit injeta coding rules antes do plan, regras EXPLICITAS no response | B5 (use case canonico para hooks UserPromptSubmit) |
| **llm-wiki-compiler / qmd** | TypeScript+Rust, knowledge retrieval | Hybrid search (BM25+vector+RRF+LLM rerank), MCP server stdio (default) + HTTP daemon mode, position-aware blend | C8 (MCP transports referencia), epico futuro (hybrid search) |
| **fff.nvim** | Neovim plugin Lua+Rust | Frecency memory (file open history + query-file combo boost), MCP integration | Epico futuro (frecency cross-cutting) |
| **awesome-harness-engineering** | Curated list de harness patterns | Indices de patterns: context delivery, tool design, MCP, planning, sandbox, memory, evals, agent loop | Validacao das escolhas (cross-check) |

### Reuniao de revisao

- `.claude/meetings/20260423-021723-review-agents-plan.md` — 16 agentes, veredito REVISED, 13 decisoes (v2.0)
- Esta v3.0 estende para SOTA com 9 fases novas baseadas em evidencia direta dos repos acima.
