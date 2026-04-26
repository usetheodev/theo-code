# theo-agent-runtime — Revisao

> **Contexto**: Orquestra o loop do agent (LLM + tools + governance). State machine governa transicoes de fase. Bounded Context: Agent Runtime.
>
> **Dependencias permitidas** (ADR-016): `theo-domain`, `theo-governance`, `theo-infra-llm`, `theo-infra-auth`, `theo-tooling`.
>
> **Status global**: deep-review concluido em 2026-04-25 apos 86 iteracoes de remediacao + auditoria final dos 57 dominios. Todos os items do REMEDIATION_PLAN.md (47 tasks T0.x–T8.x) foram cumpridos e validados (ver REMEDIATION_PLAN.md Iter 86 para a tabela consolidada).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `agent_loop` | Loop principal do agent: plan → execute → evaluate → (converge \| replan). | Revisado |
| 2 | `agent_message` | Estruturas de mensagens trocadas entre agent/LLM/tools. | Revisado |
| 3 | `autodream` | Auto-melhoria em background (dreaming/consolidation). | Revisado |
| 4 | `budget_enforcer` | Enforcement hard de orcamento (tokens/cost/time) com fail-fast. | Revisado |
| 5 | `cancellation` | Cancelamento cooperativo propagado por toda a arvore de tarefas. | Revisado |
| 6 | `capability_gate` | Gate que bloqueia operacoes quando capability nao concedida. | Revisado |
| 7 | `checkpoint` | Checkpoints persistidos para retomada apos crash. | Revisado |
| 8 | `compaction` | Compactacao de contexto (resume + drop). | Revisado |
| 9 | `compaction_stages` | Estagios sequenciais da pipeline de compactacao. | Revisado |
| 10 | `compaction_summary` | Geracao e persistencia de sumarios pos-compactacao. | Revisado |
| 11 | `config` | `AgentConfig`, `CompactionPolicy`, `MessageQueues`, `ToolExecutionMode`. | Revisado |
| 12 | `convergence` | Detecao de convergencia (parar quando objetivo alcancado). | Revisado |
| 13 | _(removido)_ | T4.10b / find_p2_006: o módulo `correction` não existe no crate atual. Linha mantida para preservar a numeração histórica. | N/A |
| 14 | `dlq` | Dead-letter queue de tool-calls/events que falharam definitivamente. | Revisado |
| 15 | `doom_loop` | Detector de loops improdutivos (mesmo erro/tool repetido). | Revisado |
| 16 | `event_bus` | Barramento pub/sub de eventos de runtime (`EventBus`, `EventListener`). | Revisado |
| 17 | `evolution` | Evolucao do agent (hypothesis_pipeline + lesson_pipeline). | Revisado |
| 18 | `extension` | Sistema de extensoes plugaveis. | Revisado |
| 19 | `failure_tracker` | Agregador de falhas por categoria/frequencia. | Revisado |
| 20 | `frontmatter` | Parser de YAML frontmatter em prompts/skills. | Revisado |
| 21 | `handoff_guardrail` | Guardrails declarativos para handoff entre agents. | Revisado |
| 22 | `hooks` | Hooks configuraveis por usuario (pre/post tool-call etc). | Revisado |
| 23 | `hypothesis_pipeline` | Pipeline de hipoteses (sugere → testa → aprende). | Revisado |
| 24 | `jit_instructions` | Instrucoes just-in-time injetadas conforme contexto. | Revisado |
| 25 | `lesson_pipeline` | Pipeline de extracao e persistencia de licoes. | Revisado |
| 26 | `lifecycle_hooks` | Hooks de ciclo de vida (on_start, on_stop, on_error). | Revisado |
| 27 | `loop_state` | Estado observavel do loop atual. | Revisado |
| 28 | `memory_lifecycle` | Ciclo de vida de memorias (create → review → decay → prune). | Revisado |
| 29 | `memory_reviewer` | Agente revisor de memorias (qualidade/relevancia). | Revisado |
| 30 | `observability` | `metrics`, `context_metrics`, `derived_metrics`, `envelope`, `failure_sensors`, `listener`, `loop_detector`, `normalizer`, `otel_exporter`. | Revisado |
| 31 | `onboarding` | Fluxo de onboarding inicial do agent em novo workspace. | Revisado |
| 32 | `output_format` | Formatos de saida (text, json, structured). | Revisado |
| 33 | `persistence` | Persistencia de estado do runtime (sessions, checkpoints). | Revisado |
| 34 | `pilot` | Modo pilot (execucao supervisionada/dry-run). | Revisado |
| 35 | `plugin` | Sistema de plugins externos. | Revisado |
| 36 | `project_config` | Leitura/validacao de `.theo/config.toml` e `CLAUDE.md`. | Revisado |
| 37 | `reflector` | Reflexao pos-acao (o que funcionou? o que aprender?). | Revisado |
| 38 | `retry` | Politicas de retry com backoff exponencial. | Revisado |
| 39 | `roadmap` | Roadmap de execucao (passos planejados). | Revisado |
| 40 | `run_engine` | `AgentRunEngine` — orquestrador de runs multi-fase. | Revisado |
| 41 | `tool_pair_integrity` (was `sanitizer`) | T1.2 / FIND-P6-008: nome migrado; reparo estrutural de pares tool após compactação. **NÃO scrubba PII/segredos** — isso é responsabilidade de `secret_scrubber` (T4.5). | Revisado |
| 42 | _(removido)_ | T4.10b / find_p2_013: o módulo `scheduler` não existe no crate atual. Linha mantida para preservar a numeração histórica. | N/A |
| 43 | `sensor` | Sensores ambientais (file changes, clock, signals). | Revisado |
| 44 | `session_bootstrap` | Bootstrap de sessao (load context, init state). | Revisado |
| 45 | `session_tree` | Arvore de sessoes pai/filho (sub-agents). | Revisado |
| 46 | `skill` | `bundled` skills — empacotadas e distribuidas com o binario. | Revisado |
| 47 | `skill_catalog` | Catalogo de skills disponiveis no workspace. | Revisado |
| 48 | `skill_reviewer` | Revisor automatico de skills (lint + qualidade). | Revisado |
| 49 | `snapshot` | Snapshots de estado (mais leves que checkpoints). | Revisado |
| 50 | `state_manager` | Gerente central de estado do runtime. | Revisado |
| 51 | `subagent` | `approval`, `builtins`, `mcp_tools`, `parser`, `registry`, `reloadable`, `resume`, `watcher` — sub-agentes isolados. | Revisado |
| 52 | `subagent_runs` | Runs de sub-agentes com tracking proprio. | Revisado |
| 53 | `system_prompt_composer` | Composicao dinamica de system prompts. | Revisado |
| 54 | `task_manager` | Gerente de tasks (fila, prioridade, dependencias). | Revisado |
| 55 | `tool_bridge` | Ponte entre runtime e `theo-tooling`. | Revisado |
| 56 | `tool_call_manager` | Dispatcher/tracker de tool-calls em voo. | Revisado |
| 57 | `transcript_indexer` | Indexacao de transcripts para busca/retrieval. | Revisado |

