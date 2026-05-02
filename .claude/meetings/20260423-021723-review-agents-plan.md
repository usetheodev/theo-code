---
id: 20260423-021723
date: 2026-04-23
topic: "Revisao critica do plano Dynamic Sub-Agent System (docs/plans/agents-plan.md)"
verdict: REVISED
participants: 16
---

# Reuniao: Revisao Critica do Plano Dynamic Sub-Agent System

## Pauta

**Contexto:** O plano `docs/plans/agents-plan.md` propoe substituir o sistema atual de 4 sub-agents hardcoded (`SubAgentRole` enum em 463 linhas) por um sistema dinamico com 7 fases: AgentSpec + Registry, Markdown parser, Refactor SubAgentManager + File Locking, delegate_task tool, Worktree Isolation, MCP Integration, Cleanup.

**Questoes a decidir:**
1. O escopo esta adequado ou inchado?
2. Quais fases sao YAGNI?
3. AgentSpec pertence a theo-domain ou theo-agent-runtime?
4. O modelo de seguranca para custom/on-demand agents e suficiente?
5. A ordem de prioridade esta correta?
6. Riscos arquiteturais nao cobertos?

**Estado atual:** Branch `develop`, commit `1197307`. SubAgentRole com 4 roles, SubAgentManager com spawn/spawn_parallel, 530+ testes no workspace.

---

## Posicoes por Agente

### Estrategia

| Agente | Posicao | Resumo |
|--------|---------|--------|
| chief-architect | CONCERN | Escopo inchado. Fases 5-7 sao YAGNI. AgentFinding e especulativo. AgentSpec deveria ir para theo-agent-runtime, nao theo-domain. Propoe 4 fases minimas. |
| evolution-agent | CONCERN | Fases 1-4 sao competitivamente necessarias. Fases 5-7 prematuras. 5 gaps nao cobertos: migracao de skills, reuso de parser, testes de integracao LLM, cost caps, security model para project agents. |

### Conhecimento

| Agente | Posicao | Resumo |
|--------|---------|--------|
| knowledge-compiler | CONCERN | Novos conceitos precisam de wiki pages. `.theo/agents/*.md` devem ser indexados como ProjectConfig (baixa autoridade). Plano nao menciona integracao com knowledge base. |
| ontology-manager | CONCERN | Identidade dual AgentSpec.name vs SubAgentRoleId precisa de bridge method `role_id()`. FindingSeverity e novo tipo valido. `delegate_task` e o nome correto. |
| data-ingestor | CONCERN | `.theo/agents/*.md` sao runtime config, NAO knowledge artifacts. Nao ingerir. Ingerir apenas o design doc como referencia. |
| wiki-expert | CONCERN | `.theo/agents/` DEVE ser excluido do BM25 index. System prompts poluiriam rankings de busca. Wiki deve gerar pagina sintetica do registry, nao indexar specs raw. |

### Qualidade

| Agente | Posicao | Resumo |
|--------|---------|--------|
| validator | CONCERN | 2 issues CRITICOS: (1) on-demand agents com CapabilitySet::unrestricted() por default, (2) project agents podem escalar capabilities de builtins via override. Schema delegate_task ambiguo. |
| linter | CONCERN | Fases 1-4 justificadas. Fases 5-7 criam risco de codigo orfao se nunca implementadas. Parser precisa de error handling explicito. Deprecation deadline necessaria. |
| retrieval-engineer | CONCERN | `.theo/` deve ser excluido do retrieval index (como `.git/`). System prompts consomem tokens sem cap. Adicionar `max_prompt_tokens` ao AgentSpec. Sub-agents usando grep/glob bypasam RRF por design — documentar. |
| memory-synthesizer | CONCERN | AgentFinding cria dados estruturados valiosos mas sem feedback path para knowledge system. (context, findings) formam pares de treino naturais que evaporam no fim da sessao. Sugere FindingsSink. |

### Engineering

