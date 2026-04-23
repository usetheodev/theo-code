# Plano: SOTA Gaps Follow-up — fechar os 10 gaps reais identificados pós-v1

> **Versão 1.0** — continuação direta de `sota-gaps-plan.md`. Fecha os gaps reais
> que sobraram depois das Fases 14-19: integrações construídas mas não-ativas,
> observabilidade cega, validação somente sintética.

## Contexto

Pós-implementação do `sota-gaps-plan.md` v1 (Fases 14-19, 2,590+ testes verdes), uma auditoria honesta identificou 10 gaps:

| # | Gap | Impacto | Categoria |
|---|---|---|---|
| 1 | MCP discovery nunca auto-dispara | tools MCP nunca aparecem no schema do LLM | **Funcional** |
| 2 | PreHandoff hook sem YAML loader | guardrails só programáticos (compile-time) | **Funcional** |
| 3 | Resume re-executa side effects | double-write em mutations | **Funcional** |
| 4 | Sem telemetria `tier_chosen` | impossível calibrar ComplexityClassifier | **Observabilidade** |
| 5 | SSE só polla disco (2s lag) | dashboard não vê HandoffEvaluated em real-time | **Observabilidade** |
| 6 | Sem teste contra MCP server real | só wire-up sintético testado | **Validação** |
| 7 | OAuth Codex E2E não exercita delegate_task | onboarding intercepta antes da tool | **Validação** |
| 8 | Sem CLI `theo mcp discover` | operator não consegue warm cache manualmente | **Menor** |
| 9 | Cache MCP não invalida no reload | tools stale após upgrade do server | **Menor** |
| 10 | Resume não restaura worktree | path inexistente se já foi cleaned | **Menor** |

**Objetivo:** zero gaps reais. Cada fase ATÔMICA (1 PR), TDD obrigatório, plan-named tests.

**Estratégia (5 tracks):**

| Track | Fases | Entrega | Fecha gaps |
|---|---|---|---|
| **K — MCP Auto-Discovery** | 20-22 | auto-trigger + CLI + invalidação | #1, #6, #8, #9 |
| **L — Declarative Guardrails** | 23-24 | YAML loader + PreHandoff matcher | #2 |
| **M — Resume Resilience** | 25-26 | idempotency + worktree restore | #3, #10 |
| **N — Observability** | 27-28 | tier metric + EventBus→SSE bridge | #4, #5 |
| **O — Validation E2E** | 29 | OAuth Codex com delegate_task real | #7 |

Tracks K-N podem rodar em paralelo. Track O depende de K-M completos.

---

## Decisões de arquitetura

### D1: Auto-discovery é lazy (on first spawn) com TTL = sessão
Trigger no primeiro `spawn_with_spec` cujo `spec.mcp_servers` é não-vazio E o cache ainda não tem entry pra esse servidor. Evita custo no startup; concentra latência no primeiro uso. TTL = lifetime do processo (alinhado com plano original §17 D4).

### D2: Guardrails YAML em `.theo/handoff_guardrails.toml`
TOML por consistência com `.theo/config.toml` existente. Schema simples: lista de matchers com regex sobre target_agent/objective + decisão estática.

### D3: Resume idempotency via `tool_call_executed` event marker
Antes de re-executar uma tool durante resume, checar se há um `ToolCallCompleted` event correspondente no log. Se sim, replayar o resultado em vez de re-executar.

### D4: tier_chosen como métrica histogram em MetricsCollector
Acumula contagem `(task_type, tier_chosen, success)` para análise post-mortem. Não emite OTel ainda — apenas serializa em `RunReport`.

### D5: EventBus→SSE bridge via shared FileEventTail
Dashboard server abre `.theo/trajectories/<latest>.jsonl` em modo tail (inotify/kqueue), emite SSE quando aparece evento de tipo `HandoffEvaluated`/`SubagentStarted`/`SubagentCompleted`. Não muda o agent runtime.

### D6: Worktree restore = cria nova worktree no resume
Se `ctx.spec.isolation == "worktree"` e a antiga foi limpa, `Resumer` cria uma nova partindo do mesmo `base_branch`. Aceita drift como custo do resume.

---

## Fases

### Fase 20 — MCP Auto-Discovery on first spawn (Track K)