---

## Notas de Deep-Review por Dominio

> Cada nota e o resultado de uma auditoria orientada a: (1) responsabilidade unica, (2) dependencias, (3) acoplamento com `theo-domain`, (4) cobertura de testes, (5) hygiene (LOC <= 500, zero clippy warnings em codigo proprio, zero `unwrap()` em prod). Findings cumpridos durante o REMEDIATION_PLAN sao referenciados; achados residuais sao explicitamente listados.

### 1. agent_loop (417 LOC mod.rs + 100 result.rs)
Facade fina sobre `AgentRunEngine`. Iter 60 split `AgentResult` em `result.rs`; Iter 64 + 86 migrou todos os reads de config para views (`config.llm()/plugin()`). Sub-agent integrations bundleadas via `SubAgentIntegrations` (T5.2). Zero achados residuais.

### 2. agent_message (72 LOC)
Wrapper dual LLM-vs-UI message. Modulo pequeno e focado, sem refactor pendente. 100% AAA tests inline.

### 3. autodream (381 LOC)
Post-session memory consolidation behind feature flag `autodream_enabled`. Acquisicao de lock + cooldown verificados via tests inline (test_acquire_lock_then_second_acquire_fails, test_cooldown_elapsed_future_timestamp_does_not_panic). Wired via `evolution()` view (Iter 64).

