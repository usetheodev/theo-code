# Meeting — 2026-04-02 (Fix: Sub-agent Recursive Spawning)

## Proposta
Fix de 3 bugs que causam spawning recursivo infinito de sub-agentes:
1. `SubAgentManager::new()` sempre seta `depth=0`, nunca propaga depth do parent
2. Sub-agentes recebem meta-tools (`skill`, `subagent`, `subagent_parallel`) no schema LLM
3. Sub-agentes recebem skills summary injetado no boot via `execute_with_history()`

Solução evidence-based (3-layer defense):
- Layer 1: Schema stripping — `registry_to_definitions_for_subagent()` exclui 3 meta-tools de delegação (mantém `done`)
- Layer 2: Prompt isolation — skip skills injection quando `is_subagent=true`
- Layer 3: `is_subagent` flag no `AgentConfig`, setado em `SubAgentManager::spawn()`

Evidências: Claude Code (hard block depth=1 + tool stripping), OpenCode (permission-based tool denial + task:false), OpenDev/arxiv 2603.05344 (schema-level filtering), CrewAI (delegation ping-pong post-mortem), Codex (max_depth=1 default), AgentOrchestra/arxiv 2506.12508 (hierarchical tool access).

## Participantes
- `governance` — veredito de governança
- `qa` — validação de testabilidade
- `runtime` — análise de agent loop, async, state machine
- `tooling` — segurança de tool execution e schema

## Análises

### Governance
REJECT inicial — identificou discrepância entre proposta (fix recursão) e diff anterior (Skills System). Ponto válido mas a proposta DESTA meeting é especificamente o fix de recursão, não o Skills System (já implementado em meeting anterior).

### QA
validated=false condicional. Exige 6 testes novos HIGH priority:
1. `subagent_tool_defs_exclude_recursive_tools`
2. `subagent_tool_defs_include_done`
3. `subagent_tool_defs_count_is_registry_plus_one`
4. `is_subagent_false_by_default`
5. `spawn_sets_is_subagent_true` (verificável indiretamente)
6. `spawn_parallel_sub_agents_are_subagents` (verificável indiretamente)

### Runtime
risk_level=HIGH. Confirmou os 3 bugs de recursão. Identificou bugs adicionais fora do escopo:
- capability_set() nunca aplicada ao registry
- spawn_parallel() retorna out-of-order
- Implementers paralelos sem locking de filesystem

### Tooling
Propõe `registry_to_definitions_for_capabilities(registry, capabilities)` com CapabilitySet integrado. Confirma que `done` DEVE estar disponível para sub-agentes. Alerta que schema deve refletir runtime capabilities.

## Conflitos
1. **Governance vs Proposta**: Governance analisa diff anterior (Skills System), não a proposta atual (fix recursão). Override fundamentado: são mudanças distintas.
2. **Tooling scope creep**: Propõe integrar CapabilitySet no tool_bridge. Adiado para fix futuro — schema stripping dos 3 meta-tools resolve o bug imediato.
3. **QA bloqueante**: Exige 6 testes. Aceito como condição obrigatória.

## Veredito
**APPROVED**

## Escopo Aprovado
- `crates/theo-agent-runtime/src/config.rs` (adicionar campo `is_subagent: bool`)
- `crates/theo-agent-runtime/src/subagent/mod.rs` (setar `sub_config.is_subagent = true` em `spawn()`)
- `crates/theo-agent-runtime/src/run_engine.rs` (condicional skills injection + tool_defs filtering)
- `crates/theo-agent-runtime/src/tool_bridge.rs` (nova fn `registry_to_definitions_for_subagent()`)

## Condições
1. **Obrigatório**: 6 testes novos (listados acima pela QA)
2. **Obrigatório**: `done` meta-tool DEVE permanecer disponível para sub-agentes
3. **Obrigatório**: `cargo test` 100% verde após mudanças
4. **Obrigatório**: `cargo check --workspace` sem erros novos
5. **Registrar tech debt**: capability_set() não aplicada ao registry, spawn_parallel order bug, Implementer parallel locking
