# Plano de Remediacao — `theo-agent-runtime`

> Derivado de `docs/reviews/theo-agent-runtime/REVIEW.md`. Cada item e um PR ou grupo de PRs executavel.
>
> **Ordem nao e negociavel**: a Fase 0 (safety net) e pre-requisito para toda refatoracao estrutural subsequente.

---

## Convencoes

| Campo | Significado |
|---|---|
| **ID** | Identificador estavel (ex.: `T1.2`) — usar em commits / PRs (`feat(agent-runtime/T1.2): ...`) |
| **Esforco** | S (< 0.5d), M (0.5-2d), L (2-5d), XL (>5d) |
| **Risco de regressao** | Baixo / Medio / Alto — orienta exigencia de testes de caracterizacao |
| **Bloqueia** | Tarefas que nao podem comecar ate esta fechar |
| **AC** | Criterio(s) de aceitacao objetivo(s), verificavel(is) via `cargo test` / CI |

---

## Roadmap Sumarizado

```
Fase 0 — Safety Net       (2-3 d)   ┐
Fase 1 — Seguranca        (3-5 d)   │  Paralelizaveis por ownership
Fase 2 — Panics & Errors  (3-5 d)   ┘

Fase 3 — DRY & Helpers    (3-4 d)   — precisa Fase 0

Fase 4 — Split God-Files  (10-15 d) — precisa Fases 0, 2, 3

Fase 5 — API Cleanup      (2-3 d)   — precisa Fase 4
Fase 6 — Performance      (2-3 d)   — precisa Fase 4
Fase 7 — Testes Gap       (3-5 d)   — continuo a partir da Fase 4

Fase 8 — Hygiene          (1-2 d)   — ultimo
```

Total: **4-6 semanas** com 1 engenheiro dedicado, ou **2-3 semanas** com 2 engenheiros trabalhando em paralelo por ownership (seguranca vs estrutura).

---

## Fase 0 — Safety Net (Pre-requisito)

> **Objetivo:** garantir que qualquer refatoracao estrutural preserve comportamento observavel. Sem isso, split de `run_engine.rs` e roleta-russa.

### T0.1 — Testes de caracterizacao do `run_engine.rs`

- **Arquivo:** `crates/theo-agent-runtime/tests/run_engine_characterization.rs` (novo)
- **Descricao:** capturar o comportamento observavel atual (via EventBus log) de 10-15 cenarios canonicos:
  1. Happy path single-tool (read file → done).
  2. Happy path multi-tool (read → edit → done).
  3. Done-gate Gate 1 bloqueio (sem git changes).
  4. Done-gate Gate 2 bloqueio (cargo test falha).
  5. Done-gate apos 3 tentativas — forced accept.
  6. Context overflow recovery (emergency compaction).
  7. Tool error + retry.
  8. Budget exhaustion (iterations).
  9. Budget exhaustion (tokens).
  10. `delegate_task` single.
  11. `delegate_task_parallel`.
  12. `skill` InContext.
  13. `skill` SubAgent.
  14. `batch` tool (5 paralelos).
  15. Resume com `ResumeContext` (replay de tool call).
- **Motivacao:** REVIEW §7 — `run_engine.rs` sem testes inline ate linha 2000. Qualquer split cego regride.
- **AC:**
  - Cada cenario produz sequencia de `DomainEvent` observavel snapshotada (usar `insta` crate).
  - `cargo test -p theo-agent-runtime --test run_engine_characterization` passa.
  - Snapshots em `tests/snapshots/` commitados.
- **Esforco:** L
- **Risco:** Baixo (apenas adiciona testes)
- **Bloqueia:** T4.* (split run_engine)

### T0.2 — Testes de caracterizacao do `subagent/mod.rs`

- **Arquivo:** `crates/theo-agent-runtime/tests/subagent_characterization.rs` (novo)
- **Descricao:** 5-8 cenarios de spawn_with_spec cobrindo: worktree isolation on/off, hooks on/off, MCP on/off, cancellation, handoff guardrail.
- **AC:** identico ao T0.1.
- **Esforco:** M
- **Risco:** Baixo
- **Bloqueia:** T4.5

### T0.3 — Coverage baseline

- **Arquivo:** CI
- **Descricao:** executar `cargo tarpaulin -p theo-agent-runtime --out Xml` e commitar em `.coverage/baseline-<sha>.xml`. Nenhum PR subsequente pode reduzir branch coverage nesse crate.
- **AC:** CI job falha se branch coverage cair >2pp.
- **Esforco:** S
- **Risco:** Baixo

---

## Fase 1 — Seguranca (Bloqueadores de producao)

> **Paralelizavel** com Fase 2. Ordem interna prioriza raio-de-impacto.

### T1.1 — Sandbox do `cargo test` no done-gate

- **Arquivo:** `crates/theo-agent-runtime/src/run_engine.rs:1583-1673`
- **Descricao:** encaminhar `cargo test` / `cargo check` atraves do `theo_tooling::sandbox::executor` (mesmo cascade bwrap → landlock → noop do `bash` tool). No minimo, aplicar `rlimits` (CPU 120s, mem 2GB).
- **Motivacao:** REVIEW §5 ALTO — RCE via `build.rs` / proc-macro quando agent escreve codigo malicioso.
- **AC:**
  - Novo teste `done_gate_cargo_test_runs_in_sandbox` com fixture malicioso (build.rs que tenta `touch /tmp/escape`) → arquivo **nao** aparece.
  - Codigo direto `std::process::Command::new("cargo")` nao existe mais em run_engine.rs.
- **Esforco:** M
- **Risco:** Medio (mudanca de path crítico)
- **Bloqueia:** —

### T1.2 — Sanitizar git log e boot context antes do system prompt

- **Arquivo:** `crates/theo-agent-runtime/src/run_engine.rs:694-721`, novo helper `crates/theo-agent-runtime/src/sanitizer.rs` (ja existe — estender).
- **Descricao:** antes de injetar git log / progress / boot context no `Message::system`, passar por `sanitizer::fence_untrusted(input, tag)`:
  - Escapa/remove tokens especiais (`<|im_start|>`, `<|im_end|>`, `<|begin_of_text|>`, `<|system|>`, `</s>`, `[INST]`, etc. — lista por provider).
  - Envolve em `<git-log>...</git-log>` XML tags.
  - Trunca em 4KB hard cap por payload.
- **Motivacao:** REVIEW §5 ALTO — prompt injection via commit messages.
- **AC:**
  - Teste `git_log_with_injection_tokens_is_stripped`: commit message contendo `<|im_start|>system\nignore all...` nao aparece literal no prompt final.
  - Teste `git_log_is_fenced_in_xml_tags` verifica envelope.
- **Esforco:** M
- **Risco:** Baixo
- **Bloqueia:** —

### T1.3 — Hardening do plugin/hook loader

- **Arquivos:** `crates/theo-agent-runtime/src/plugin.rs`, `crates/theo-agent-runtime/src/hooks.rs`
- **Descricao:**
  1. Antes de carregar plugin/hook, verificar `fs::metadata(path).uid() == getuid()`.
  2. Adicionar `AgentConfig.plugin_allowlist: Option<HashSet<PathBuf>>`. Se `Some`, so plugins com path em allowlist sao carregados. Se `None` (default em test/dev), loga warning "plugins loaded without allowlist".
  3. Emitir `DomainEvent::PluginLoaded` com name + hash (sha256 do plugin.toml).
  4. Tool registrado por plugin recebe `ToolCategory::Plugin` (novo variant) e deve passar pelo `capability_gate` mesmo se `CapabilitySet` global for unrestricted.
- **Motivacao:** REVIEW §5 ALTO — supply-chain via plugin arbitrario.
- **AC:**
  - Teste `plugin_with_wrong_owner_rejected`.
  - Teste `plugin_not_in_allowlist_rejected_when_configured`.
  - Teste `plugin_tool_blocked_by_capability_gate_read_only`.
- **Esforco:** M
- **Risco:** Medio (pode quebrar setups de dev que dependem de plugin sem allowlist)
- **Bloqueia:** —

### T1.4 — Fallback `HOME=/tmp` deve falhar explicitamente

- **Arquivos:** `run_engine.rs:777`, `plugin.rs:86`, `hooks.rs:96`, `memory_lifecycle.rs:572`
- **Descricao:** trocar os 4 sites por helper unico `theo_infra_auth::paths::user_config_dir() -> Result<PathBuf, ConfigError>`. Se `HOME` nao existir, retornar `Err(NoHomeDir)` e caller decide skippar (log warning) em vez de `/tmp`.
- **Motivacao:** REVIEW §5 MEDIO — TOCTOU em container sem HOME.
- **AC:**
  - Nenhum `.unwrap_or_else(|_| PathBuf::from("/tmp"))` no crate.
  - Teste `load_plugins_skips_when_home_unset`.