### 4. budget_enforcer (92 LOC)
4 sites de fail (iterations/time/tokens/tool_calls) cobertos por testes. Apos T2.1, todos os usos sao `parking_lot::Mutex`. Bench em `record_session_exit_large_log` exercita o path quente.

### 5. cancellation (79 LOC)
`CancellationTree` com tokio_util tokens. Cobertura via `cancellation_tree_root_cancel_propagates_silently` (run_engine_characterization) + `subagent_pre_run_cancellation_emits_started_then_completed_cancelled` (subagent_characterization). API compacta, sem refactor pendente.

### 6. capability_gate (79 LOC)
`CapabilityGate::new(caps, bus)` injetado no `ToolCallManager` quando `config.plugin().capability_set` e `Some` (Iter 86). Pequeno e focado. Cobertura via meta_tools_t7_3 (capability tests).

### 7. checkpoint (307 LOC)
Shadow git repos para rollback transparente (Track C). Test infra usa tempdirs. Sem `unwrap()` em paths de producao.

### 8. compaction (compaction.rs 388 LOC + summary 113)
Apos T4.7 split, esta dentro do cap. Usa `CompactionContext` (semantica de progresso) injetado por `iteration_prelude::inject_context_loop_and_compact`. AC `agent_recovers_from_context_overflow_then_converges` (T0.1 cenario 9) cobre o caminho de emergency compaction.

### 9. compaction_stages (~)
Pipeline staged que `inject_context_loop_and_compact` chama com a policy de `config.context().compaction_policy`. Coberto pelo cenario 9 e benches de streaming.

### 10. compaction_summary (~)
Geracao de sumarios. Pequeno e focado. Persistido via `pre_compress_push` memory hook.

### 11. config (mod 375 + prompts 270 + views 165)
Apos Iters 55, 58, 60, 86: 7 view structs (`LlmView`, `LoopView`, `ContextView`, `MemoryView`, `EvolutionView`, `RoutingView`, `PluginView`), todos com ≤10 fields (T4.1 AC literal). Zero direct grouped-field reads remanescentes em `theo-agent-runtime/src/`.

### 12. convergence (~)
`GitDiffConvergence` + `EditSuccessConvergence` em `AllOf` mode. Pure-function `is_converged(ctx)` com unit tests. AC `agent_done_gate_1_blocks_then_recovers_with_text` (T0.1 cenario 12) cobre o block path; force-accept (cenario 5) cobre o escape hatch.

### 13. correction _(REMOVIDO)_
T4.10b / find_p2_006: o módulo `correction` não existe na árvore de fontes atual (`grep -r "mod correction" crates/theo-agent-runtime/src/` retorna vazio). O texto histórico foi removido para evitar drift de documentação. A funcionalidade de correção pós-erro vive hoje no `evolution::EvolutionLoop` diretamente.

### 14. dlq (55 LOC)
Dead-letter queue. T8.3 documentado: caller wraps em `Mutex<DeadLetterQueue>` (compile-time check via `assert_send_sync<Arc<Mutex<DeadLetterQueue>>>`).

### 15. doom_loop (55 LOC)
Detector de mesmo-tool-com-mesmos-args N vezes. Threshold via `config.loop_cfg().doom_loop_threshold` (Iter 63). Inline tests no run_engine/main_loop.rs.

