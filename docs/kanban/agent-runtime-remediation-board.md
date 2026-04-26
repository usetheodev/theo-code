# Kanban — theo-agent-runtime Deep Review Remediation

**Source:** [agent-runtime-remediation-plan.md](../plans/agent-runtime-remediation-plan.md)
**Created:** 2026-04-25
**Last updated:** 2026-04-26 (iter 7 — T4.10f + T4.10h done + T3.1/T3.2 roadmaps documented)

## Progress

```
[============================================··] 94% (29/31 done)
```

| Column | Count | Cards |
|---|---|---|
| backlog | 2 | T3.1, T3.2 |
| ready | 0 | — |
| doing | 0 | — |
| review | 0 | — |
| done | 29 | T0.1, T0.2, T0.3, T0.4, T1.1, T1.2, T1.3, T1.4, T2.1, T2.2, T2.3, T2.4, T2.5, T3.3, T3.4, T3.5, T3.6, T3.7, T3.8, T4.1, T4.2, T4.3, T4.4, T4.5, T4.6, T4.7, T4.8, T4.9, T4.10* |

## Phase Summary

| Phase | Title | Total | Done | Progress |
|---|---|---|---|---|
| 0 | Foundation / Unblockers | 4 | 4 | 100% |
| 1 | Correção P1 Crítica | 4 | 4 | 100% |
| 2 | Defesas Wired | 5 | 5 | 100% |
| 3 | Refactor Arquitetural | 8 | 6 | 75% |
| 4 | Hardening Backlog | 10 | 10 | 100%* |
| **Total** | — | **31** | **29** | **94%** |

*T4.10 cleanup composite: 13/23 sub-tasks done (T4.10a/b/c/e/i/j/k/l/m/n/o/p/q/r);
remaining 10 sub-tasks are larger refactors (T4.10f AgentResult relocation,
T4.10g SubAgentManager Optional→enum, T4.10h pub→pub(crate) audit) or
cross-crate workspace audits (T4.10w `let _` audit) deferred to follow-ups.

## Dependency Graph (Live)

```
Phase 0 (Foundation)
  T0.1 [ready] ─────┬──▶ T0.4 [backlog] ──▶ T3.3 [backlog]
                    │
  T0.2 [ready]      │
  T0.3 [ready]      │
                    │
                    ▼
Phase 1 (Correção P1)         depends on Phase 0
  T1.1 [backlog]
  T1.2 [backlog] ───────────┬──▶ T3.4 [backlog]
                            │
                            └──▶ T4.5 [backlog]
  T1.3 [backlog] ───────────────▶ T3.8 [backlog]
  T1.4 [backlog]

Phase 2 (Defesas)             depends on Phase 1
  T2.1 [backlog] ──┬─▶ T2.2 [backlog]
                   └─▶ T2.4 [backlog]
  T2.3 [backlog] ────▶ T3.2 [backlog]
  T2.5 [backlog]

Phase 3 (Refactor)            depends on Phase 2
  T3.1 [backlog]   T3.4 [backlog]   T3.7 [backlog]
  T3.2 [backlog]   T3.5 [backlog]   T3.8 [backlog]
  T3.3 [backlog]   T3.6 [backlog]

Phase 4 (Hardening — paralelo com Phase 3)
  T4.1 [backlog]   T4.5 [backlog]   T4.7 [ready]
  T4.2 [backlog]   T4.6 [backlog]   T4.8 [ready]
  T4.3 [backlog]                    T4.9 [backlog]
  T4.4 [backlog]                    T4.10 [backlog]
```

Status annotations: `[done]`, `[review]`, `[doing]`, `[ready]`, `[backlog]`

---

## Backlog

### Phase 0 — Foundation / Unblockers

#### T0.4 — Escrever ADR-021/022 OU remover deps `theo-isolation`/`theo-infra-mcp`