**Objetivo:** primeira chamada `spawn_with_spec` com `mcp_servers` não-vazio popula o cache automaticamente; chamadas subsequentes consomem o cache.

**Mudanças:**
- `SubAgentManager::spawn_with_spec` antes de registrar adapters: se `mcp_discovery.cached_servers()` não cobre `spec.mcp_servers`, chamar `discovery.discover_filtered(registry, &spec.mcp_servers, DEFAULT_PER_SERVER_TIMEOUT)`.
- Falhas de discovery são fail-soft (já testado): registram no relatório, sub-agent continua sem aqueles tools.

**TDD:**
```
RED:
  spawn_with_spec_auto_triggers_discovery_when_cache_empty
  spawn_with_spec_skips_discovery_when_cache_already_populated
  spawn_with_spec_continues_when_discovery_fails_completely
  spawn_with_spec_does_not_discover_when_mcp_servers_empty
  spawn_with_spec_does_not_discover_when_no_registry_attached
GREEN:
  Adicionar bloco `if !covered { discover_filtered(...).await; }` em spawn_with_spec
INTEGRATION:
  Test que monta cache vazio + spawn explorer com mcp_servers=["fake"] e
  verifica que discovery foi tentada (failed report + cache ainda vazio).
```

**Verify:** `cargo test -p theo-agent-runtime -- subagent::tests::spawn_with_spec_auto`

---

### Fase 21 — `theo mcp discover` CLI + watcher invalidation (Track K)

**Objetivo:** operator pode warm/refresh cache manualmente; reloadable watcher invalida entries dos servidores afetados.

**Mudanças:**
- `apps/theo-cli/src/main.rs`: novo subcomando `Mcp { Discover { server: Option<String> }, Invalidate { server: String }, ClearAll, List }`.
- `apps/theo-cli/src/mcp_admin.rs` (novo): handlers que constroem registry + cache compartilhados.
- `subagent::reloadable::ReloadableRegistry` ganha hook `on_spec_changed` que invalida `cache.invalidate(server)` para cada server no spec antigo + novo.

**TDD:**
```
RED:
  cmd_mcp_discover_unknown_server_returns_err
  cmd_mcp_discover_known_server_populates_cache
  cmd_mcp_invalidate_drops_entry
  cmd_mcp_clear_all_empties_cache
  cmd_mcp_list_shows_cached_servers_and_tool_counts
  reloadable_invalidates_cache_when_spec_mcp_servers_changes
GREEN:
  - Implementar handlers em mcp_admin.rs
  - Adicionar callback registration em ReloadableRegistry
INTEGRATION:
  E2E: `theo mcp discover` → cache populado → `theo mcp list` mostra
```

**Verify:** `cargo test -p theo --bin theo mcp_admin`

---

### Fase 22 — Real MCP server integration test (Track K)

**Objetivo:** prova que o protocolo MCP funciona end-to-end (handshake, tools/list, tools/call), não só wire-up.

**Mudanças:**
- `crates/theo-infra-mcp/tests/real_server.rs` (novo, gated by `MCP_REAL_TEST=1` env).
- Spawn `npx @modelcontextprotocol/server-filesystem /tmp/mcp-test-dir` como child process.
- Verificar `tools/list` retorna `read_file`, `list_directory`.
- Chamar `tools/call read_file` e verificar conteúdo.
- Skip if `npx` não disponível (graceful).

**TDD:**
```
RED (gated):
  real_mcp_filesystem_server_lists_expected_tools
  real_mcp_filesystem_server_calls_read_file
  real_mcp_filesystem_server_handles_invalid_args
GREEN:
  Implementar test harness com tokio::process::Command
SKIP_IF: npx ausente OR MCP_REAL_TEST não set
```

**Verify:** `MCP_REAL_TEST=1 cargo test -p theo-infra-mcp --test real_server`

---

### Fase 23 — Project-level guardrails YAML/TOML loader (Track L)

**Objetivo:** `.theo/handoff_guardrails.toml` definindo guardrails declarativamente; carregados na CLI e injetados no `GuardrailChain` antes dos custom programáticos.

**Schema:**
```toml
[[guardrail]]
id = "no-implementer-touches-prod"
matcher.target_agent = "implementer"
matcher.objective_pattern = "production|prod"  # regex
decision.kind = "block"
decision.reason = "production changes require human review"

[[guardrail]]
id = "verifier-cannot-mutate"
matcher.target_agent = "verifier"
matcher.objective_pattern = "implement|write|edit"
decision.kind = "redirect"
decision.new_agent_name = "implementer"
```