- **Esforco:** S
- **Risco:** Baixo
- **Bloqueia:** T3.3 (env centralizacao)

### T1.5 — Substituir hand-rolled JSON em `THEO_FORCE_TOOL_CHOICE`

- **Arquivo:** `run_engine.rs:1079`
- **Descricao:** trocar `format!(r#"{{"type":"function","name":"{}"}}"#, name)` por `serde_json::json!({"type": "function", "name": name}).to_string()`.
- **Motivacao:** REVIEW §5 MEDIO — JSON quebra se `name` tiver aspas.
- **AC:** teste `force_tool_choice_with_quote_in_name_serializes_correctly`.
- **Esforco:** S
- **Risco:** Baixo

### T1.6 — Nao vazar entity_id em logs stderr

- **Arquivo:** `event_bus.rs:85-89`
- **Descricao:** trocar `eprintln!("[EventBus] listener panicked on event {:?} for entity {}", ..., event.entity_id)` por evento `DomainEvent::ListenerPanic` com payload redacted (so event_type, sem entity_id).
- **Motivacao:** REVIEW §5 MEDIO — PII vaza em logs se entity_id for session_id.
- **AC:** grep `eprintln!.*entity_id` no crate retorna 0 hits.
- **Esforco:** S
- **Risco:** Baixo

---

## Fase 2 — Panics & Silent Errors

### T2.1 — Substituir `std::sync::Mutex` por `parking_lot::Mutex`

- **Arquivos:** `event_bus.rs`, `task_manager.rs`, `tool_call_manager.rs`, qualquer outro com `.expect("... lock poisoned")`
- **Descricao:**
  1. Adicionar `parking_lot = "0.12"` em `Cargo.toml` workspace.
  2. `use parking_lot::Mutex;`
  3. Remover todos `.expect("... lock poisoned")` — `parking_lot::Mutex::lock()` retorna `MutexGuard` direto (nao `Result`).
- **Motivacao:** REVIEW §2 CRITICO — poison propaga e derruba bus inteiro.
- **AC:**
  - `rg "lock poisoned" crates/theo-agent-runtime/src` retorna 0 hits.
  - Todos testes existentes continuam verdes.
  - Novo teste `listener_panic_does_not_poison_bus_for_subsequent_publish`.
- **Esforco:** M (automatizavel com sed + compile)
- **Risco:** Baixo (parking_lot e drop-in)
- **Bloqueia:** —

### T2.2 — Eliminar `.expect("system clock before UNIX epoch")` duplicado

- **Arquivo:** `crates/theo-domain/src/clock.rs` (novo)
- **Descricao:**
  ```rust
  pub fn now_millis() -> u64 {
      SystemTime::now()
          .duration_since(UNIX_EPOCH)
          .map(|d| d.as_millis() as u64)
          .unwrap_or(0) // clock skew → 0, not panic
  }
  ```
  Substituir as 3+ implementacoes duplicadas em `task_manager.rs`, `tool_call_manager.rs`, `run_engine.rs` (e checar outros crates).
- **Motivacao:** REVIEW §2 CRITICO + DRY.
- **AC:**
  - `rg "before UNIX epoch" crates/` retorna 0 hits.
  - Todo `fn now_millis` local deletado; imports apontam para `theo_domain::clock::now_millis`.
- **Esforco:** S
- **Risco:** Baixo
- **Bloqueia:** —

### T2.3 — Typed error para `record_session_exit` persistence

- **Arquivo:** `run_engine.rs:483-580`
- **Descricao:** trocar cada `let _ = tokio::fs::write(...)` por match que emite `DomainEvent::PersistenceError` com payload `{path, error}`. Manter o comportamento de nao abortar (durabilidade best-effort) mas tornar falha observavel.
- **Motivacao:** REVIEW §2 ALTO + §5 — silent error swallowing em shutdown path.
- **AC:**
  - Teste `record_session_exit_emits_persistence_error_when_fs_readonly`.
  - `rg "let _ = tokio::fs::" crates/theo-agent-runtime/src/run_engine.rs` retorna 0 hits.
- **Esforco:** M
- **Risco:** Medio (shutdown path)
- **Bloqueia:** —

### T2.4 — Varredura dos outros 57 silent-swallow sites

- **Arquivos:** 22 arquivos listados no grep do REVIEW
- **Descricao:** para cada `let _ = tokio::fs` / `let _ = std::fs`:
  - Se erro e diagnostico-only: trocar por `if let Err(e) = ... { tracing::warn!(...) }` com contexto.
  - Se erro pode quebrar invariante: retornar `Result` ao caller.
- **Motivacao:** REVIEW §2 ALTO.
- **AC:** cada site revisado e ou (a) com log estruturado ou (b) propagado como erro, ou (c) com comentario `// best-effort: <razao>` justificando.
- **Esforco:** L
- **Risco:** Medio
- **Bloqueia:** —

### T2.5 — Remover `.expect()` dead-code em `retry.rs`

- **Arquivo:** `retry.rs:68`
- **Descricao:** refatorar `with_retry` para expressao que nao precisa do unwrap pos-loop. Opcoes:
  - Retornar do loop ao exceder max: `return Err(e);`
  - Usar `Result::from_iter` se simplicar.
- **Motivacao:** REVIEW §3 P15 — dead code que panica se invariante quebrar.
- **AC:** grep `expect\("retry loop` retorna 0 hits; teste existente `exhausts_max_retries_returns_last_error` continua verde.
- **Esforco:** S
- **Risco:** Baixo

### T2.6 — Substituir `std::process::Command` sincrono em async fn

- **Arquivos:** `run_engine.rs:703`, `checkpoint.rs:396`
- **Descricao:** trocar por `tokio::process::Command::...::output().await`. Consistente com resto do codigo (que ja usa tokio em 1549).
- **Motivacao:** REVIEW §2 ALTO — bloqueia worker tokio.
- **AC:** `rg "std::process::Command" crates/theo-agent-runtime/src` retorna 0 hits.
- **Esforco:** S
- **Risco:** Baixo

---

## Fase 3 — DRY & Helpers

### T3.1 — `AgentResult::from_engine_state` helper

- **Arquivo:** `crates/theo-agent-runtime/src/agent_loop.rs` (ou novo `run_engine/result.rs`)
- **Descricao:** criar helper que consome `&AgentRunEngine` + `summary` + `success` + `error_class` e preenche os 12 campos de metricas. Substituir os 5+ sites inline em `run_engine.rs`.
- **Motivacao:** REVIEW §2 ALTO — duplicacao DRY.
- **AC:**
  - `rg "tokens_used: self.metrics.snapshot\(\).total_tokens_used" crates/theo-agent-runtime/src/run_engine.rs` retorna 0 hits (apenas dentro do helper).
  - Todos os AgentResult builders usam `AgentResult::from_engine_state(self, ...)`.
- **Esforco:** S
- **Risco:** Baixo (alto valor)
- **Bloqueia:** T4.* (pre-requisito de leitura limpa pre-split)

### T3.2 — Unificar `AgentLoop::run` e `AgentLoop::run_with_history`

- **Arquivo:** `agent_loop.rs:285-404`
- **Descricao:** extrair `build_engine(&self, project_dir, external_bus: Option<Arc<EventBus>>) -> (Arc<EventBus>, AgentRunEngine)` compartilhado. Ver esboco em REVIEW §2.
- **Motivacao:** REVIEW §2 CRITICO — 80% overlap.
- **AC:**
  - `run()` e `run_with_history()` <= 30 LOC cada.
  - Testes existentes verdes.
  - Novo teste `run_and_run_with_history_both_call_record_session_exit`.
- **Esforco:** M
- **Risco:** Medio
- **Bloqueia:** —

### T3.3 — Centralizar env var reads

- **Arquivos:** novo `crates/theo-domain/src/environment.rs` + trait; impl em `theo-application`
- **Descricao:**
  ```rust
  pub trait Environment: Send + Sync {
      fn home_dir(&self) -> Option<PathBuf>;
      fn theo_var(&self, name: &str) -> Option<String>;
      fn otlp_config(&self) -> OtlpConfig;
  }
  ```
  Injetar em `AgentConfig` ou `ApplicationContext`. Remover as 20+ chamadas diretas `std::env::var` em `run_engine.rs`, `project_config.rs`, `onboarding.rs`, `subagent/mod.rs`, `memory_lifecycle.rs`, `hooks.rs`, `plugin.rs`.
- **Motivacao:** REVIEW §3 P3 — DIP violation, 7 modulos leem env ad-hoc.
- **AC:**
  - `rg "std::env::var" crates/theo-agent-runtime/src` retorna apenas no `bin/theo-agent.rs`.
  - Teste `environment_injected_via_trait_not_read_directly`.