| Agente | Posicao | Resumo |
|--------|---------|--------|
| code-reviewer | CONCERN | CRITICO: `context: Option<&str>` e regressao vs `Option<Vec<Message>>`. `std::sync::Mutex` em async e hazard. `HashMap` nao-deterministico para build_tool_description(). AgentSpec sem derives. On-demand sem cap de seguranca. |
| graphctx-expert | CONCERN | Plano corretamente ortogonal ao GRAPHCTX por arquitetura. Mas: parallel agents podem causar fan-out de retrieval sem budget, custom agents nao recebem GRAPHCTX context enrichment, sem teste de regressao de retrieval. |
| arch-validator | APPROVE c/ correcoes | MCP em theo-agent-runtime VIOLA boundary — deve ir para theo-infra-mcp. FileLockManager precisa de PathValidator trait. AgentSpec em theo-domain e valido. |
| test-runner | CONCERN | TDD plan inadequado — sem sequencias RED-GREEN-REFACTOR. 530+ testes em risco no Phase 3 (mudanca de assinatura). Precisa de backward compat layer + migracao bulk. Parser precisa de 15+ test cases. |
| frontend-dev | CONCERN | Gap: nenhum evento Tauri para lifecycle de sub-agents. UI fica cega durante delegate_task. Precisa de subagent_start/subagent_end events com agent_name e agent_source. |

### Pesquisa

| Agente | Posicao | Resumo |
|--------|---------|--------|
| research-agent | APPROVE c/ 3 concerns | Evidencias bem usadas. MCP server deve ser deferido (30 CVEs em 60 dias no ecosistema). On-demand precisa de cap de capabilities. Faltam Google ADK e OpenAI Agents SDK na tabela de referencias. |

---

## Conflitos

### Conflito 1: AgentSpec — theo-domain vs theo-agent-runtime

- **chief-architect** defende mover para `theo-agent-runtime` (conceito runtime, nao domain)
- **arch-validator** aprova em `theo-domain` (pure value type, zero deps)
- **ontology-manager** aceita em `theo-domain` se bridge method `role_id()` for adicionado

**Resolucao:** AgentSpec fica em `theo-domain`. Justificativa: e um value type puro (strings, enums, options) sem deps de runtime. `SubAgentRoleId` ja vive em `theo-domain/routing.rs`. Mover para runtime criaria import circular quando routing precisar do spec. Bridge method `AgentSpec::role_id() -> SubAgentRoleId` e obrigatorio.

**POREM:** `AgentFinding` e `FindingSeverity` sao DEFERIDOS (YAGNI — "measure before schema"). `IsolationMode` e DEFERIDO (so faz sentido com Phase 5).

### Conflito 2: Escopo — 7 fases vs 4 fases

- **chief-architect** propoe 4 fases
- **evolution-agent** propoe 4 fases + stop + measure
- **linter** concorda com 4 fases
- **research-agent** sugere split de MCP (client ok, server defer)

**Resolucao:** CONSENSO UNANIME. Cortar para 4 fases. Fases 5-7 viram epicos separados.

### Conflito 3: AgentFinding — construir agora ou medir primeiro?

- **memory-synthesizer** quer AgentFinding agora (dados valiosos para sintese)
- **chief-architect** + **validator** + **code-reviewer** defendem defer (especulativo, parser fragil)

**Resolucao:** DEFER. Memoria "measure before schema" prevalece. AgentResult ganha apenas `agent_name` e `context_used`. AgentFinding sera desenhado quando houver dados reais de output de sub-agents para basear o schema.

---

## Decisoes

### D1: Escopo reduzido a 4 fases (UNANIME)

| Fase | Conteudo | Status |
|------|----------|--------|
| **Fase 1** | AgentSpec em theo-domain (sem AgentFinding, sem IsolationMode), builtins.rs, SubAgentRegistry | APROVADO |
| **Fase 2** | Markdown parser (reaproveitar/unificar com skill parser), load_custom/load_global, resolution order | APROVADO |
| **Fase 3** | Refactor SubAgentManager (spawn recebe &AgentSpec), AgentResult +agent_name +context_used, backward compat layer | APROVADO |
| **Fase 4** | delegate_task tool, on-demand mode, cleanup SubAgentRole, integration test | APROVADO |
| ~~Fase 5~~ | ~~Worktree Isolation~~ | CORTADO — epic separado |
| ~~Fase 6~~ | ~~MCP Integration~~ | CORTADO — epic separado em theo-infra-mcp |
| ~~Fase 7~~ | ~~Cleanup final~~ | ABSORVIDO pela Fase 4 |

### D2: Seguranca — on-demand agents (CRITICO)