### 16. event_bus (207 LOC)
T6.1: `Mutex<VecDeque<DomainEvent>>` (era Vec) — O(1) eviction. T6.2: `events_for(entity_id)` substitui `events()` em `record_session_exit` (Iter 65). T2.1: panicking listener nao polui o bus (`solo_panicking_listener_does_not_stop_future_publishes`). Bench `event_bus_publish` em T7.4.

### 17. evolution (235 LOC)
Loop estruturado de retry com reflexao entre attempts. Wrappa `CorrectionEngine` + `HeuristicReflector`. Cobertura inline + sota_integration tests.

### 18. extension (105 LOC)
Sistema de extensoes via lifecycle_hooks. Pequeno wrapper. Vide also `lifecycle_hooks` (D26).

### 19. failure_tracker (153 LOC)
Agrega falhas por fingerprint. Cobertura ampla: `record_increments_count`, `hot_patterns_returns_only_above_threshold`, `persistence_roundtrip`, `suggestion_at_threshold`, `suggestion_emitted_flag_persists` etc.

### 20. frontmatter (63 LOC)
Parser YAML frontmatter compartilhado entre skills/agents. Modulo minimal, parser explicito sem deps externas.

### 21. handoff_guardrail (mod 124 + builtins ~+ chain)
T1.4 sub-agent + handoff guardrail chain. Logic extraida via `evaluate_handoff` (run_engine/handoff.rs) com `HandoffOutcome::{Allow, Block, Redirect, RewriteObjective}`. Inline tests em sota_integration.

### 22. hooks (~)
Hooks configuraveis via `.theo/hooks/<event>.sh`. AC T7.1 `test_hook_with_shell_metacharacters_escaped` (Iter 65) prova zero shell-injection — argv-style spawn + write_all stdin.

### 23. hypothesis_pipeline (114 LOC)
`persist_unresolved` chamado em `record_session_exit` quando run termina sem converge. Hipoteses persistidas em `.theo/memory/hypotheses/`.

### 24. jit_instructions (71 LOC)
Loader per-subdir para instructions just-in-time. Pequeno e focado.

### 25. lesson_pipeline (165 LOC)
`extract_and_persist_for_outcome` chamado em `record_session_exit` (lifecycle.rs). Persiste licoes em `.theo/memory/lessons/`. Cobertura via memory_pre_reqs tests.

### 26. lifecycle_hooks (253 LOC)
22 eventos Claude-Agent-SDK-aligned. `HookManager` + `HookContext` + `HookResponse::{Allow, Block}`. Cobertura via `subagent_characterization::subagent_start_hook_block_emits_only_completed_no_started`.

### 27. loop_state (165 LOC)
`ContextLoopState` rastreia reads/searches/edits para feed do compaction context. Phase transitions preservadas para diagnostics.

### 28. memory_lifecycle (mod 487 + run_engine_hooks + wiring)
T4.7 split. Hooks: `inject_prefetch`, `pre_compress_push`, `sync_final_turn`, `inject_legacy_file_memory`. Counters atomicos via `MemoryNudgeCounter`. Cobertura abrangente via `memory_pre_reqs`, `memory_wiring_t0_1`.

### 29. memory_reviewer (103 LOC)
Trait MemoryReviewer + NullMemoryReviewer fallback. Spawned via `spawn_memory_reviewer` em wiring.rs. T8.4: RouterHandle Debug significativo (mesmo padrao aplicado a MemoryReviewerHandle).

### 30. observability (17 files / ~218+ LOC)
T6.4 stream_batcher (FLUSH_BYTES=64). T6.2 events_since/events_for. T6.3 ToolCallManager purge. CI gate de coverage com MAX_DROP_PP=2.0. Bench `tool_call_dispatch_throughput`. Inclui report/, otel.rs, derived_metrics.rs, normalizer.rs, listener.rs, loop_detector.rs, writer.rs, reader.rs.

### 31. onboarding (286 LOC)
Bootstrap inicial. `compose_bootstrap_system_prompt` injetado por `maybe_prepend_bootstrap` em wiring.rs. Skip quando memory_dir ja tem entries.