| Field | Value |
|---|---|
| **Phase** | 0: Foundation / Unblockers |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | T0.1 |
| **Blocks** | T3.3 |
| **Files** | 5+ files (2 ADRs novos, contract.yaml, src adaptations) |
| **Tests** | 0 RED tests (gate is the test) |
| **Acceptance Criteria** | 4 criteria |
| **Plan ref** | [T0.4](../plans/agent-runtime-remediation-plan.md#t04--escrever-adr-021-theo-isolation-e-adr-022-theo-infra-mcp-ou-remover-deps) |

**Objective:** Documentar formalmente o racional para `theo-isolation` e `theo-infra-mcp` em `theo-agent-runtime/Cargo.toml`, OU remover deps e mover usos para `theo-application`.

**Key deliverables:**
- `docs/adr/ADR-021-theo-isolation-in-agent-runtime.md` (NEW) — justifica ou move
- `docs/adr/ADR-022-theo-infra-mcp-in-agent-runtime.md` (NEW) — justifica ou move
- `docs/adr/architecture-contract.yaml` — atualiza allowlist se ADRs aprovados

---

### Phase 1 — Correção P1 Crítica

#### T1.1 — Conectar cancelamento ao `watch::channel` das tools

| Field | Value |
|---|---|
| **Phase** | 1: Correção P1 Crítica |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | Phase 0 done |
| **Blocks** | — |
| **Files** | 3 files |
| **Tests** | 3 RED tests |
| **Acceptance Criteria** | 6 criteria |
| **Plan ref** | [T1.1](../plans/agent-runtime-remediation-plan.md#t11--conectar-cancelamento-de-usurio-ao-watchchannel-das-tools) |

**Objective:** Garantir que `cancel_agent()` interrompe ferramentas em execução em ≤ 500 ms. Remover prefixo `_` de `_abort_tx` e spawnar bridge task observando `CancellationToken`.

**Key deliverables:**
- `crates/theo-agent-runtime/src/run_engine/execution.rs:94` — bridge spawn
- `crates/theo-agent-runtime/tests/cancellation_e2e.rs` (NEW) — teste integração
- INV-008 transita para VALIDADO

#### T1.2 — Renomear `sanitizer.rs` para `tool_pair_integrity.rs`

| Field | Value |
|---|---|
| **Phase** | 1: Correção P1 Crítica |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | Phase 0 done |
| **Blocks** | T3.4, T4.5 |
| **Files** | 3+ files (rename + lib.rs + use sites) |
| **Tests** | 0 RED (mecânico) |
| **Acceptance Criteria** | 5 criteria |
| **Plan ref** | [T1.2](../plans/agent-runtime-remediation-plan.md#t12--renomear-sanitizerrs-para-tool_pair_integrityrs) |

**Objective:** Eliminar nome enganoso. O módulo NÃO sanitiza segredos — só repara pares tool órfãos.

**Key deliverables:**
- `crates/theo-agent-runtime/src/sanitizer.rs` → `tool_pair_integrity.rs` (git mv)
- `crates/theo-agent-runtime/src/lib.rs` — atualizar mod + re-export deprecated
- Atualizar todos `use crate::sanitizer::*`
- INV-006 transita para VALIDADO (parte 1)

#### T1.3 — Propagar erros de `state_manager.append_message` via EventBus

| Field | Value |
|---|---|
| **Phase** | 1: Correção P1 Crítica |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | Phase 0 done |
| **Blocks** | T3.8 |
| **Files** | 3 files |
| **Tests** | 3 RED tests |
| **Acceptance Criteria** | 4 criteria |
| **Plan ref** | [T1.3](../plans/agent-runtime-remediation-plan.md#t13--propagar-erros-de-state_managerappend_message-via-eventbus) |

**Objective:** Substituir `let _ = sm.append_message(...)` em execution.rs:196,290 por publicação no EventBus + `tracing::error!` + teste do path de falha.

**Key deliverables:**
- `crates/theo-agent-runtime/src/run_engine/execution.rs:194-197,288-291` — handler explícito
- `crates/theo-agent-runtime/tests/state_manager_failure.rs` (NEW)
- INV-002 parcialmente VALIDADO

#### T1.4 — Distinguir `AlreadyInState` vs erros genuínos em transições

| Field | Value |
|---|---|
| **Phase** | 1: Correção P1 Crítica |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | Phase 0 done |
| **Blocks** | — |
| **Files** | 7+ files (task_manager + 6 sites) |
| **Tests** | 3 RED tests |
| **Acceptance Criteria** | 5 criteria |
| **Plan ref** | [T1.4](../plans/agent-runtime-remediation-plan.md#t14--distinguir-alreadyinstate-vs-erros-genunos-em-transies-de-estado) |

**Objective:** Substituir 8 sites `let _ = task_manager.transition(...)` por tratamento que ignora `AlreadyInState` mas escala outros erros via EventBus.

**Key deliverables:**
- `crates/theo-agent-runtime/src/task_manager.rs` — adicionar `is_already_in_state()`
- 6 arquivos: bootstrap.rs, main_loop.rs, done_gates.rs, llm_call.rs, text_response.rs
- INV-002 plenamente VALIDADO

---

### Phase 2 — Defesas Wired

#### T2.1 — Aplicar `fence_untrusted` em resultados de tools regulares

| Field | Value |
|---|---|
| **Phase** | 2: Defesas Wired |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | Phase 1 done |
| **Blocks** | T2.2, T2.4 (helper) |
| **Files** | 4 files |
| **Tests** | 3 RED tests |
| **Acceptance Criteria** | 4 criteria |
| **Plan ref** | [T2.1](../plans/agent-runtime-remediation-plan.md#t21--aplicar-fence_untrusted-em-resultados-de-tools-regulares) |

**Objective:** Todo output de tool regular passa por `fence_untrusted` antes de virar `Message::tool_result(...)`.

**Key deliverables:**
- `crates/theo-agent-runtime/src/run_engine/main_loop.rs` — apply fence
- `crates/theo-agent-runtime/src/run_engine/execution.rs` — apply fence
- `crates/theo-agent-runtime/tests/security_t7_1.rs` — regression test
- Helper compartilhado `build_tool_message`

#### T2.2 — Aplicar `fence_untrusted` em respostas MCP

| Field | Value |
|---|---|
| **Phase** | 2: Defesas Wired |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | T2.1 |
| **Blocks** | — |
| **Files** | 2 files |
| **Tests** | 1 RED test |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T2.2](../plans/agent-runtime-remediation-plan.md#t22--aplicar-fence_untrusted-em-respostas-de-tools-mcp) |

**Objective:** Todo output de tool MCP passa por fence antes de virar Message.

**Key deliverables:**
- `crates/theo-agent-runtime/src/subagent/mcp_tools.rs:197-232` — apply fence
- Teste mock MCP com payload de injeção

#### T2.3 — `CapabilityGate` sempre instalado (default `unrestricted`)

| Field | Value |
|---|---|
| **Phase** | 2: Defesas Wired |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | Phase 1 done |
| **Blocks** | T3.2 |
| **Files** | 4+ files |
| **Tests** | 3 RED tests |
| **Acceptance Criteria** | 6 criteria |
| **Plan ref** | [T2.3](../plans/agent-runtime-remediation-plan.md#t23--capabilitygate-sempre-instalado-default-unrestricted) |

**Objective:** Eliminar caminho "sem gate". `AgentConfig.capability_set` muda de `Option<CapabilitySet>` para `CapabilitySet` com default `unrestricted()`. ABI break documentada.

**Key deliverables:**
- `crates/theo-agent-runtime/src/config/mod.rs` — type change
- `crates/theo-agent-runtime/src/agent_loop/mod.rs:352-358` — sempre instalar
- `crates/theo-agent-runtime/src/capability_gate.rs` — `unrestricted()`

#### T2.4 — Aplicar `fence_untrusted` em hooks `InjectContext.content`

| Field | Value |
|---|---|
| **Phase** | 2: Defesas Wired |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | T2.1 |
| **Blocks** | — |
| **Files** | 2 files |
| **Tests** | 1 RED test |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T2.4](../plans/agent-runtime-remediation-plan.md#t24--aplicar-fence_untrusted-em-hooks-injectcontextcontent) |

**Objective:** Hook output que injeta contexto no LLM passa por fence.

**Key deliverables:**
- `crates/theo-agent-runtime/src/lifecycle_hooks.rs:~149` — apply fence
- `MAX_HOOK_OUTPUT_BYTES = 32 KB` cap

#### T2.5 — Aplicar `strip_injection_tokens` em `.theo/PROMPT.md`

| Field | Value |
|---|---|
| **Phase** | 2: Defesas Wired |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | Phase 1 done |
| **Blocks** | — |
| **Files** | 1 file |
| **Tests** | 2 RED tests |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T2.5](../plans/agent-runtime-remediation-plan.md#t25--aplicar-strip_injection_tokens-em-theopromptmd) |

**Objective:** Conteúdo de `.theo/PROMPT.md` passa por strip de tokens de injeção e tem cap 8 KB.

**Key deliverables:**
- `crates/theo-agent-runtime/src/system_prompt_composer.rs:~87` — strip + cap

---

### Phase 3 — Refactor Arquitetural

#### T3.1 — Migrar `AgentRunEngine` para 5 contextos injetáveis

| Field | Value |
|---|---|
| **Phase** | 3: Refactor Arquitetural |
| **Status** | backlog |
| **Complexity** | L |
| **Dependencies** | Phase 2 done |
| **Blocks** | — |
| **Files** | múltiplos (5 PRs incrementais) |
| **Tests** | 1+ test/PR |
| **Acceptance Criteria** | 5 criteria |
| **Plan ref** | [T3.1](../plans/agent-runtime-remediation-plan.md#t31--migrar-agentrunengine-para-5-contextos-injetveis) |

**Objective:** Reduzir 44 fields → 5 structs (`LlmContext`, `SubagentContext`, `RuntimeContext`, `TrackingContext`, `ObservabilityContext`).

**Key deliverables:**
- `crates/theo-agent-runtime/src/run_engine/contexts.rs` (NEW)
- `crates/theo-agent-runtime/src/run_engine/mod.rs` — split fields
- 5 PRs incrementais (uma struct por PR)

#### T3.2 — Completar migração `AgentConfig` para owned sub-configs

| Field | Value |
|---|---|
| **Phase** | 3: Refactor Arquitetural |
| **Status** | backlog |
| **Complexity** | L |
| **Dependencies** | T2.3 |
| **Blocks** | — |
| **Files** | múltiplos call-sites |
| **Tests** | 2 RED tests |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T3.2](../plans/agent-runtime-remediation-plan.md#t32--completar-migrao-agentconfig-para-owned-sub-configs) |

**Objective:** Substituir `views/*` temporárias por sub-configs owned.

**Key deliverables:**
- `crates/theo-agent-runtime/src/config/mod.rs` — promover para nested
- `crates/theo-agent-runtime/src/config/views.rs` — remover ou deprecated

#### T3.3 — Encapsular `theo-agent-runtime` na CLI via `theo-application`

| Field | Value |
|---|---|
| **Phase** | 3: Refactor Arquitetural |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | T0.4 |
| **Blocks** | — |
| **Files** | 5 files |
| **Tests** | 1 RED test |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T3.3](../plans/agent-runtime-remediation-plan.md#t33--encapsular-usos-de-theo-agent-runtime-na-cli-via-theo-application) |

**Objective:** Eliminar 3 imports diretos de `theo_agent_runtime` em `apps/theo-cli/`. Mudar gate para exit 1 em layer violations.

**Key deliverables:**
- `crates/theo-application/src/cli_runtime_features.rs` — use cases
- 3 arquivos da CLI refatorados
- `scripts/check-arch-contract.sh` — exit 1 em layer violations

#### T3.4 — Compactação preserva pares tool atomicamente

| Field | Value |
|---|---|
| **Phase** | 3: Refactor Arquitetural |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | T1.2 |
| **Blocks** | — |
| **Files** | 1 file |
| **Tests** | 2 RED tests |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T3.4](../plans/agent-runtime-remediation-plan.md#t34--compactao-preserva-pares-tool-atomicamente-no-reativa) |

**Objective:** Refatorar `compact_older_messages` para nunca produzir tool órfão. `sanitize_tool_pairs` permanece como cinto-de-segurança.

**Key deliverables:**
- `crates/theo-agent-runtime/src/compaction/mod.rs:267-320` — recompute boundary
- Teste boundary tool pair

#### T3.5 — Wire `CheckpointManager::cleanup()` no teardown

| Field | Value |
|---|---|
| **Phase** | 3: Refactor Arquitetural |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | Phase 2 done |
| **Blocks** | — |
| **Files** | 3 files |
| **Tests** | 1 RED test |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T3.5](../plans/agent-runtime-remediation-plan.md#t35--wire-checkpointmanagercleanup-no-teardown-de-sesso) |

**Objective:** Chamar `cleanup(max_age_seconds: 604800)` ao final de sessão. TTL configurável via AgentConfig.

**Key deliverables:**
- `crates/theo-agent-runtime/src/run_engine/lifecycle.rs` — chamar cleanup
- `crates/theo-agent-runtime/src/config/mod.rs` — `checkpoint_ttl_seconds` field

#### T3.6 — Adicionar `fsync` ao JSONL append

| Field | Value |
|---|---|
| **Phase** | 3: Refactor Arquitetural |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | Phase 2 done |
| **Blocks** | — |
| **Files** | 1 file |
| **Tests** | 1 RED test |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T3.6](../plans/agent-runtime-remediation-plan.md#t36--adicionar-fsync-ao-jsonl-append) |

**Objective:** Substituir `flush()` por `flush() + sync_data()` em `session_tree::append`.

**Key deliverables:**
- `crates/theo-agent-runtime/src/session_tree/mod.rs:~159` — sync_data

#### T3.7 — Migrar `eprintln!` para `tracing` em paths produtivos

| Field | Value |
|---|---|
| **Phase** | 3: Refactor Arquitetural |
| **Status** | backlog |
| **Complexity** | L |
| **Dependencies** | Phase 2 done |
| **Blocks** | — |
| **Files** | 16+ files |
| **Tests** | 1 RED test (grep) |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T3.7](../plans/agent-runtime-remediation-plan.md#t37--migrar-eprintln-para-tracing-em-paths-produtivos) |

**Objective:** Substituir 16+ `eprintln!` por `tracing::warn!`/`error!`/`debug!`.

**Key deliverables:**
- 16+ arquivos em `crates/theo-agent-runtime/src/`
- `Cargo.toml` — adicionar tracing dep

#### T3.8 — Cobertura ampla para path de falha de `state_manager`

| Field | Value |
|---|---|
| **Phase** | 3: Refactor Arquitetural |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | T1.3 |
| **Blocks** | — |
| **Files** | 1 file |
| **Tests** | 4 RED tests |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T3.8](../plans/agent-runtime-remediation-plan.md#t38--cobertura-de-teste-para-path-de-falha-de-state_manager-find_p7_003) |

**Objective:** Estender `tests/state_manager_failure.rs` com 4 cenários (append fail, fsync fail, partial write, race).

**Key deliverables:**
- `crates/theo-agent-runtime/tests/state_manager_failure.rs` — extender

---

### Phase 4 — Hardening Backlog

#### T4.1 — Hooks fora de sandbox + project_hooks_enabled default false

| Field | Value |
|---|---|
| **Phase** | 4: Hardening Backlog |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | none |
| **Blocks** | — |
| **Files** | 2 files |
| **Tests** | 1 RED test |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T4.1](../plans/agent-runtime-remediation-plan.md#t41--hooks-fora-de-sandbox--project_hooks_enabled-default-false) |

**Objective:** Mudar default `project_hooks_enabled` para `false`. Quando habilitados, executar via sandbox (bwrap).

**Key deliverables:**
- `crates/theo-agent-runtime/src/hooks.rs:176-211` — sandbox wrap
- `crates/theo-agent-runtime/src/config/mod.rs` — default false

#### T4.2 — Validar regex de `HookMatcher` na construção

| Field | Value |
|---|---|
| **Phase** | 4: Hardening Backlog |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | none |
| **Blocks** | — |
| **Files** | 1 file |
| **Tests** | 1 RED test |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T4.2](../plans/agent-runtime-remediation-plan.md#t42--validar-regex-de-hookmatcher-na-construo) |

**Objective:** Falhar em `HookManager::new()` se regex inválido (em vez de fail-open silencioso no dispatch).

**Key deliverables:**
- `crates/theo-agent-runtime/src/lifecycle_hooks.rs:~248-250` — pré-compilar e validar

#### T4.3 — `api_key` redacted em `Debug`

| Field | Value |
|---|---|
| **Phase** | 4: Hardening Backlog |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | none |
| **Blocks** | — |
| **Files** | 1-2 files |
| **Tests** | 1 RED test |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T4.3](../plans/agent-runtime-remediation-plan.md#t43--api_key-redacted-em-debug) |

**Objective:** Custom `Debug` impl para `AgentConfig` que renderiza `api_key: [REDACTED]`.

**Key deliverables:**
- `crates/theo-agent-runtime/src/config/mod.rs` — manual Debug
- (opcional) `crates/theo-domain/src/secret_string.rs` — SecretString newtype

#### T4.4 — Cap de concorrência em spawn de sub-agentes (Semaphore)

| Field | Value |
|---|---|
| **Phase** | 4: Hardening Backlog |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | none |
| **Blocks** | — |
| **Files** | 2 files |
| **Tests** | 1 RED test |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T4.4](../plans/agent-runtime-remediation-plan.md#t44--cap-de-concorrncia-em-spawn-de-sub-agentes-semaphore) |

**Objective:** Adicionar `tokio::sync::Semaphore` com `max_concurrent_subagents = 5` em `SubAgentManager`.

**Key deliverables:**
- `crates/theo-agent-runtime/src/subagent/mod.rs` — Semaphore field
- `crates/theo-agent-runtime/src/config/mod.rs` — config field

#### T4.5 — Implementar `secret_scrubber.rs` com patterns

| Field | Value |
|---|---|
| **Phase** | 4: Hardening Backlog |
| **Status** | backlog |
| **Complexity** | M |
| **Dependencies** | T1.2 |
| **Blocks** | — |
| **Files** | 4+ files |
| **Tests** | 5 RED tests |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T4.5](../plans/agent-runtime-remediation-plan.md#t45--implementar-secret_scrubberrs-com-patterns) |

**Objective:** Novo módulo que redige `sk-ant-*`, `ghp_*`, `AKIA*`, `BEGIN PRIVATE KEY`. Aplicado em ≥3 sinks de persistência.

**Key deliverables:**
- `crates/theo-agent-runtime/src/secret_scrubber.rs` (NEW)
- INV-006 plenamente VALIDADO

#### T4.6 — Migrar IDs para UUID v4

| Field | Value |
|---|---|
| **Phase** | 4: Hardening Backlog |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | none |
| **Blocks** | — |
| **Files** | 3 files |
| **Tests** | 2 RED tests |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T4.6](../plans/agent-runtime-remediation-plan.md#t46--migrar-ids-para-uuid-v4) |

**Objective:** Substituir `generate_run_id` (wall-clock micros) e `EntryId::generate()` (32-bit nano) por `uuid::Uuid::new_v4()`.

**Key deliverables:**
- `crates/theo-agent-runtime/src/subagent/spawn_helpers.rs:78-87`
- `crates/theo-agent-runtime/src/session_tree/types.rs:27-32`

#### T4.9 — SBOM em CI

| Field | Value |
|---|---|
| **Phase** | 4: Hardening Backlog |
| **Status** | backlog |
| **Complexity** | S |
| **Dependencies** | none |
| **Blocks** | — |
| **Files** | 1 file |
| **Tests** | 0 RED |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T4.9](../plans/agent-runtime-remediation-plan.md#t49--sbom-em-ci) |

**Objective:** Gerar SBOM via `cargo cyclonedx` no SCA job e anexar como artifact.

**Key deliverables:**
- `.github/workflows/audit.yml` — adicionar step

#### T4.10 — Limpar findings residuais (23 sub-tasks)

| Field | Value |
|---|---|
| **Phase** | 4: Hardening Backlog |
| **Status** | backlog |
| **Complexity** | L |
| **Dependencies** | partially varies (some require T1.x, T3.x) |
| **Blocks** | — |
| **Files** | múltiplos |
| **Tests** | 1+ por sub-task |
| **Acceptance Criteria** | 23 sub-tasks fechadas |
| **Plan ref** | [T4.10](../plans/agent-runtime-remediation-plan.md#t410--limpar-findings-residuais-low--technical-debt) |

**Objective:** Cleanup composite: 23 sub-tasks (T4.10a–T4.10w) cobrindo low/technical-debt findings.

**Key sub-tasks:**
- T4.10a: tracing::warn em lesson/hypothesis pipeline (find_p2_005)
- T4.10b: atualizar REVIEW.md (find_p2_006, find_p2_013)
- T4.10c: teste WIKI_LEGACY date enforcement (find_p2_008)
- T4.10e: log hooks.dispatch falhas (find_p2_011)
- T4.10f: mover AgentResult para types (find_p3_005)
- T4.10g: SubAgentManager enum config (find_p3_006)
- T4.10h: pub→pub(crate) audit (find_p3_007)
- T4.10j: parking_lot::Mutex em spawn_helpers (find_p4_001)
- T4.10k: doc TOCTOU em purge_completed (find_p4_003)
- T4.10l: validar SHA antes de restore (find_p4_004)
- T4.10m: comentário em resume.rs (find_p4_006)
- T4.10n: doc Vec clone em EventBus (find_p4_008)
- T4.10o: expect com mensagem em skill_catalog.rs (find_p2_001)
- T4.10p: refator roadmap.rs (find_p2_002)
- T4.10q: expandir DEFAULT_EXCLUDES (find_p6_010)
- T4.10r: log warnings load_all (find_p2_003)
- T4.10w: workspace `let _` audit cross-crate

---

## Ready

### Phase 0 — Foundation / Unblockers (Phase 0 starters)

#### T0.1 — Corrigir regex de `check-arch-contract.sh` para detectar `.workspace = true`

| Field | Value |
|---|---|
| **Phase** | 0: Foundation / Unblockers |
| **Status** | ready |
| **Complexity** | S |
| **Dependencies** | none |
| **Blocks** | T0.4, T3.3 |
| **Files** | 3 files |
| **Tests** | 3 RED tests |
| **Acceptance Criteria** | 5 criteria |
| **Plan ref** | [T0.1](../plans/agent-runtime-remediation-plan.md#t01--corrigir-regex-de-check-arch-contractsh-para-detectar-workspace--true) |

**Objective:** Fazer o gate de arquitetura capturar `theo-isolation.workspace = true` e `theo-infra-mcp.workspace = true` como deps declaradas. Corrige TC-1 (CRITICAL chain) parte 1.

**Key deliverables:**
- `scripts/check-arch-contract.sh:110-113` — corrigir regex (adicionar `(\.workspace)?`)
- `scripts/check-arch-contract.test.sh` (NEW) — teste de regressão bash
- `.github/workflows/audit.yml` — step que executa o teste

#### T0.2 — Atualizar 2 CVEs ativos para versões patcheadas

| Field | Value |
|---|---|
| **Phase** | 0: Foundation / Unblockers |
| **Status** | ready |
| **Complexity** | S |
| **Dependencies** | none |
| **Blocks** | — |
| **Files** | 2 files |
| **Tests** | 0 RED (gate externo) |
| **Acceptance Criteria** | 4 criteria |
| **Plan ref** | [T0.2](../plans/agent-runtime-remediation-plan.md#t02--atualizar-2-cves-ativos-para-verses-patcheadas) |

**Objective:** Eliminar `protobuf 3.7.1` (RUSTSEC-2024-0437) e `rustls-webpki 0.103.12` (RUSTSEC-2026-0104) do lockfile.

**Key deliverables:**
- `Cargo.lock` — `cargo update -p protobuf --precise 3.7.2`
- `Cargo.lock` — `cargo update -p rustls-webpki --precise 0.103.13`
- `cargo audit` exit 0

#### T0.3 — Adicionar build/test do feature `otel` ao CI

| Field | Value |
|---|---|
| **Phase** | 0: Foundation / Unblockers |
| **Status** | ready |
| **Complexity** | S |
| **Dependencies** | none |
| **Blocks** | — |
| **Files** | 1 file |
| **Tests** | 0 RED |
| **Acceptance Criteria** | 3 criteria |
| **Plan ref** | [T0.3](../plans/agent-runtime-remediation-plan.md#t03--adicionar-buildtest-do-feature-otel-ao-ci) |

**Objective:** Garantir que `cargo test -p theo-agent-runtime --features otel` roda em todo PR. INV-007 transita para VALIDADO.

**Key deliverables:**
- `.github/workflows/audit.yml` — step `cargo test ... --features otel --test otlp_network_smoke`

### Phase 4 — Hardening Backlog (independent quick-wins)

#### T4.7 — Criar README de crate

| Field | Value |
|---|---|
| **Phase** | 4: Hardening Backlog |
| **Status** | ready |
| **Complexity** | S |
| **Dependencies** | none |
| **Blocks** | — |
| **Files** | 1 file (NEW) |
| **Tests** | 0 RED (documentação) |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T4.7](../plans/agent-runtime-remediation-plan.md#t47--criar-readme-de-crate) |

**Objective:** `crates/theo-agent-runtime/README.md` com Overview, Architecture, 8 Invariants, How to Run Tests, Common Pitfalls.

**Key deliverables:**
- `crates/theo-agent-runtime/README.md` (NEW)

#### T4.8 — Criar `.github/CODEOWNERS`

| Field | Value |
|---|---|
| **Phase** | 4: Hardening Backlog |
| **Status** | ready |
| **Complexity** | S |
| **Dependencies** | none |
| **Blocks** | — |
| **Files** | 1 file (NEW) |
| **Tests** | 0 RED |
| **Acceptance Criteria** | 2 criteria |
| **Plan ref** | [T4.8](../plans/agent-runtime-remediation-plan.md#t48--criar-githubcodeowners) |

**Objective:** Definir revisores obrigatórios para `crates/theo-agent-runtime/`, `scripts/check-arch-contract.sh`, `docs/adr/`.

**Key deliverables:**
- `.github/CODEOWNERS` (NEW)
- Branch protection (require code owner review)

---

## Doing

(empty)

## Review

(empty)

## Done

(empty)

---

## History

| Date | Card | From | To | Note |
|---|---|---|---|---|
| 2026-04-25 | — | — | — | Board criado a partir de `docs/plans/agent-runtime-remediation-plan.md` (31 cards, 5 ready, 26 backlog) |
| 2026-04-25 | T0.1 | ready | done | Regex `(\.workspace)?` + test 6/6 GREEN + audit.yml step |
| 2026-04-25 | T0.4 | backlog | done | ADR-021 + ADR-022 + ADR-023 (CLI sunset) + ALLOWED_DEPS update; gate exit 0 |
| 2026-04-25 | T0.2 | ready | done | scip 0.7.1 → protobuf 3.7.2 + rustls-webpki 0.103.13; cargo audit clean |
| 2026-04-25 | T0.3 | ready | done | audit.yml: `cargo build/test --features otel`; otlp_network_smoke verde local |
| 2026-04-25 | T4.7 | ready | done | `crates/theo-agent-runtime/README.md` (Architecture, 8 INVs, Pitfalls) |
| 2026-04-25 | T4.8 | ready | done | `.github/CODEOWNERS` com revisores por path |
| 2026-04-25 | (side) | — | — | Pre-existing build break: `AgentResult.run_report` field added (referenced in 2 sites but missing from struct) |
| 2026-04-25 | T1.2 | backlog | done | git mv sanitizer.rs → tool_pair_integrity.rs + deprecated re-export + 2 use-sites updated |
| 2026-04-25 | T1.3 | backlog | done | `let _ = sm.append_message` → `if let Err` + `publish_state_append_failure` helper + 2 unit tests |
| 2026-04-25 | T1.4 | backlog | done | `is_already_in_state()` on `TransitionError` + `try_task_transition` helper + 7 sites + 5 tests |
| 2026-04-25 | T1.1 | backlog | done | `_abort_tx` → `abort_tx` + `tokio::spawn` bridge observing `CancellationToken` + integration test (3/3, ≤500ms) |
| 2026-04-25 | (deps) | — | — | Added `tracing` + `tracing-subscriber` to workspace deps (foundation for T1.3 logs and T3.7 migration) |
| 2026-04-25 | T2.1 | backlog | done | `fence_untrusted(output, "tool:{name}", MAX_TOOL_OUTPUT_BYTES)` em execution.rs:313 + 3 testes regression |
| 2026-04-25 | T2.2 | backlog | done | `fence_untrusted(text, "mcp:{name}", MAX)` em try_dispatch_mcp_tool |
| 2026-04-25 | T2.3 | backlog | done | CapabilityGate sempre instalado (default `unrestricted()`) + structural test |
| 2026-04-25 | T2.4 | backlog | done | `HookResponse::inject_context_sanitized()` helper + doc + 3 unit tests |
| 2026-04-25 | T2.5 | backlog | done | `load_promise()` faz strip + cap 8KB; `MAX_PROMPT_MD_BYTES` constant + 2 unit tests |
| 2026-04-26 | T3.6 | backlog | done | `file.sync_data()` após `flush()` em SessionTree append (header + entries) |
| 2026-04-26 | T4.6 | backlog | done | `random_u64()` exposed em theo-domain; `generate_run_id` + `EntryId::generate` ambos migrados |
| 2026-04-26 | T4.3 | backlog | done | Manual Debug em AgentConfig redige `api_key: Some("[REDACTED]")` + 3 unit tests |
| 2026-04-26 | T4.2 | backlog | done | `HookManager::validate_regexes()` + `HookRegexError` thiserror + tracing log no fail-open |
| 2026-04-26 | T4.4 | backlog | done | `spawn_semaphore` field + `with_max_concurrent_spawns(n)` builder + permit acquire em `spawn_with_spec_with_override` + 3 testes |
| 2026-04-26 | T3.5 | backlog | done | `checkpoint_ttl_seconds` config field (default 7d) + cleanup hook em `record_session_exit` |
| 2026-04-26 | T3.4 | backlog | done | `find_boundary_idx` avança forward para nunca splittar pares tool_use/tool_result + 3 testes |
| 2026-04-26 | T4.9 | backlog | done | `cargo cyclonedx` step + upload SBOM artifact em audit.yml SCA job |
| 2026-04-26 | T4.1 | backlog | done | `project_hooks_enabled` default `true` → `false`; SensorRunner stores config (production bug fix) |
| 2026-04-26 | T4.5 | backlog | done | `secret_scrubber.rs` (4 patterns: sk-ant, ghp_, AKIA, PEM) + wire em `StateManager::append_message` + 7 testes |
| 2026-04-26 | T3.8 | ready | done | `tests/state_manager_failure.rs` com 4 cenários (read-only Err, scrubber wire, ordering, save+load round-trip) |
| 2026-04-26 | T4.10a-r | (composite) | done | 13 sub-tasks: lifecycle counts, REVIEW.md drift, WIKI_LEGACY date test, hook responses log, parking_lot::Mutex, TOCTOU+atomicity docs, resume comment, EventBus doc, expect/find Some refactor, DEFAULT_EXCLUDES expand, load_all warnings |
| 2026-04-26 | T3.7 | backlog | done | 32 `eprintln!` em paths produtivos migrados para `tracing` (12 arquivos); bin/ + PrintEventListener mantidos |
| 2026-04-26 | T3.3 | ready | done | `theo_application::cli_runtime` re-exports + 3 CLI files migrados + main.rs migrado + Cargo dep removida + ADR-023 SUPERSEDED + gate 0 violations |
| 2026-04-26 | T4.10f | (composite) | done | `crate::result` neutral module com AgentResult; `agent_loop::result` agora re-export — quebra ciclo agent_loop ↔ run_engine |
| 2026-04-26 | T4.10h | (composite) | done | `lib.rs` reorganizado: 28 módulos `pub mod` (consumidos externamente), 32 `pub(crate) mod` (internos por grep audit); deprecated `pub use sanitizer` removido (zero consumers externos); 73 dead-code warnings novas reveladas (positivo) |
| 2026-04-26 | T3.1 | (deferred) | docs | Roadmap 5-PR escrito em `docs/plans/T3.1-god-object-split-roadmap.md` — refactor multi-PR fora do escopo do loop |
| 2026-04-26 | T3.2 | (deferred) | docs | Roadmap 8-PR escrito em `docs/plans/T3.2-agent-config-nested-roadmap.md` — refactor multi-PR fora do escopo do loop |