`AgentSpec::on_demand()` DEVE usar `CapabilitySet::read_only()` como default. Agentes com capabilities de escrita exigem spec registrado (builtin, global, ou project). O LLM NAO pode escalar capabilities via on-demand.

### D3: Seguranca — override de builtins (CRITICO)

Project agents com mesmo nome de builtin: capability set e INTERSECCIONADO (nunca escalado). Um `.theo/agents/explorer.md` pode RESTRINGIR o Explorer, nunca ampliar. Log de warning obrigatorio quando builtin e overridden.

### D4: context mantém Vec<Message> (CRITICO)

`spawn()` mantem `context: Option<Vec<Message>>`. Nova helper `spawn_with_text_context(&str)` para o path do delegate_task (constroi `vec![Message::user(text)]`). Preserva structured history internamente.

### D5: IndexMap em vez de HashMap para registry

`SubAgentRegistry` usa `IndexMap<String, AgentSpec>` para preservar insertion order e garantir determinismo em `build_tool_description()`.

### D6: .theo/agents/ excluido de retrieval e wiki index

Adicionar `.theo/` a exclusion list do `FsSourceProvider` e wiki indexer. Tratar como `.git/` — runtime config, nao knowledge.

### D7: Eventos frontend para sub-agents

Adicionar `subagent_start` e `subagent_end` ao `AgentEventType` com `agent_name` e `agent_source`. Pode ser implementado na Fase 3 junto com o refactor do SubAgentManager.

### D8: Parser de frontmatter — reaproveitar

Unificar com parser existente em `skill/mod.rs` (linhas 130-170) ou extrair modulo compartilhado `frontmatter_parser`. NAO duplicar.

### D9: Mutex — usar std::sync::Mutex corretamente

Se FileLockManager for implementado no futuro: `acquire()` e sincrono, `MutexGuard` dropped antes de retornar `FileLockGuard`. `FileLockGuard::drop()` re-adquire brevemente para remover paths. Documentar explicitamente no plano.

---

## Issues Criticos Identificados (Pre-Requisitos para Implementacao)

| # | Severidade | Issue | Decisao |
|---|-----------|-------|---------|
| 1 | CRITICO | On-demand agents com CapabilitySet::unrestricted() | D2: Default read_only() |
| 2 | CRITICO | Project override pode escalar capabilities de builtins | D3: Intersecao, nunca escalacao |
| 3 | CRITICO | context: Option<&str> perde structured history | D4: Manter Vec<Message> |
| 4 | ALTO | HashMap nao-deterministico | D5: IndexMap |
| 5 | ALTO | MCP no crate errado | Cortado do escopo (epic separado) |
| 6 | ALTO | TDD plan sem RED-GREEN sequences | Action item abaixo |
| 7 | ALTO | 530+ testes quebram na Fase 3 | Action item: backward compat layer |
| 8 | MEDIO | Frontend cego durante sub-agents | D7: Eventos subagent_start/end |
| 9 | MEDIO | .theo/ indexado por retrieval/wiki | D6: Excluir |
| 10 | MEDIO | Parser duplicado (skill vs agents) | D8: Unificar |
| 11 | MEDIO | AgentSpec sem derives no plano | Adicionar Debug, Clone, Serialize, Deserialize |
| 12 | MEDIO | delegate_task schema ambiguo (agent + parallel) | oneOf ou erro explicito |
| 13 | BAIXO | Skill system migration (SkillMode::SubAgent) | Enderecado na Fase 3/4 |

---

## Plano TDD

### Fase 1: AgentSpec + Builtins + Registry

**RED:**
```rust
#[test] fn test_agent_spec_new_has_correct_name()
#[test] fn test_agent_spec_role_id_returns_subagent_role_id()
#[test] fn test_agent_spec_source_variants()
#[test] fn test_builtin_explorer_has_read_only_capabilities()
#[test] fn test_builtin_implementer_has_write_capabilities()
#[test] fn test_registry_with_builtins_has_4_agents()
#[test] fn test_registry_get_returns_none_for_missing()
#[test] fn test_registry_register_adds_agent()
#[test] fn test_registry_names_returns_sorted_list()
```
**GREEN:** Implementar AgentSpec, builtins, SubAgentRegistry minimais.
**REFACTOR:** Extrair constantes, garantir derives corretos.
**VERIFY:** `cargo test -p theo-domain -- agent_spec && cargo test -p theo-agent-runtime -- registry`