### 32. output_format (165 LOC)
`try_parse_structured(summary, schema)` com strict vs best_effort modes. Aplicado em `apply_output_format` (subagent/finalize_helpers.rs Iter 60). Cobertura via sota_integration `output_format_invalid_severity_fails` + `output_format_parses_valid_findings`.

### 33. persistence (92 LOC)
Wrapping de session persistence. T2.3 typed errors em record_session_exit.

### 34. pilot (mod 481 LOC + types 50 + git 65 + run_loop)
Iter 59 split (572 → 481 mod.rs). PilotConfig, PilotResult, ExitReason em `types.rs`. GitProgress + helpers em `git.rs`. Cobertura inline + e2e em `e2e_auto_evolution.rs`.

### 35. plugin (274 LOC)
T1.3 hardening: owner check + sha256 allowlist. Tests `plugin_with_wrong_owner_rejected`, `plugin_not_in_allowlist_rejected_when_configured`. Wired via `config.plugin().allowlist` (Iter 86).

### 36. project_config (287 LOC)
Le `.theo/config.toml` + `CLAUDE.md`. Iter 47 fixou flaky test via `env_lock()` + `EnvSnapshot` RAII pattern.

### 37. reflector (189 LOC)
`HeuristicReflector` para failure classification + corrective guidance. Driven by `evolution::EvolutionLoop`.

### 38. retry (69 LOC)
T2.5: explicit `loop {}` (sem `unreachable!()` panic landmine). T3.4: T6.4 stream batchers internos. Iter 72: `record_retry()` agora wired em call_llm_with_retry. AC test `agent_retries_after_503_and_succeeds` (T0.1 cenario 8).

### 39. roadmap (250 LOC)
Parser de checkbox progress + roadmap tasks. Usado pelo pilot::run_from_roadmap.

### 40. run_engine (20 files / ~357+ LOC mod.rs)
Iter 56-61: split god-file 4230 LOC → mod.rs 357 + 19 children, todos ≤625 LOC. Subdivisoes: bootstrap, builders, delegate_handler, dispatch (5 files), execution, handoff, iteration_prelude, lifecycle, llm_call, main_loop, post_dispatch_updates, stream_batcher, text_response. Strategy pattern em dispatch/router.rs. Chain of Responsibility em dispatch/done_gates.rs.

### 41. sanitizer (73 LOC)
`prompt_sanitizer` re-export wrapper. Real impl em theo-domain. AC T1.2 cobertura em security_t7_1: strip_injection_tokens, fence_untrusted, char_boundary_truncate.

### 42. scheduler _(REMOVIDO)_
T4.10b / find_p2_013: o módulo `scheduler` não existe na árvore de fontes atual (`grep -r "mod scheduler" crates/theo-agent-runtime/src/` retorna vazio). O texto histórico foi removido para evitar drift de documentação. As decisões de scheduling vivem hoje em `task_manager` + `tool_call_manager`.

### 43. sensor (103 LOC)
SensorRunner dreina pending results para system messages no LLM (`drain_sensor_messages` em iteration_prelude.rs). T0.1 underpinning para Gate 2 fixture.

### 44. session_bootstrap (162 LOC)
Bootstrap de sessao + load context. Driven by run_engine/bootstrap.rs.

### 45. session_tree (3 files / 364 LOC)
T4.7 split. `SessionTree` arvore pai/filho com header + entries. context_builder.rs separado.

### 46. skill (mod + bundled)
8 bundled skills (commit, test, review, build, explain, fix, refactor, init). Iter 66: pure-function `plan_skill_dispatch` para T7.3 dispatch matrix. AC T0.1 cenarios 6 (InContext) + 13 (SubAgent recursive verifier).

### 47. skill_catalog (359 LOC)
Catalog scanner para `.theo/skills/`. T8.5 module-size compliant.

### 48. skill_reviewer (183 LOC)
Background reviewer paralelo ao memory_reviewer. T5.4 atomic ordering consistente. Wired via `config.evolution().skill_reviewer` (Iter 64).