**Mudanças:**
- `crates/theo-agent-runtime/src/handoff_guardrail/declarative.rs` (novo): structs serde + `DeclarativeGuardrail` impl `HandoffGuardrail`.
- `crates/theo-application/src/use_cases/guardrail_loader.rs` (novo): `load_project_guardrails(project_dir) -> GuardrailChain`.
- CLI `build_injections` chama o loader.

**TDD:**
```
RED:
  declarative_guardrail_block_decision_serializes_correctly
  declarative_guardrail_redirect_decision_serializes_correctly
  declarative_guardrail_matches_target_agent_exact
  declarative_guardrail_matches_objective_via_regex
  declarative_guardrail_skips_when_matcher_misses
  load_project_guardrails_empty_when_file_absent
  load_project_guardrails_parses_2_entries
  load_project_guardrails_returns_err_for_malformed_toml
GREEN:
  - Criar declarative.rs com struct + impl
  - Criar guardrail_loader.rs use case
  - Wire em build_injections
INTEGRATION:
  E2E: criar .theo/handoff_guardrails.toml → spawn agent → verificar block
```

**Verify:** `cargo test -p theo-agent-runtime -- handoff_guardrail::declarative && cargo test -p theo-application -- guardrail_loader`

---

### Fase 24 — PreHandoff lifecycle hook YAML matcher (Track L)

**Objetivo:** habilitar matcher YAML para `HookEvent::PreHandoff` no `lifecycle_hooks::HookManager` (mesmo formato dos outros hooks).

**Mudanças:**
- `lifecycle_hooks::HookContext` ganha campos `target_agent`, `target_objective` para PreHandoff matching.
- `evaluate_handoff` em `run_engine.rs` popula esses campos quando dispatcha PreHandoff.
- Doc/exemplo em `.theo/hooks.toml`.

**TDD:**
```
RED:
  hook_context_carries_pre_handoff_fields
  pre_handoff_matcher_blocks_by_target_agent_regex
  pre_handoff_matcher_blocks_by_objective_regex
  pre_handoff_matcher_allows_when_no_match
GREEN:
  - Estender HookContext
  - Atualizar evaluate_handoff dispatch
```

**Verify:** `cargo test -p theo-agent-runtime -- lifecycle_hooks::pre_handoff`

---

### Fase 25 — Resume idempotency via tool_call replay (Track M)

**Objetivo:** durante resume, tool calls que já completaram NÃO são re-executadas; o resultado persistido é replayado para o LLM.

**Mudanças:**
- `Resumer::build_context` reconstrói não só history mas também `executed_tool_calls: HashSet<call_id>`.
- `AgentLoop` ganha modo "replay" onde, antes de executar uma tool, checa se `call_id` está em executed_tool_calls e usa o resultado do log.
- `SubagentEvent` recebe variant `tool_call_completed { call_id, result }` se já não tem (verificar).

**TDD:**
```
RED:
  resume_skips_tool_call_with_existing_completed_event
  resume_executes_tool_call_when_no_completed_event_exists
  resume_replay_preserves_call_id_in_history
  reconstruct_executed_tool_calls_returns_set_of_call_ids
GREEN:
  - Estender ResumeContext com executed_tool_calls
  - Modificar AgentLoop dispatch para checar set
INTEGRATION:
  Smoke: spawn agent que chama bash, kill mid-run, resume, verificar
  bash NÃO é re-executado (count via mock).
```

**Verify:** `cargo test -p theo-agent-runtime -- subagent::resume::idempotency`

---

### Fase 26 — Resume worktree restore (Track M)

**Objetivo:** se `ctx.spec.isolation == "worktree"` e a path antiga não existe, criar nova worktree partindo do mesmo `base_branch`.

**Mudanças:**
- `Resumer::resume_with_objective` antes de spawn checa `theo_isolation::WorktreeProvider::create` se necessário.
- ResumeContext expõe novo campo `worktree_strategy: Reuse(PathBuf) | Recreate { base_branch: String } | None`.