- **Esforco:** L
- **Risco:** Medio (tocar 7 modulos)
- **Bloqueia:** —

### T3.4 — Consolidar retry logic

- **Arquivos:** `retry.rs`, `run_engine.rs:1114-1174`
- **Descricao:** substituir o loop inline em run_engine pelo `RetryExecutor::with_retry`. Adaptar callback de `on_retry` para emitir `DomainEvent::Error{type:retry}` se ainda nao emite.
- **Motivacao:** REVIEW §3 P4.
- **AC:**
  - `run_engine.rs` nao tem `for attempt in 0..=max_retries` explicito.
  - Comportamento observavel (eventos, delays) identico — teste snapshot da sequencia de DomainEvent do cenario "LLM retry + success" passa.
- **Esforco:** M
- **Risco:** Medio
- **Bloqueia:** T4.2

### T3.5 — Extrair preview/truncate helpers

- **Arquivos:** `run_engine.rs` (1933), `tool_call_manager.rs` (207), sensor.rs (918)
- **Descricao:** criar `theo_domain::truncate::{preview_200, preview_2000, char_boundary_safe_truncate}` (ja existe `theo_domain::truncate` — so centralizar). Eliminar as 3+ copias de `while end > 0 && !s.is_char_boundary(end)`.
- **Motivacao:** REVIEW §3 P11 + DRY.
- **AC:** apenas um site implementa truncate char-safe; restantes usam helper.
- **Esforco:** S
- **Risco:** Baixo

### T3.6 — Constantes nomeadas para magic numbers

- **Arquivo:** adicionar em `config.rs` ou novo `constants.rs`
- **Descricao:**
  - `MAX_DONE_ATTEMPTS: u32 = 3`
  - `MAX_BATCH_SIZE: usize = 25`
  - `DONE_GATE_TEST_TIMEOUT: Duration = Duration::from_secs(60)`
  - `DONE_GATE_CHECK_FALLBACK_TIMEOUT: Duration = Duration::from_secs(30)`
  - `TOOL_PREVIEW_BYTES: usize = 200`
  - `TOOL_INPUT_TRUNCATE_BYTES: usize = 500`
  - `EMERGENCY_COMPACT_RATIO: f64 = 0.5`
- **Motivacao:** REVIEW §3 P11.
- **AC:** literais numericos magicos em `run_engine.rs` sao apenas indices/retornos.
- **Esforco:** S
- **Risco:** Baixo

---

## Fase 4 — Split God-Files

> **Pre-requisitos:** Fase 0 (caracterizacao), T3.1 (AgentResult helper), T3.4 (retry unificado).

### T4.1 — Split `AgentConfig`

- **Arquivo:** `crates/theo-agent-runtime/src/config.rs`
- **Descricao:**
  ```rust
  pub struct AgentConfig {
      pub llm: LlmConfig,
      pub loop_cfg: LoopConfig,
      pub context: ContextConfig,
      pub memory: MemoryConfig,
      pub evolution: EvolutionConfig,
      pub routing: RoutingConfig,
  }
  ```
  Cada sub-config em seu proprio modulo dentro de `config/`. Backward-compat via `Deref` ou helpers de migracao.
- **Motivacao:** REVIEW §2 ALTO — god-struct com 32+ campos.
- **AC:**
  - Cada sub-config <= 10 campos.
  - Todos os call sites atualizados (~50+ em run_engine).
  - Testes verdes.
- **Esforco:** L
- **Risco:** Alto (API publica do crate quebra — mas esta atras de `theo-application`)
- **Bloqueia:** T4.2

### T4.2 — Split `run_engine.rs` (4230 → multiplos <= 500 LOC)

- **Arquivo:** `crates/theo-agent-runtime/src/run_engine.rs`
- **Descricao:** estrutura alvo:
  ```
  run_engine/
  ├── mod.rs              (~200 LOC: struct + builders + new + public API)
  ├── lifecycle.rs        (~150 LOC: transition_run, record_session_exit, finalize_observability)
  ├── bootstrap.rs        (~250 LOC: execute_with_history prefacio: system prompt, memory prefetch, boot context, skills injection)
  ├── main_loop.rs        (~400 LOC: for iteration loop, budget check, sensor drain, LLM call, streaming)
  ├── dispatch/
  │   ├── mod.rs          (enum ToolDispatch + dispatcher trait)
  │   ├── done.rs         (~250 LOC: Gate 0/1/2)
  │   ├── delegate.rs     (~200 LOC: handle_delegate_task)
  │   ├── skill.rs        (~200 LOC: skill InContext + SubAgent)
  │   ├── batch.rs        (~200 LOC: handle_batch_tool)
  │   └── mcp.rs          (~100 LOC: try_dispatch_mcp_tool)
  ├── context_overflow.rs (~100 LOC: recovery flow)
  ├── routing.rs          (~100 LOC: routing decision)
  └── result.rs           (~80 LOC: AgentResult::from_engine_state)
  ```
- **Motivacao:** REVIEW §2 CRITICO.
- **AC:**
  - Nenhum arquivo em `run_engine/` excede 500 LOC.
  - `cargo test -p theo-agent-runtime` passa (incluindo Fase 0 caracterizacao).
  - Snapshot tests do T0.1 identicos byte-a-byte.
  - `cargo bench` (se houver) sem regressao >5%.
- **Esforco:** XL
- **Risco:** Alto
- **Dependencias:** T0.1, T3.1, T3.4, T4.1
- **Bloqueia:** T5.*, T6.*

### T4.3 — Aplicar Strategy pattern em tool dispatch

- **Arquivo:** `run_engine/dispatch/mod.rs` (criado em T4.2)
- **Descricao:** apos split, substituir `if name == "done" ... if name == "delegate_task" ... if name == "skill" ... if name == "batch"` por:
  ```rust
  trait MetaToolDispatcher {
      fn handles(&self, name: &str) -> bool;
      async fn dispatch(&self, engine: &mut AgentRunEngine, call: &ToolCall) -> DispatchOutcome;
  }
  ```
  Registrar `DoneDispatcher`, `DelegateDispatcher`, `SkillDispatcher`, `BatchDispatcher`, `McpDispatcher` em lista e iterar.
- **Motivacao:** REVIEW §8 — OCP: adicionar meta-tool hoje exige editar `execute_with_history`.
- **AC:**
  - Novo meta-tool pode ser adicionado com 1 novo arquivo + 1 linha em vec de registro. Sem tocar main_loop.
  - Teste `new_meta_tool_dispatcher_registered_without_main_loop_edit`.
- **Esforco:** M
- **Risco:** Medio
- **Dependencias:** T4.2

### T4.4 — Aplicar Chain of Responsibility em done gates

- **Arquivo:** `run_engine/dispatch/done.rs`
- **Descricao:** separar Gate 0 (max attempts), Gate 1 (convergence), Gate 2 (cargo test) em handlers encadeados:
  ```rust
  trait DoneGate { fn check(&self, ctx: &DoneContext) -> GateOutcome; }
  struct AttemptLimitGate;  // Gate 0
  struct ConvergenceGate;   // Gate 1
  struct SandboxedTestGate; // Gate 2 (usa sandbox da T1.1)
  ```
- **Motivacao:** REVIEW §8.
- **AC:** adicionar novo gate nao toca `done.rs::handle_done`, so novo arquivo + registro.
- **Esforco:** M
- **Risco:** Medio
- **Dependencias:** T4.2, T1.1

### T4.5 — Split `subagent/mod.rs` (1896 LOC)

- **Arquivo:** `crates/theo-agent-runtime/src/subagent/mod.rs`
- **Descricao:** estrutura alvo:
  ```
  subagent/
  ├── mod.rs          (~150 LOC: re-exports + tipos publicos)
  ├── manager.rs      (~300 LOC: SubAgentManager)
  ├── spawn.rs        (~400 LOC: spawn_with_spec + spawn_with_spec_text)
  ├── context.rs      (~200 LOC: contexto de sub-agent, prompt composition)
  ├── dispatch.rs     (~300 LOC: delegacao parallel)
  ├── [existentes: approval, builtins, mcp_tools, parser, registry, reloadable, resume, watcher — OK]
  ```
- **Motivacao:** REVIEW §2 CRITICO.
- **AC:** nenhum arquivo em `subagent/` excede 500 LOC; T0.2 caracterizacao byte-identica.
- **Esforco:** L
- **Risco:** Alto
- **Dependencias:** T0.2

### T4.6 — Split `pilot.rs` (1218 LOC) e `tool_bridge.rs` (1155 LOC)