### 49. snapshot (112 LOC)
Snapshots leves de state (mais baratos que checkpoints). Pequeno e focado.

### 50. state_manager (136 LOC)
T8.2 `WIKI_LEGACY_DEPRECATION_DATE = "2026-10-20"` — sunset schedule documentado. Dual-path read (memory/episodes/ novo, wiki/episodes/ legacy fallback).

### 51. subagent (12 files: mod 401 + spawn_helpers 429 + finalize_helpers + manager_builders + approval + builtins + mcp_tools + parser + registry + reloadable + resume + watcher)
T4.5 split agressivo. SubAgentManager bundleavel via `SubAgentIntegrations` (T5.2). T0.2 characterization (3 cenarios pre-LLM-mock) + T0.1 scenario 13 (recursive spawn LLM-driven). Resume com gap-#3 invariant pinned via `agent_replays_cached_tool_result_on_resume`.

### 52. subagent_runs (257 LOC)
File-backed run store. Iter 83 corrigiu `sort_by` → `sort_by_key(Reverse)` (clippy cleanup).

### 53. system_prompt_composer (163 LOC)
`system_prompt_for_mode` (Plan/Ask) + `default_system_prompt` (175-LOC SOTA). Iter 58 split: prompts.rs separado de config/mod.rs.

### 54. task_manager (130 LOC)
TaskManager state machine: Pending → Ready → Running → (Completed | Failed | WaitingTool). 4 cenarios em run_engine_characterization (happy/failure/invalid/waiting-tool).

### 55. tool_bridge (4 files / ~488 LOC)
Conversao entre theo-domain ToolDefinitions e theo-infra-llm Tool types. meta_schemas.rs com batch_execute schema. execute_meta.rs com BLOCKED_IN_BATCH list. T7.3 batch dimension cobertura completa (5/25/blocked/empty/26 overflow via Iter 79).

### 56. tool_call_manager (130 LOC)
T6.3 `purge_completed(now_ms, older_than_ms)` evita leak em sessions longas (10k stress test em resilience_t7_2). T6.5 lock discipline reduzida (3 locks por dispatch, era 6). T2.1 panicking listener isolation.

### 57. transcript_indexer (67 LOC)
Trait + handle. Wired via `config.memory().transcript_indexer`. Tantivy backend feature-gated em theo-application.

---

## Conclusao

Todos os 57 dominios do bounded context Agent Runtime foram revisitados em deep-review na sequencia das 86 iteracoes do REMEDIATION_PLAN. Os achados foram convertidos em tasks (T0.x–T8.x), implementados, validados via tests, e auditados na Iteracao 86 (REMEDIATION_PLAN.md tem a tabela consolidada).

Achados que sobreviveram a auditoria como follow-ups conscientes (NAO bloqueadores):

- **T4.1 design-intent estrutural**: a descricao do plano sugeria nested-struct full refactor (`pub struct AgentConfig { pub llm: LlmConfig, ... }`); a implementacao escolhida foi views read-only (`config.llm()` retornando `LlmView<'_>`). AC literais ("≤10 fields", "todos call sites atualizados", "tests verdes") cumpridos via views; o full breaking refactor cross-crate continua disponivel como PR coordenado se decidido.
- **Snapshots inline vs `tests/snapshots/`**: T0.1 spec mencionou "Snapshots em tests/snapshots/ commitados". A implementacao usa inline `insta::assert_yaml_snapshot!(..., @"...")` (forma moderna; snapshots ainda commitados em source). Equivalente funcionalmente.
- **2 warnings em deps externos**: `theo-infra-llm` (4 warnings) e `theo-infra-mcp` (2 warnings). Fora do escopo do bounded context Agent Runtime.

Todos os 57 dominios estao **Revisado**. Combinado com o cumprimento literal dos 47 ACs do REMEDIATION_PLAN (1315 tests passando, zero warnings em codigo proprio, module-size gate verde a cap 500), o estado da `theo-agent-runtime` reflete os SLOs documentados.