**TDD:**
```
RED:
  resume_worktree_strategy_none_when_spec_not_isolated
  resume_worktree_strategy_recreate_when_path_missing
  resume_worktree_strategy_reuse_when_path_exists
  resume_with_recreate_strategy_creates_new_worktree
GREEN:
  - Adicionar enum WorktreeStrategy
  - Wire em build_context + resume_with_objective
```

**Verify:** `cargo test -p theo-agent-runtime -- subagent::resume::worktree`

---

### Fase 27 — `tier_chosen` telemetry (Track N)

**Objetivo:** cada decisão do `AutomaticModelRouter` emite contagem (task_type, tier_chosen, model_id) no `MetricsCollector`. Disponível em `RunReport` e exposto via `/api/system/stats`.

**Mudanças:**
- `MetricsCollector::record_routing_decision(task_type, tier, model_id, success)`.
- `AutomaticModelRouter` precisa de handle pro collector — opcional, attached via `with_metrics`.
- Nova seção `routing_decisions: Vec<RoutingDecisionMetric>` em `RunReport`.

**TDD:**
```
RED:
  metrics_collector_records_routing_decision
  metrics_collector_aggregates_decisions_by_tier
  router_with_metrics_handle_records_each_decision
  router_without_metrics_handle_silently_skips_recording
  routing_decisions_appear_in_run_report
GREEN:
  - Estender MetricsCollector
  - AutomaticModelRouter::with_metrics builder
  - Serialização em RunReport
```

**Verify:** `cargo test -p theo-agent-runtime -- observability::metrics::routing && cargo test -p theo-infra-llm -- routing::auto::with_metrics`

---

### Fase 28 — EventBus → SSE bridge para `HandoffEvaluated`/Subagent* (Track N)

**Objetivo:** dashboard SSE emite eventos de tipos `HandoffEvaluated`, `SubagentStarted`, `SubagentCompleted` em real-time (não 2s polling).

**Estratégia:** dashboard tail-eia `.theo/trajectories/<latest>.jsonl` via `notify`/`tokio::fs` (file-watch). Quando linha JSON chega, parse, filtra por event_type, emite SSE.

**Mudanças:**
- `apps/theo-cli/src/dashboard_agents.rs::agents_events_handler`: trocar `interval(2s)` por `tokio::fs::File` + `BufReader::lines()` em loop, com poll secundário fallback se inotify não disponível.
- Manter retro-compat: emite ainda `subagent_run_added` ao detectar novos arquivos em `runs/`.

**TDD:**
```
RED:
  sse_handler_emits_handoff_evaluated_within_500ms_of_event_append
  sse_handler_emits_subagent_started_from_trajectory
  sse_handler_emits_subagent_completed_from_trajectory
  sse_handler_filters_out_non_subagent_events (e.g. ToolCallCompleted)
  sse_handler_keeps_alive_when_no_events
GREEN:
  - Implementar tail loop com tokio::fs::File + BufReader
  - Filtrar event_types desejados
INTEGRATION:
  Smoke: test que escreve linha em fixtures/.theo/trajectories/*.jsonl
  e confirma SSE entrega em <1s.
```

**Verify:** `cargo test -p theo --bin theo dashboard_agents::tests::sse_handler`

---

### Fase 29 — OAuth Codex E2E que exercita delegate_task (Track O)

**Objetivo:** smoke test real (OAuth Codex) onde o agent SEM onboarding intercept invoca `delegate_task` e o caminho completo (LLM → guardrail → discovery → spawn → MCP) executa.

**Mudanças:**
- Novo system_prompt mode "headless-direct": pula onboarding, executa instruções literalmente. Atualizar `theo agent --headless` para usar esse modo por default (já é o caminho de benchmark/CI).
- Test script `scripts/sota12-oauth-smoke.sh` que:
  1. Verifica token OAuth válido
  2. Cria fixture `.theo/agents/sota12-validator.md`
  3. Aprova via `theo agents approve --all`
  4. Roda `theo agent --headless 'use delegate_task ... '`
  5. Asserta:
     - `theo subagent list` mostra ≥ 1 run
     - `theo subagent status <id>` mostra `agent_name=sota12-validator`
     - `.theo/trajectories/*.jsonl` contém `HandoffEvaluated` event
     - Dashboard `/api/agents/sota12-validator` retorna stats

**TDD:** ferramentas externas → script-based, asserts via `jq` + exit-code.

**Verify:** `bash scripts/sota12-oauth-smoke.sh` (manual; CI gated por OAuth disponível)