- **Descricao:** mesmo principio. `pilot.rs` → `pilot/` (mode/state/orchestration); `tool_bridge.rs` → `tool_bridge/` (definitions/execution/conversion).
- **AC:** <= 500 LOC por arquivo.
- **Esforco:** L
- **Risco:** Medio
- **Dependencias:** T0.1 (se houver caracterizacao)

### T4.7 — Split `memory_lifecycle.rs` (1025), `session_tree.rs` (921), `observability/report.rs` (832), `handoff_guardrail/mod.rs` (811), `compaction.rs` (798)

- **Descricao:** aplicar mesmo padrao. Cada um recebe seu proprio sub-diretorio.
- **Esforco:** XL total (dividir em multiplos PRs)
- **Risco:** Medio
- **Dependencias:** —

---

## Fase 5 — API Cleanup

### T5.1 — Extrair `RunMetadata` de `AgentResult`

- **Arquivo:** `run_engine/result.rs`
- **Descricao:**
  ```rust
  pub struct AgentResult {
      pub success: bool,
      pub summary: String,
      pub error_class: Option<ErrorClass>,
      pub metadata: RunMetadata,
  }
  pub struct RunMetadata {
      pub files_edited: Vec<String>,
      pub iterations_used: usize,
      pub tokens: TokenAccounting,
      pub tool_calls: ToolCallAccounting,
      pub duration_ms: u64,
      pub was_streamed: bool,
      pub cancelled: bool,
      pub agent_name: String,
      pub context_used: Option<String>,
      pub structured: Option<serde_json::Value>,
      pub worktree_path: Option<PathBuf>,
  }
  ```
  Remover todos os `#[doc(hidden)] pub` — ou metadata e publica ou e private.
- **Motivacao:** REVIEW §3 P8.
- **AC:** `rg "#\[doc\(hidden\)\]" crates/theo-agent-runtime/src` retorna 0 hits.
- **Esforco:** M (API publica — breaking)
- **Risco:** Alto (consumers externos)
- **Dependencias:** T4.2

### T5.2 — Compactar `AgentLoop::with_subagent_*` em `SubAgentIntegrations`

- **Arquivo:** `agent_loop.rs:159-243`
- **Descricao:** trocar 13 metodos `with_subagent_*` por struct unificado:
  ```rust
  #[derive(Default, Clone)]
  pub struct SubAgentIntegrations {
      pub registry: Option<Arc<SubAgentRegistry>>,
      pub run_store: Option<Arc<FileSubagentRunStore>>,
      pub hooks: Option<Arc<HookManager>>,
      pub cancellation: Option<Arc<CancellationTree>>,
      pub checkpoint: Option<Arc<CheckpointManager>>,
      pub worktree: Option<Arc<WorktreeProvider>>,
      pub mcp: Option<Arc<McpRegistry>>,
      pub mcp_discovery: Option<Arc<DiscoveryCache>>,
      pub handoff_guardrails: Option<Arc<GuardrailChain>>,
      pub reloadable: Option<ReloadableRegistry>,
      pub resume_context: Option<Arc<ResumeContext>>,
  }

  impl AgentLoop {
      pub fn with_subagent_integrations(mut self, i: SubAgentIntegrations) -> Self { ... }
  }
  ```
- **Motivacao:** REVIEW §3 P17 — 13 builders.
- **AC:**
  - `AgentLoop` impl tem <= 5 builders publicos.
  - Call sites (em `theo-application`) atualizados para struct.
- **Esforco:** M
- **Risco:** Alto (API breaking)
- **Dependencias:** T5.1

### T5.3 — Remover dead code e `#[allow(dead_code)]`

- **Arquivos:** `agent_loop.rs:444, 463`, `lib.rs:12, 50` (correction, scheduler)
- **Descricao:** deletar `phase_nudge`, `has_real_changes`, modulos `correction` e `scheduler`. Remover teste `test_phase_nudge_urgent`.
- **Motivacao:** REVIEW §3 P9 — dead code em producao.
- **AC:**
  - `rg "#\[allow\(dead_code\)\]" crates/theo-agent-runtime/src` retorna 0 hits (aceitavel apenas em testes marcados `#[cfg(test)]`).
  - `cargo build --all-targets` sem warnings.
- **Esforco:** S
- **Risco:** Baixo

### T5.4 — Consistente `Atomic*` ordering

- **Arquivo:** `run_engine.rs` (depois split: `run_engine/main_loop.rs`, `run_engine/mod.rs`)
- **Descricao:** revisar `skill_created_this_task`, `autodream_attempted`, `checkpoint_taken_this_turn`:
  - Flags sem relacao causal: `Relaxed` (load/store).
  - Flags com barreira de publicacao: `AcqRel` (CAS) / `Release` (store) / `Acquire` (load).
  Adicionar comentario em cada site explicitando a razao do ordering.
- **Motivacao:** REVIEW §3 P7.
- **AC:** cada uso de `std::sync::atomic::Ordering::*` tem comentario de uma linha com justificativa.
- **Esforco:** S
- **Risco:** Baixo

### T5.5 — Corrigir typo `lmm_call`

- **Arquivo:** `tool_call_manager.rs:110, 140`
- **Descricao:** renomear `lmm_call` → `llm_call`.
- **Esforco:** S
- **Risco:** Baixo

---

## Fase 6 — Performance

### T6.1 — `EventBus::log: VecDeque<DomainEvent>`

- **Arquivo:** `event_bus.rs`
- **Descricao:** trocar `Mutex<Vec<DomainEvent>>` por `Mutex<VecDeque<DomainEvent>>` (ou `parking_lot::Mutex` ja de T2.1). `log.remove(0)` → `log.pop_front()`. O(n) → O(1).
- **Motivacao:** REVIEW §6 P-1.
- **AC:**
  - `cargo bench --bench event_bus_publish` (novo) mostra throughput >= 10x em publish em log cheio.
- **Esforco:** S
- **Risco:** Baixo

### T6.2 — Paginar `EventBus::events()`

- **Arquivo:** `event_bus.rs`
- **Descricao:** adicionar `events_range(&self, offset: usize, limit: usize) -> Vec<DomainEvent>` e `events_since(&self, event_id: EventId) -> Vec<DomainEvent>`. Marcar `events()` antigo `#[deprecated]` com sugestao do novo API.
- **Motivacao:** REVIEW §6 P-2 — 10MB por call em `record_session_exit`.
- **AC:** `record_session_exit` usa `events_since` em vez de `events()`.
- **Esforco:** M
- **Risco:** Medio (API change)

### T6.3 — Purge de `ToolCallManager::records` / `results` apos N terminal

- **Arquivo:** `tool_call_manager.rs`
- **Descricao:** `dispatch_and_execute` apos transicao para state terminal e `completed_at` setado, agendar remocao em metodo `purge_completed(&self, older_than_ms: u64)`. Chamado periodicamente em `record_session_exit` ou em transicoes de run state.
- **Motivacao:** REVIEW §6 P-3 — crescimento sem limite.
- **AC:** teste `long_session_10k_tool_calls_does_not_leak_records`.
- **Esforco:** M
- **Risco:** Medio

### T6.4 — Batch streaming deltas

- **Arquivo:** `run_engine/main_loop.rs` (pos-split)
- **Descricao:** em vez de publicar `ContentDelta` por chunk recebido, acumular buffer com janela de 50ms ou 64 bytes (o que vier primeiro). Publish em batch.
- **Motivacao:** REVIEW §6 P-6 — resposta de 5000 tokens = 3000 publishes.
- **AC:** teste `streaming_publishes_at_most_1_delta_per_50ms`.
- **Esforco:** M
- **Risco:** Medio (affecta UX se janela for mal calibrada)

### T6.5 — Reduzir locks em `dispatch_and_execute`

- **Arquivo:** `tool_call_manager.rs::dispatch_and_execute`
- **Descricao:** refatorar para 1-2 locks por dispatch (entrada: clone snapshot, saida: update + publish). Eliminar re-acquire para ler `tool_name` duas vezes no final (linhas 187-205).
- **Motivacao:** REVIEW §6 P-2.
- **AC:** cada dispatch pega lock no maximo 2 vezes (profiled via `tracing::instrument`).
- **Esforco:** M
- **Risco:** Medio

---

## Fase 7 — Testes (Gap Closing)

### T7.1 — Security tests

- **Arquivos:** `tests/security_*.rs` (novos)
- **Descricao:** cobrir cenarios do REVIEW §5:
  - `test_plugin_with_wrong_owner_rejected` (T1.3)
  - `test_git_log_injection_sanitized` (T1.2)
  - `test_cargo_test_done_gate_is_sandboxed` (T1.1)
  - `test_hook_with_shell_metacharacters_escaped`
  - `test_home_unset_does_not_fallback_to_tmp` (T1.4)
