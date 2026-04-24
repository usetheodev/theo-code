# theo-agent-runtime — Revisao

> **Contexto**: Orquestra o loop do agent (LLM + tools + governance). State machine governa transicoes de fase. Bounded Context: Agent Runtime.
>
> **Dependencias permitidas** (ADR-016): `theo-domain`, `theo-governance`, `theo-infra-llm`, `theo-infra-auth`, `theo-tooling`.

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `agent_loop` | Loop principal do agent: plan → execute → evaluate → (converge \| replan). | Pendente |
| 2 | `agent_message` | Estruturas de mensagens trocadas entre agent/LLM/tools. | Pendente |
| 3 | `autodream` | Auto-melhoria em background (dreaming/consolidation). | Pendente |
| 4 | `budget_enforcer` | Enforcement hard de orcamento (tokens/cost/time) com fail-fast. | Pendente |
| 5 | `cancellation` | Cancelamento cooperativo propagado por toda a arvore de tarefas. | Pendente |
| 6 | `capability_gate` | Gate que bloqueia operacoes quando capability nao concedida. | Pendente |
| 7 | `checkpoint` | Checkpoints persistidos para retomada apos crash. | Pendente |
| 8 | `compaction` | Compactacao de contexto (resume + drop). | Pendente |
| 9 | `compaction_stages` | Estagios sequenciais da pipeline de compactacao. | Pendente |
| 10 | `compaction_summary` | Geracao e persistencia de sumarios pos-compactacao. | Pendente |
| 11 | `config` | `AgentConfig`, `CompactionPolicy`, `MessageQueues`, `ToolExecutionMode`. | Pendente |
| 12 | `convergence` | Detecao de convergencia (parar quando objetivo alcancado). | Pendente |
| 13 | `correction` | Correcao automatica apos erro detectado (`#[doc(hidden)]`, dead code). | Pendente |
| 14 | `dlq` | Dead-letter queue de tool-calls/events que falharam definitivamente. | Pendente |
| 15 | `doom_loop` | Detector de loops improdutivos (mesmo erro/tool repetido). | Pendente |
| 16 | `event_bus` | Barramento pub/sub de eventos de runtime (`EventBus`, `EventListener`). | Pendente |
| 17 | `evolution` | Evolucao do agent (hypothesis_pipeline + lesson_pipeline). | Pendente |
| 18 | `extension` | Sistema de extensoes plugaveis. | Pendente |
| 19 | `failure_tracker` | Agregador de falhas por categoria/frequencia. | Pendente |
| 20 | `frontmatter` | Parser de YAML frontmatter em prompts/skills. | Pendente |
| 21 | `handoff_guardrail` | Guardrails declarativos para handoff entre agents. | Pendente |
| 22 | `hooks` | Hooks configuraveis por usuario (pre/post tool-call etc). | Pendente |
| 23 | `hypothesis_pipeline` | Pipeline de hipoteses (sugere → testa → aprende). | Pendente |
| 24 | `jit_instructions` | Instrucoes just-in-time injetadas conforme contexto. | Pendente |
| 25 | `lesson_pipeline` | Pipeline de extracao e persistencia de licoes. | Pendente |
| 26 | `lifecycle_hooks` | Hooks de ciclo de vida (on_start, on_stop, on_error). | Pendente |
| 27 | `loop_state` | Estado observavel do loop atual. | Pendente |
| 28 | `memory_lifecycle` | Ciclo de vida de memorias (create → review → decay → prune). | Pendente |
| 29 | `memory_reviewer` | Agente revisor de memorias (qualidade/relevancia). | Pendente |
| 30 | `observability` | `metrics`, `context_metrics`, `derived_metrics`, `envelope`, `failure_sensors`, `listener`, `loop_detector`, `normalizer`, `otel_exporter`. | Pendente |
| 31 | `onboarding` | Fluxo de onboarding inicial do agent em novo workspace. | Pendente |
| 32 | `output_format` | Formatos de saida (text, json, structured). | Pendente |
| 33 | `persistence` | Persistencia de estado do runtime (sessions, checkpoints). | Pendente |
| 34 | `pilot` | Modo pilot (execucao supervisionada/dry-run). | Pendente |
| 35 | `plugin` | Sistema de plugins externos. | Pendente |
| 36 | `project_config` | Leitura/validacao de `.theo/config.toml` e `CLAUDE.md`. | Pendente |
| 37 | `reflector` | Reflexao pos-acao (o que funcionou? o que aprender?). | Pendente |
| 38 | `retry` | Politicas de retry com backoff exponencial. | Pendente |
| 39 | `roadmap` | Roadmap de execucao (passos planejados). | Pendente |
| 40 | `run_engine` | `AgentRunEngine` — orquestrador de runs multi-fase. | Pendente |
| 41 | `sanitizer` | Sanitizacao de inputs/outputs (PII, secrets, injection). | Pendente |
| 42 | `scheduler` | Scheduler de tarefas (`#[doc(hidden)]`, dead code). | Pendente |
| 43 | `sensor` | Sensores ambientais (file changes, clock, signals). | Pendente |
| 44 | `session_bootstrap` | Bootstrap de sessao (load context, init state). | Pendente |
| 45 | `session_tree` | Arvore de sessoes pai/filho (sub-agents). | Pendente |
| 46 | `skill` | `bundled` skills — empacotadas e distribuidas com o binario. | Pendente |
| 47 | `skill_catalog` | Catalogo de skills disponiveis no workspace. | Pendente |
| 48 | `skill_reviewer` | Revisor automatico de skills (lint + qualidade). | Pendente |
| 49 | `snapshot` | Snapshots de estado (mais leves que checkpoints). | Pendente |
| 50 | `state_manager` | Gerente central de estado do runtime. | Pendente |
| 51 | `subagent` | `approval`, `builtins`, `mcp_tools`, `parser`, `registry`, `reloadable`, `resume`, `watcher` — sub-agentes isolados. | Pendente |
| 52 | `subagent_runs` | Runs de sub-agentes com tracking proprio. | Pendente |
| 53 | `system_prompt_composer` | Composicao dinamica de system prompts. | Pendente |
| 54 | `task_manager` | Gerente de tasks (fila, prioridade, dependencias). | Pendente |
| 55 | `tool_bridge` | Ponte entre runtime e `theo-tooling`. | Pendente |
| 56 | `tool_call_manager` | Dispatcher/tracker de tool-calls em voo. | Pendente |
| 57 | `transcript_indexer` | Indexacao de transcripts para busca/retrieval. | Pendente |