---

## Riscos e mitigações

| Risco | Mitigação |
|---|---|
| Auto-discovery na primeira spawn introduz latência ~5s | Feature flag `THEO_MCP_AUTO_DISCOVERY=1` (default ON, mas operator pode desligar) |
| YAML guardrails malformados quebram CLI startup | Loader retorna `Result`; CLI continua sem chain custom + warning |
| Resume idempotency não cobre tool calls com side-effect externo (HTTP POST) | Documentar limitação; idempotency só aplica a tools puras / read-only |
| Worktree recreate diverge do estado original | ResumeContext expõe `WorktreeStrategy` ao caller; CLI prompt antes de criar |
| Telemetry de routing infla event log | Routing metrics agregadas in-memory; só serializa snapshot em RunReport (1x por run) |
| File-tail SSE falha em filesystems sem inotify | Fallback automático para poll a cada 1s |
| OAuth Codex E2E flaky por dependência de modelo externo | Script gated por env `OAUTH_E2E=1`; CI roda 1x por dia |

---

## Verificação final agregada

```bash
# Track K
cargo test -p theo-agent-runtime -- subagent::tests::spawn_with_spec_auto
cargo test -p theo --bin theo mcp_admin
MCP_REAL_TEST=1 cargo test -p theo-infra-mcp --test real_server  # opcional

# Track L
cargo test -p theo-agent-runtime -- handoff_guardrail::declarative
cargo test -p theo-application -- guardrail_loader
cargo test -p theo-agent-runtime -- lifecycle_hooks::pre_handoff

# Track M
cargo test -p theo-agent-runtime -- subagent::resume::idempotency
cargo test -p theo-agent-runtime -- subagent::resume::worktree

# Track N
cargo test -p theo-agent-runtime -- observability::metrics::routing
cargo test -p theo-infra-llm -- routing::auto::with_metrics
cargo test -p theo --bin theo dashboard_agents::tests::sse_handler

# Track O (manual / OAuth gated)
OAUTH_E2E=1 bash scripts/sota12-oauth-smoke.sh
```

---

## Cronograma (ordem de execução)

Trabalho atômico — cada fase fecha em um único commit + PR.

```
Sprint 1 (Tracks K + L paralelos):
  Fase 20 → Fase 21 → Fase 22  (MCP)
  Fase 23 → Fase 24            (Guardrails YAML)

Sprint 2 (Tracks M + N paralelos):
  Fase 25 → Fase 26            (Resume)
  Fase 27 → Fase 28            (Observabilidade)

Sprint 3:
  Fase 29                      (OAuth E2E final)
```

Total: 10 fases, ~1 PR cada. Estimate conservador: 2 sprints (~2 semanas).

---

## Compromisso de cobertura final

Após este plano: **0 gaps reais**. Sistema SOTA não só no design (já era), mas no fluxo ATIVO end-to-end.

| Gap | Status pós-plano |
|---|---|
| #1 MCP auto-discovery | ✓ Fase 20 |
| #2 PreHandoff YAML | ✓ Fases 23-24 |
| #3 Resume idempotency | ✓ Fase 25 |
| #4 tier_chosen telemetria | ✓ Fase 27 |
| #5 SSE EventBus push | ✓ Fase 28 |
| #6 MCP server real test | ✓ Fase 22 |
| #7 OAuth Codex E2E | ✓ Fase 29 + follow-up: validado real com 4 chamadas `delegate_task_single` via Codex, 4 HandoffEvaluated, 4 sub-agent runs persistidos, dashboard responde. Fix exigiu split do schema unificado em `delegate_task_single`/`delegate_task_parallel` (Codex confundia o one-of original) + `THEO_FORCE_TOOL_CHOICE=function:NAME` + headless mode passou a usar `build_injections`. |
| #8 `theo mcp discover` CLI | ✓ Fase 21 |
| #9 Cache invalidação no reload | ✓ Fase 21 |
| #10 Resume worktree restore | ✓ Fase 26 |

---

## Referências (links já no plano original)

- `docs/plans/sota-gaps-plan.md` — plano v1 (entregue)
- `docs/plans/agents-plan.md` v3.1 — fundação (13 fases entregues)
- ADR-016 — dependency direction (apps → application → infra/runtime)
- TDD: RED → GREEN → REFACTOR (sem exceções)