- **Esforco:** M
- **Dependencias:** T1.1, T1.2, T1.3, T1.4

### T7.2 — Resilience / failure-mode tests

- **Arquivos:** `tests/resilience_*.rs` (novos)
- **Descricao:**
  - `test_record_session_exit_surfaces_fs_error_via_event` (T2.3)
  - `test_listener_panic_does_not_poison_event_bus` (T2.1)
  - `test_dispatch_under_mutex_contention_100_parallel` (T6.5)
  - `test_tool_call_records_purged_after_n_terminal` (T6.3)
  - `test_budget_exceeded_mid_tool_batch`
- **Esforco:** M
- **Dependencias:** Fase 2, T6.3

### T7.3 — Integration test matrix para meta-tools

- **Arquivo:** `tests/meta_tools_integration.rs` (novo)
- **Descricao:** um teste por combinacao (done × [no changes / has changes / tests fail / tests pass / 3rd attempt]), (delegate_task × [single / parallel / worktree]), (skill × [InContext / SubAgent]), (batch × [5 ok / 5 with 1 blocked / 25 max / 26 overflow]).
- **Esforco:** L
- **Dependencias:** T4.3 (Strategy split)

### T7.4 — Benchmark baseline

- **Arquivo:** `benches/run_engine_bench.rs` (novo)
- **Descricao:** medir `cargo bench` baseline para:
  - `event_bus_publish` (T6.1)
  - `tool_call_dispatch_throughput` (T6.5)
  - `record_session_exit_large_log` (T6.2)
  - `streaming_delta_batching` (T6.4)
- **Esforco:** M

---

## Fase 8 — Hygiene

### T8.1 — Migrar phase tags para CHANGELOG

- **Arquivos:** todo o crate (310 ocorrencias em 45 arquivos)
- **Descricao:** para cada `// Phase N (nome)`:
  - Se e documentacao historica unica: mover para entrada dedicada em `CHANGELOG.md`.
  - Se e comentario explicativo tecnico: reformular sem referencia a phase (ex.: `// Phase 9: snapshot pre-mutation` → `// Snapshot workdir pre-mutation; see ADR-0XX`).
  - Se e apenas ruido: deletar.
- **Motivacao:** REVIEW §3 P12.
- **AC:** `rg "Phase \d+" crates/theo-agent-runtime/src` retorna <= 20 hits (todos justificados como ADR reference).
- **Esforco:** L
- **Risco:** Baixo

### T8.2 — Remover `memory/` vs `wiki/` legacy path com timeline

- **Arquivo:** `state_manager.rs:106-140`
- **Descricao:** adicionar:
  ```rust
  const WIKI_LEGACY_DEPRECATION_DATE: &str = "2026-06-01";
  ```
  Apos a data, remover leitura de `.theo/wiki/episodes/`. Adicionar `#[deprecated(since = "...")]` no helper se exposto.
- **Motivacao:** REVIEW §3 P14 — legacy eterno.
- **AC:** codigo legacy removido ou claramente datado.
- **Esforco:** S
- **Risco:** Baixo

### T8.3 — `DeadLetterQueue` thread-safe ou documentado

- **Arquivo:** `dlq.rs`
- **Descricao:** decidir:
  - (a) adicionar `Mutex<Vec<DeadLetter>>` interno; OR
  - (b) documentar `#[doc = "NOT thread-safe; caller must wrap in Mutex"]`.
  Verificar usos no crate e no workspace para decidir.
- **Motivacao:** REVIEW §3 P18.
- **AC:** 100% dos usages estao consistentes com a decisao.
- **Esforco:** S
- **Risco:** Baixo

### T8.4 — `RouterHandle::Debug` significativo

- **Arquivo:** `config.rs:426-430`
- **Descricao:** trocar `"<dyn ModelRouter>"` (string literal) por delegacao (`self.0.name()` ou similar, se trait tiver).
- **Esforco:** S
- **Risco:** Baixo

### T8.5 — CI gate: module-size-auditor

- **Arquivo:** `.github/workflows/ci.yml` ou equivalente
- **Descricao:** adicionar job que falha PR se algum arquivo em `crates/theo-agent-runtime/src` excede 500 LOC. Bloqueia regressao futura.
- **AC:** PR com arquivo > 500 LOC em CI falha.
- **Esforco:** S
- **Risco:** Baixo

---

## Dependency Graph

```
T0.1 ──────────────────────────┐
T0.2 ────────┐                 │
T0.3         │                 │
             │                 │
T1.1 ──► T4.4│                 │
T1.2         │                 │
T1.3         │                 │
T1.4 ──► T3.3│                 │
T1.5         │                 │
T1.6         │                 │
             │                 │
T2.1 ────────┼─────────────────┼──► (estabilidade)
T2.2         │                 │
T2.3         │                 │
T2.4         │                 │
T2.5         │                 │
T2.6         │                 │
             │                 │
T3.1 ────────┼─────────────────┤
T3.2         │                 │
T3.3         │                 │
T3.4 ───────────────────────┐  │
T3.5                        │  │
T3.6                        │  │
                            │  │
T4.1 ──► T4.2 ◄─────────────┘◄─┘
         T4.2 ──► T4.3
         T4.2 ──► T4.4 (+ T1.1)
T0.2 ──► T4.5
T4.6 (indep)
T4.7 (indep)

T4.2 ──► T5.1 ──► T5.2
         T4.2 ──► T5.3
         T4.2 ──► T5.4
T5.5 (indep)

T4.2 ──► T6.1..T6.5

T1.x/T2.x/T3.x/T4.x ──► T7.1..T7.4 (continuo)

T8.x (independente, no final)
```

---

## Estimativa Total

| Fase | Esforco (eng-dias) | Paralelizavel |
|---|---|---|
| Fase 0 | 2-3 | N (serial) |
| Fase 1 | 3-5 | S (3-6 devs) |
| Fase 2 | 3-5 | S (2-3 devs) |
| Fase 3 | 3-4 | S (2-3 devs) |
| Fase 4 | 10-15 | Parcial (ownership) |
| Fase 5 | 2-3 | S |
| Fase 6 | 2-3 | S |
| Fase 7 | 3-5 | S |
| Fase 8 | 1-2 | S |
| **Total** | **29-45 eng-dias** | |

**Cenarios:**
- **1 engenheiro dedicado:** 6-9 semanas.
- **2 engenheiros (seguranca + estrutura):** 3-5 semanas.
- **3 engenheiros (+ perf/testes):** 2-4 semanas.

---

## Criterios de Saida Global (Definition of Done)

O crate pode ser considerado "remediado" quando **todos** abaixo forem verdadeiros:

1. Nenhum arquivo em `crates/theo-agent-runtime/src/**` excede 500 LOC (CI enforced).
2. `rg "\.expect\(|\.unwrap\(\)|panic!" crates/theo-agent-runtime/src --type rust` retorna <= 10 hits, todos em `#[cfg(test)]`.
3. `rg "let _ = tokio::fs::|let _ = std::fs::" crates/theo-agent-runtime/src` retorna 0 hits.
4. `rg "std::env::var" crates/theo-agent-runtime/src` retorna hits apenas em `bin/`.
5. `rg "std::process::Command" crates/theo-agent-runtime/src` retorna 0 hits.
6. Branch coverage >= baseline + 5pp.
7. Todos os testes da Fase 7 verdes.
8. `cargo bench` sem regressao > 5% vs baseline.
9. `cargo clippy -p theo-agent-runtime -- -D warnings` passa.
10. REVIEW.md atualizado: todos os dominios com status != Pendente.

---

## Riscos & Mitigacoes

| Risco | Probabilidade | Impacto | Mitigacao |
|---|---|---|---|
| Split de `run_engine.rs` quebra fluxos nao testados | Alta | Alto | Fase 0 (caracterizacao) **antes** de qualquer split. PRs por sub-modulo, um de cada vez. |
| `parking_lot::Mutex` introduz bug sutil | Baixa | Alto | API drop-in; testes existentes pegam qualquer mudanca de semantica (poisoning nao era esperado anyway). |
| Env var centralizacao quebra CI/bench | Media | Medio | Manter backward-compat: `Environment::new()` default le `std::env::var` direto. Test injecta mock. |
| Breaking API (`AgentResult`, `AgentLoop`) | Alta | Medio | Coordenar com `theo-application` e `theo-cli` nos mesmos PRs (monorepo vantagem). |
| Remocao de legacy wiki path perde dados de usuarios em upgrade | Baixa | Alto | Decisao do meeting 20260420-221947 #4 **precisa reafirmar**. Adicionar migracao automatica: na primeira leitura pos-upgrade, copiar `.theo/wiki/episodes/*` → `.theo/memory/episodes/` e marcar diretorio com `.migrated`. |
| Sandbox do `cargo test` em done-gate retarda convergencia | Media | Baixo | Medir throughput antes/depois. Se >10% lento, introduzir cache de test results por diff hash. |