### Fase 2: Markdown Parser + Custom Loading

**RED:**
```rust
#[test] fn test_parse_valid_frontmatter_extracts_all_fields()
#[test] fn test_parse_missing_name_uses_filename()
#[test] fn test_parse_invalid_yaml_returns_error()
#[test] fn test_parse_missing_closing_delimiter_returns_error()
#[test] fn test_parse_empty_body_allowed()
#[test] fn test_parse_denied_tools_populates_capability_set()
#[test] fn test_parse_unknown_fields_ignored()
#[test] fn test_load_custom_from_project_dir()
#[test] fn test_load_global_from_home_dir()
#[test] fn test_resolution_order_project_overrides_global()
#[test] fn test_resolution_order_intersects_builtin_capabilities()
#[test] fn test_override_builtin_logs_warning()
```
**GREEN:** Implementar parser (reutilizando skill frontmatter), loaders.
**REFACTOR:** Extrair modulo compartilhado de frontmatter parsing.
**VERIFY:** `cargo test -p theo-agent-runtime -- parser`

### Fase 3: Refactor SubAgentManager

**RED:**
```rust
#[test] fn test_spawn_with_agent_spec_works()
#[test] fn test_spawn_with_text_context_helper()
#[test] fn test_agent_result_has_agent_name()
#[test] fn test_backward_compat_role_to_spec_conversion()
#[test] fn test_existing_530_tests_still_compile() // bulk migration
#[test] fn test_subagent_start_event_emitted()
#[test] fn test_subagent_end_event_emitted()
```
**GREEN:** Refatorar SubAgentManager, adicionar backward compat, emitir eventos.
**REFACTOR:** Remover SubAgentRole shim apos migracao completa (Fase 4).
**VERIFY:** `cargo test` (workspace inteiro — regressao)

### Fase 4: delegate_task + Cleanup

**RED:**
```rust
#[test] fn test_delegate_task_dispatches_named_agent()
#[test] fn test_delegate_task_dispatches_custom_agent()
#[test] fn test_delegate_task_on_demand_uses_read_only_capabilities()
#[test] fn test_delegate_task_on_demand_max_iterations_capped_at_10()
#[test] fn test_delegate_task_parallel_spawns_concurrent()
#[test] fn test_delegate_task_rejects_agent_and_parallel_simultaneously()
#[test] fn test_delegate_task_schema_lists_available_agents()
#[test] fn test_subagent_role_enum_removed() // compile-time: old code fails
```
**GREEN:** Implementar delegate_task, remover SubAgentRole, atualizar system prompts.
**REFACTOR:** Cleanup de imports orfaos, atualizar CHANGELOG.
**VERIFY:** `cargo test && cargo clippy -- -D warnings`

---

## Action Items

- [ ] **Plano** — Reescrever `agents-plan.md` com escopo reduzido (4 fases), decisoes de seguranca (D2, D3), e TDD sequences acima
- [ ] **Plano** — Adicionar secao de decisoes de seguranca: on-demand read_only, override intersecao, user confirmation para project agents
- [ ] **Plano** — Especificar crate de YAML parsing (`serde_yaml` ou `serde_yml`) e adicionar a `[workspace.dependencies]`
- [ ] **Plano** — Documentar que sub-agents via grep/glob NAO passam por RRF (decisao consciente)
- [ ] **Plano** — Adicionar delegate_task schema com oneOf (single vs parallel), nao ambos simultaneos
- [ ] **Retrieval** — Adicionar `.theo/` a exclusion list antes da Fase 1
- [ ] **Frontend** — Planejar eventos `subagent_start`/`subagent_end` para Fase 3
- [ ] **Ontology** — Adicionar `AgentSpec::role_id() -> SubAgentRoleId` bridge ao design da Fase 1

---

## Veredito Final

**REVISED**: O plano e aprovado com modificacoes significativas. A pesquisa SOTA e excelente e a arquitetura core (AgentSpec + Registry + Parser + delegate_task) e competitivamente necessaria e bem fundamentada. Porem, o escopo original de 7 fases viola YAGNI — fases 5-7 (file locking, worktree isolation, MCP) sao prematuras e devem ser epicos separados. Dois issues criticos de seguranca (on-demand unrestricted, project override escalation) devem ser resolvidos antes da implementacao. O plano deve ser reescrito incorporando as 13 decisoes desta reuniao.