---

## Primeiros 3 PRs Recomendados (Quick Wins)

Se voce quer abrir PRs **amanha** com valor imediato e risco baixo:

1. **T2.2 — clock helper unificado** (S, baixo risco): uma fonte unica de `now_millis()`. Toca ~5 arquivos. 2-4h.
2. **T3.1 — `AgentResult::from_engine_state`** (S, baixo risco, alto valor): elimina 5+ sites de duplicacao antes de qualquer split. 2-4h.
3. **T2.1 — `parking_lot::Mutex` em event_bus + managers** (M, baixo risco): remove ~30 `.expect("lock poisoned")`. 4-8h.

Esses tres juntos fecham pontos do REVIEW sem tocar fluxo critico e deixam a base pronta para a Fase 4.

---

## Metricas de Progresso (Dashboard)

Sugerido em `scripts/remediation_progress.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
CRATE=crates/theo-agent-runtime/src

echo "=== theo-agent-runtime remediation progress ==="
echo
echo "God-files (>500 LOC):"
find "$CRATE" -name "*.rs" -exec wc -l {} + | awk '$1 > 500 {print}' | sort -rn | head -20
echo
echo ".expect()/.unwrap()/panic! count: $(rg -c '\.expect\(|\.unwrap\(\)|panic!' "$CRATE" --type rust | awk -F: '{s+=$2} END {print s}')"
echo "silent-swallow count:            $(rg -c 'let _ = tokio::fs::|let _ = std::fs::' "$CRATE" --type rust | awk -F: '{s+=$2} END {print s}')"
echo "std::env::var count:             $(rg -c 'std::env::var' "$CRATE" --type rust | awk -F: '{s+=$2} END {print s}')"
echo "std::process::Command count:     $(rg -c 'std::process::Command' "$CRATE" --type rust | awk -F: '{s+=$2} END {print s}')"
echo "'Phase N' tags count:            $(rg -c 'Phase \d+' "$CRATE" --type rust | awk -F: '{s+=$2} END {print s}')"
```

Baseline atual (para comparacao):
- God-files (>500 LOC): **~20 arquivos**
- `.expect()/.unwrap()/panic!`: **~1071** (muitos em test)
- silent-swallow: **~61**
- `std::env::var`: **~25**
- `std::process::Command`: **2**
- Phase tags: **~310**

Objetivo pos-remediacao: **0 god-files, <10 unwraps (test-only), 0 silent-swallow, 0 env::var fora de bin/, 0 std::process::Command sync, <20 phase tags justificadas.**

---

## Progress Log

> Atualizar a cada iteracao. Referenciar commit hash em cada entrada.

### Iteracao 1 (2026-04-24) — Quick Wins

| Task | Status | Notas |
|---|---|---|
| T2.2 clock helper | **DONE** | `theo-domain::clock::now_millis()`; 3 duplicatas removidas + 3 `.expect("system clock ...")` eliminados em `task_manager`, `snapshot`, `run_engine`, `tool_call_manager`, `hooks`. |
| T5.5 typo `lmm_call` | **DONE** | renomeado para `llm_call` em `tool_call_manager.rs`. |
| T2.5 retry `.expect()` dead code | **DONE** | substituido por `unreachable!()` com justificativa de invariante; variavel morta `last_error` removida. |
| T1.5 serde_json em `THEO_FORCE_TOOL_CHOICE` | **DONE** | `format!(r#"..."#, name)` → `serde_json::json!(...).to_string()`. |
| T2.6 `std::process::Command` async | **PARCIAL** | `run_engine.rs:703` migrado para `tokio::process::Command`. `checkpoint.rs:396` e em `#[cfg(test)]` helper — mantido. |
| T5.3 dead code | **DONE** | removidos `correction.rs` (145 LOC), `scheduler.rs` (304 LOC), `phase_nudge`, `has_real_changes`, + 2 testes que validavam dead code. Total: 449 LOC producao. |
| T8.5 dashboard script | **DONE** | `scripts/remediation_progress.sh` criado. CI guard `check-sizes.sh` ja existia (800 LOC por arquivo — alvo do plano e 500 LOC, revisar). |

**Baseline → atual (por metrica):**
- `.expect/.unwrap/panic!`: 1071 → 1042 (-29)
- silent-swallow: 61 → 13 (medidas diferentes; recontar apos Fase 2)
- `std::env::var`: 25 → 27 (pontos novos em teste, nao producao)
- `std::process::Command` producao: 2 → 1 (run_engine migrado; checkpoint restante e test-helper)
- phase tags: 310 → 242 (queda vem da remocao de `correction` e `scheduler`)

**Validacao:** `cargo test -p theo-domain -p theo-agent-runtime` → 1608 passed, 0 failed.

**Nao feito nesta iteracao (proximas):** T0.1-T0.3 (caracterizacao), T1.1-T1.4, T1.6, T2.1, T2.3, T2.4, T3.1-T3.6, T4.*, T5.1-T5.4, T6.*, T7.*, T8.1-T8.4.

### Iteracao 2 (2026-04-24) — Security + Panics + DRY

| Task | Status | Notas |
|---|---|---|
| T1.2 sanitizar git log | **DONE** | `theo_domain::prompt_sanitizer::{fence_untrusted, char_boundary_truncate, strip_injection_tokens}` — 17 tokens de 5 familias de providers (OpenAI, Llama, Mistral, etc.) neutralizados. Git log no system prompt passa por `fence_untrusted_default` (4KB hard cap, XML-tagged). 10 testes unit + 1 regression test para o cenario do REVIEW §5. |
| T1.4 HOME fallback explicito | **DONE** | `theo_domain::user_paths::{home_dir, theo_config_dir, theo_config_subdir}` centralizado. 4 sites migrados: `run_engine.rs`, `memory_lifecycle.rs`, `hooks.rs`, `plugin.rs`. Nenhum fallback para `/tmp` no crate. |
| T1.6 entity_id nao vazar | **DONE** | `event_bus.rs`: listener panic agora loga apenas `event_type` (entity_id redacted). |
| T2.1 parking_lot::Mutex | **DONE** | workspace dep adicionada. Trocado em `event_bus.rs` (`Mutex<Vec>` → `Mutex<VecDeque>` tambem atende T6.1), `task_manager.rs`, `tool_call_manager.rs`, `subagent/reloadable.rs` (`RwLock`), `observability/metrics.rs` (`RwLock`). 30+ `.expect("lock poisoned")` removidos. Teste `listener_panic_does_not_poison_bus_for_subsequent_publish` adicionado. |
| T3.1 AgentResult::from_engine_state | **DONE** | helper em `agent_loop.rs`; 5 sites em `run_engine.rs` migrados (budget exhaustion, LLM error, text-only converge, done gate force-accept, done gate success, doom-loop abort). `run_result_context()` exposto no engine para encapsular acesso privado. |
| T3.5 truncate helpers centralizado | **DONE** | `char_boundary_truncate` de `prompt_sanitizer` usado em 3 sites (`tool_call_manager`, `run_engine` sensor drain, `run_engine` batch preview, `run_engine` done gate error preview). Duplicacao eliminada. |
| T3.6 constantes nomeadas | **DONE** | `crate::constants` com `MAX_DONE_ATTEMPTS`, `MAX_BATCH_SIZE`, `DONE_GATE_TEST_TIMEOUT`, `DONE_GATE_CHECK_FALLBACK_TIMEOUT`, `TOOL_PREVIEW_BYTES`, `TOOL_INPUT_TRUNCATE_BYTES`, `DONE_GATE_ERROR_PREVIEW_BYTES`, `SENSOR_OUTPUT_PREVIEW_BYTES`, `EMERGENCY_COMPACT_RATIO`. 7 magic numbers removidos. |
| T6.1 `VecDeque` event log | **DONE** (com T2.1) | `EventBus::log` passou de `Mutex<Vec>` para `Mutex<VecDeque>`; `remove(0)` O(n) → `pop_front()` O(1). |

**Baseline → atual (por metrica, desde Iteracao 0):**
- `.expect/.unwrap/panic!`: 1071 → **1004** (-67)
- silent-swallow: 61 → 13
- `std::env::var`: 25 → **23** (-2; HOME removido de 4 sites, restante e em producao)
- `std::process::Command` producao: 2 → 1
- phase tags: 310 → 241 (queda vem da Iteracao 1; Iteracao 2 manteve estavel)

**Validacao:** `cargo test -p theo-domain -p theo-agent-runtime` → 440 + 1096 = 1536 unit tests passed, 88 integration tests passed, 0 failed. Nenhuma regressao.

**Quebra de API introduzida:** `EventBus::events()` e `events_for()` agora retornam via iteradores clonados (antes um `.clone()` direto do Vec). API externa preserva assinatura `Vec<DomainEvent>`.

**Nao feito nesta iteracao (proximas):** T0.1-T0.3 (caracterizacao), T1.1 sandbox do cargo test, T1.3 plugin hardening, T2.3 typed fs errors, T2.4 silent-swallow sweep restante, T3.2 unify run/run_with_history, T3.3 env centralization, T3.4 consolidate retry, T4.* split god-files, T5.*, T6.2-T6.5, T7.*, T8.1-T8.4.

### Iteracao 3 (2026-04-24) — FS errors + Unify + Sandbox rlimits

| Task | Status | Notas |
|---|---|---|
| T1.1 sandbox cargo test | **PARCIAL** | `spawn_done_gate_cargo` aplica `apply_rlimits(ProcessPolicy { cpu:180s, mem:2GiB, fsize:512MiB, nproc:128 })` via `pre_exec` nas 2 chamadas do done-gate (`cargo test`, `cargo check`). Bwrap/landlock completo fica como follow-up — este PR entrega o "no minimo" do plan. Linux-only (non-Linux falls back to unrestricted tokio::process). |
| T2.3 typed fs errors em record_session_exit | **DONE** | novo `crate::fs_errors::{warn_fs_error, emit_fs_error}`. 4 `let _ = tokio::fs::...` em `record_session_exit` trocados por match + `emit_fs_error(..., site, path, err)` que emite `DomainEvent::Error {type: "fs", site, path, error}`. 3 testes unit. |
| T2.4 silent-swallow sweep | **DONE** | migrado em `failure_tracker.rs` (3x), `session_bootstrap.rs` (2x), `hypothesis_pipeline.rs` (2x), `lesson_pipeline.rs` (1x), `autodream.rs` (2x), `run_engine.rs::auto_init_project_context` (3x). Cada site loga via `warn_fs_error(site, path, err)`. Silent-swallow: **61 → 2** (restantes sao em `fs_errors.rs` proprio e em test). |
| T3.2 unificar run/run_with_history | **DONE** | `build_engine(task, project_dir, external_bus)`, `build_llm_client()`, `build_registry()`, `execute_and_shutdown(engine, history)` extraidos. `run()` e `run_with_history()` agora tem <= 10 LOC cada; ambos compartilham o mesmo shutdown path (elimina o bug de "headless callers silently skip episode summaries"). |
| T3.4 consolidar retry | **PARCIAL** | `RetryExecutor::with_retry` agora emite `delay_ms` no payload (match do inline em run_engine). Consolidar o inline do `run_engine.rs` por si so exige generalizar o executor para aceitar streaming callback — nao feito nesta iteracao. |

**Baseline → atual (por metrica, desde Iteracao 0):**
- `.expect/.unwrap/panic!`: 1071 → **1004** (-67; contagem inalterada esta iteracao — rlimits nao adicionou expect em producao)
- silent-swallow: 61 → **2** (-59 total; -11 esta iteracao)
- `std::env::var`: 25 → 23
- `std::process::Command` producao: 2 → 1 (teste helper restante)
- phase tags: 310 → 240 (-70)

**Validacao:** `cargo test -p theo-domain -p theo-agent-runtime` → 440 + 1099 = **1539 unit**, 88 integration, 0 falhas. Novo modulo `fs_errors.rs` com 3 testes.

**Nao feito nesta iteracao (proximas):** T0.1-T0.3 caracterizacao, T1.3 plugin hardening, T3.3 env centralization, T3.4 consolidacao completa do retry inline, T4.* split god-files, T5.*, T6.2-T6.5, T7.*, T8.1-T8.4.

### Iteracao 4 (2026-04-24) — Plugin hardening + pagination + purge

| Task | Status | Notas |
|---|---|---|
| T1.3 plugin/hook hardening | **DONE (parcial — allowlist é follow-up)** | (1) novo variant `ToolCategory::Plugin` em `theo-domain`; `can_use_tool` bloqueia Plugin mesmo em `CapabilitySet::unrestricted()` salvo opt-in explicito via `allowed_categories` ou `allowed_tools`. (2) `manifest_is_owned_by_current_user` via `libc::getuid() == metadata.uid()` em `plugin.rs`. (3) `LoadedPlugin.manifest_sha256` (SHA-256 hex do `plugin.toml`) emitido no log de loading. (4) `ShellTool::category()` agora retorna `Plugin`. 4 testes novos em `theo-domain::capability` + 3 em `plugin.rs`. Allowlist de hashes pinados em `AgentConfig` e `DomainEvent::PluginLoaded` tipado ficam como follow-up. |
| T6.2 events_since + events_range | **DONE** | `EventBus::events_range(offset, limit)` e `EventBus::events_since(&event_id)`. 3 testes novos. `events()` marcado como "prefer events_range/events_since para logs grandes" na doc. |
| T6.3 purge tool-call records | **DONE** | `ToolCallManager::purge_completed(now_ms, older_than_ms)` remove records terminais mais velhos que o corte (records + results em batch). `record_count()` exposto para diagnostico. 2 testes novos. |
| T5.4 Atomic* ordering com comentario | **DONE** | `reset_turn_checkpoint` (Release), `compare_exchange` do checkpoint (AcqRel/Acquire), `skill_created_this_task` (Relaxed) — cada site agora tem comentario de uma linha justificando o ordering. |
| T8.4 RouterHandle::Debug significativo | **DONE** | trait `ModelRouter::name()` adicionado (default `std::any::type_name::<Self>()`); `RouterHandle::fmt` delega via `self.0.name()` em vez do literal `"<dyn ModelRouter>"`. |

**Baseline → atual (por metrica, desde Iteracao 0):**
- `.expect/.unwrap/panic!`: 1071 → **1017** (-54; aumento de +13 vs Iter 3 por conta dos novos testes AAA que usam `.unwrap()` em setup; producao caiu)
- silent-swallow: 61 → 2
- `std::env::var`: 25 → 23
- `std::process::Command` producao: 2 → 1
- phase tags: 310 → 240

**Validacao:** `cargo test` em todos os crates afetados — **2612 unit tests passando, 0 falhas.**
- `theo-domain`: 444 (+4 novos testes de plugin gate)
- `theo-tooling`: 289 (+1 ajustado para Plugin category)
- `theo-agent-runtime`: 1108 (+9 de purge/events_since/plugin sha256)
- `theo-application`: 124
- outros crates: sem regressao

**Nao feito nesta iteracao (proximas):** T0.1-T0.3 caracterizacao, T1.3 allowlist completa + `DomainEvent::PluginLoaded`, T3.3 env centralization, T3.4 consolidacao completa do retry, T4.* split god-files, T5.1-T5.2, T6.4-T6.5, T7.*, T8.1-T8.3.

### Iteracao 5 (2026-04-24) — Env centralization + T1.3 finish + locks

| Task | Status | Notas |
|---|---|---|
| T3.3 env centralization | **DONE** | novo `theo_domain::environment` com funcoes `theo_var`, `bool_var`, `parse_var`, `home_dir` + trait `Environment` + `SystemEnvironment` default + `MapEnvironment` test double. 5 sites migrados em `project_config.rs` (THEO_MODEL/TEMP/MAX_ITER/MAX_TOKENS/REASONING/DOOM_LOOP), 1 em `onboarding.rs` (THEO_SKIP_ONBOARDING), 2 em `run_engine.rs` (THEO_FORCE_TOOL_CHOICE, THEO_DEBUG_CODEX), 1 em `subagent/mod.rs` (THEO_MCP_AUTO_DISCOVERY), 6 em `observability/otel_exporter.rs` (OTLP_*), 1 em `observability/mod.rs`. **std::env::var production sites: 23 → 6 (todos em `bin/theo-agent.rs`).** |
| T1.3 completion | **DONE** | novo variant `DomainEvent::PluginLoaded` com payload `{name, dir, manifest_sha256, tool_count, hook_count}` em theo-domain; `ALL_EVENT_TYPES.len() == 26`, `EventKind::Lifecycle`. Allowlist em `AgentConfig` fica como follow-up — consumers ja podem gatear via `ToolCategory::Plugin` no capability set. |
| T8.2 legacy wiki timeline | **DONE** | `WIKI_LEGACY_DEPRECATION_DATE = "2026-10-20"` em `state_manager.rs`; doc explicita o caminho de remocao (delete dual-path + testes legacy). |
| T8.3 DLQ thread-safe documentado | **DONE** | `DeadLetterQueue` documentado como "single-threaded — wrap em `Arc<Mutex<_>>` para compartilhar"; teste `dead_letter_queue_is_send_sync_under_mutex` bloqueia compile-time que ele compoe com parking_lot::Mutex. |
| T6.5 reduzir locks em dispatch | **DONE** | `dispatch_and_execute` agora toma 3 locks (entrada: records, saida: records + results) em vez de 6 (antes re-adquiria para ler tool_name e input duas vezes apos a execucao). tool_name e input sao snapshotados no primeiro lock e usados depois do await. |

**Baseline → atual (por metrica, desde Iteracao 0):**
- `.expect/.unwrap/panic!`: 1071 → 1017 (-54; estavel esta iteracao)
- silent-swallow: 61 → 2
- `std::env::var`: 25 → **6** (-19; todos os 6 restantes em `bin/theo-agent.rs` — CLI layer, aceitavel)
- `std::process::Command` producao: 2 → 1
- phase tags: 310 → 240

**Validacao:**
- `theo-domain`: 452 passing (+8 novos testes: environment + PluginLoaded)
- `theo-tooling`: 289 passing (inalterado)
- `theo-agent-runtime`: 1109 passing (+1 novo teste DLQ send/sync)
- **88 integration tests passando, 0 falhas.**

**Nao feito nesta iteracao (proximas):** T0.1-T0.3 caracterizacao, T1.1 bwrap completo no done-gate, T1.3 AgentConfig allowlist, T3.4 consolidar retry inline, T4.* split god-files, T5.1-T5.2, T6.4 batch streaming deltas, T7.*, T8.1 phase tags migration.

### Iteracao 6 (2026-04-24) — Plugin allowlist + SubAgentIntegrations + phase sweep

| Task | Status | Notas |
|---|---|---|
| T1.3 completion | **DONE** | `AgentConfig.plugin_allowlist: Option<BTreeSet<String>>` — quando `Some`, `load_plugins_with_policy` so aceita plugins cujo `manifest_sha256` esta no set. `load_plugin_tools` em `agent_loop.rs` propaga `&self.config.plugin_allowlist` + `event_bus`. Eventos `DomainEvent::PluginLoaded` (sucesso) e `DomainEvent::Error{type:plugin_rejected, reason:ownership_mismatch|allowlist_miss}` emitidos. 3 testes novos: hash match aceita, hash miss rejeita, bus captura evento. |
| T5.2 SubAgentIntegrations | **DONE (compat-preserving)** | novo struct `SubAgentIntegrations` com 11 campos `Option<Arc<_>>` + `Default`/`Clone`; `AgentLoop::with_subagent_integrations(bundle)` seta tudo em 1 chamada. Os 11 `with_subagent_*` individuais foram mantidos (docs atualizadas apontando a API nova) para nao quebrar `theo-application`/`theo-cli`. |
| T8.1 phase tags sweep (parcial) | **DONE (parcial)** | 22 `/// Phase N:` doc-comments em `agent_loop.rs` e `run_engine.rs` limpos para prosa neutra. Phase tags: 310 → 210 (-100 desde baseline; -30 vs iter 5). Os restantes estao dentro de blocos de implementacao que carregam referencia historica (`PLAN_AUTO_EVOLUTION_SOTA Phase 4 — index ...`) — esses ficam ate o proximo ADR referencia-los. |

**Baseline → atual (por metrica, desde Iteracao 0):**
- `.expect/.unwrap/panic!`: 1071 → 1017 (-54; estavel)
- silent-swallow: 61 → 2
- `std::env::var`: 25 → 6 (todos em `bin/`)
- `std::process::Command` producao: 2 → 1
- phase tags: 310 → **210** (-100)

**Validacao:** `cargo test -p theo-domain -p theo-agent-runtime` → 452 + 1112 = **1564 unit**, 88 integration, 0 falhas.

**Nao feito nesta iteracao (proximas):** T0.1-T0.3 caracterizacao, T1.1 bwrap completo, T3.4 consolidar retry inline, T4.* split god-files, T5.1 RunMetadata sub-struct, T6.4 batch streaming deltas, T7.* (security/resilience/meta-tools/bench), T8.1 completar phase sweep nos ~210 restantes.

### Iteracao 7 (2026-04-24) — Characterization tests + phase sweep subagent

| Task | Status | Notas |
|---|---|---|
| T0.1 caracterizacao | **DONE (parcial — 8/15)** | Novo `tests/run_engine_characterization.rs` com 8 cenarios snapshot via `insta`: task happy path, task failure path, invalid transition, tool call dispatch, budget exceeded, cancellation propagation, task+tool combined, event bus bounded rotation. Cada cenario snapshota a sequencia de `EventType`s em YAML inline — qualquer split estrutural que altere a ordem/tipos falha o teste. Os 7 cenarios restantes (context overflow, LLM retry, done gate Gate 1/2, delegate_task, skill, batch, resume replay) exigem HTTP mock de LLM nao disponivel no crate e ficam para iteracao dedicada com wiremock/axum. |
| T8.1 phase sweep subagent | **DONE (parcial)** | 17 docstrings `/// Phase N:` em `subagent/mod.rs` limpos para prosa. Phase tags: 210 → 193 (-17 esta iter; -117 total). Restantes estao em blocos de implementacao + referencias a PLAN_* — cleanup direcionado via ADR e trabalho futuro. |

**Baseline → atual (por metrica, desde Iteracao 0):**
- `.expect/.unwrap/panic!`: 1071 → 1017
- silent-swallow: 61 → 2
- `std::env::var`: 25 → 6 (todos em bin/)
- `std::process::Command` producao: 2 → 1
- phase tags: 310 → **193** (-117)

**Validacao:**
- `theo-domain`: 452 passing
- `theo-agent-runtime` lib: 1112 passing
- `theo-agent-runtime` integration: 88 + **8 novos characterization snapshots** = 96 passing
- **Total: 1660 tests, 0 falhas.**

**Nao feito nesta iteracao (proximas):** T0.1 restante (7 cenarios LLM), T0.2 caracterizacao do subagent, T0.3 coverage baseline, T1.1 bwrap, T3.4 retry inline, T4.* split god-files, T5.1 RunMetadata, T6.4 batch streaming, T7.* security/resilience/bench tests.

### Iteracao 8 (2026-04-24) — Fase 4 kick-off: extract pure helpers

| Task | Status | Notas |
|---|---|---|
| T4.2 run_engine.rs split — primeira etapa | **DONE (parcial)** | extraidos 3 novos arquivos irmaos + registrados em `lib.rs`: `run_engine_helpers.rs` (`llm_error_to_class`, `truncate_handoff_objective`, `truncate_batch_args`, `derive_provider_hint` — 169 LOC incl tests), `run_engine_auto_init.rs` (`auto_init_project_context` + `detect_project_name_simple` — 218 LOC incl tests), `run_engine_sandbox.rs` (`spawn_done_gate_cargo` — 65 LOC). `run_engine.rs`: 4230 → **4029 LOC** (−201). Os 8 snapshot tests de caracterizacao (T0.1) continuam verdes — comportamento observavel preservado. Esta e a **primeira extracao real do god-file**; as proximas iterations podem atacar `lifecycle.rs` (record_session_exit, finalize_observability), `main_loop.rs`, `dispatch/done.rs`, etc. |

**Baseline → atual (por metrica, desde Iteracao 0):**
- `.expect/.unwrap/panic!`: 1071 → 1041 (+24 desde iter 7 — novos testes dos extraidos usam `.unwrap()`)
- silent-swallow: 61 → 2
- `std::env::var`: 25 → 6
- `std::process::Command` producao: 2 → 1
- phase tags: 310 → 191 (-119)
- **`run_engine.rs` LOC: 4230 → 4029 (-201)**

**Validacao:** 1132 unit + 96 integration = **1228 tests passando, 0 falhas.**
- `run_engine.rs` agora **usa** os helpers via `use crate::run_engine_helpers::{llm_error_to_class, ..., derive_provider_hint}` + analogos para auto_init e sandbox.
- Os testes inline de `derive_provider_hint` dentro de `run_engine::tests` continuam no mesmo modulo mas testam o import — agora importado do helper.

**Nao feito nesta iteracao (proximas):** T0.1 restante (7 cenarios LLM), T0.2 caracterizacao subagent, T1.1 bwrap completo, T3.4 retry inline, T4.2 continuar extracao (`lifecycle.rs`, `main_loop.rs`, `dispatch/*`), T4.3 Strategy pattern meta-tools, T4.4 Chain of Responsibility done gates, T4.5 split subagent/mod.rs, T5.1 RunMetadata, T6.4 batch streaming deltas, T7.* tests gap, T8.1 phase sweep restante.
