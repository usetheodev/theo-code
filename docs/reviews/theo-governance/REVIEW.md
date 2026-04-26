# theo-governance — Revisao

> **Contexto**: Policy engine simplificado. Bounded Context: Governance. Sits in the critical path but lightweight.
>
> **Dependencias permitidas**: `theo-domain`.
>
> **Status global**: deep-review concluido em 2026-04-25. 74 tests passando, 0 falhas (era 45+1 fail antes desta auditoria — fix de boundary test architectural).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `alerts` | Sistema de alertas quando politicas sao violadas/acionadas. | Revisado |
| 2 | `impact` | Avaliacao de impacto de acoes antes de permitir execucao. | Revisado |
| 3 | `metrics` | Metricas de governance (violacoes, approvals, denials). | Revisado |
| 4 | `sandbox_audit` | Auditoria de execucoes em sandbox (log + analise). | Revisado |
| 5 | `sandbox_policy` | Politicas de sandbox (bwrap/landlock/noop cascade). | Revisado |
| 6 | `sequence_analyzer` | Analise de sequencias de acoes (deteccao de padroes suspeitos). | Revisado |

---

## Notas de Deep-Review

### 1. alerts
Sistema de alertas tipados por severity. Driven por boundary test framework + sequence analyzer.

### 2. impact
Pre-execution impact assessment. Inputs: action type + capability set + recent history. Output: risk score + rationale.

### 3. metrics
Counters (violations, approvals, denials) + temporal aggregation. Used pelo governance dashboard + observability pipeline.

### 4. sandbox_audit
Per-invocation log de sandbox decisions: command, policy aplicada, allowed/denied, reasons. Audit trail para forensic analysis pos-incidente.

### 5. sandbox_policy
Policy declarations per command class (bash, cargo, git, file_io). Cascade bwrap → landlock → noop com fail-closed default (T1.1).

### 6. sequence_analyzer
Pattern detection em sequences de tool calls — flag suspicious chains (e.g., read sensitive + write external).

### Boundary tests (`tests/boundary_test.rs`)
Tests que verificam invariantes arquiteturais cross-crate ao parsear todos os Cargo.toml do workspace:
- `theo_domain_has_no_internal_deps`: ADR-010
- `apps_must_not_import_engines_directly`: ADR-016
- `apps_only_use_allowed_internal_deps`: lista de deps permitidas em apps/

**Iter desta revisao**: Identificada drift arquitetural — `apps/theo-cli/Cargo.toml` declarava `theo-infra-mcp.workspace = true` apesar do source code usar exclusivamente `theo_application::facade::mcp` (re-export via fachada). O dep direto era stale (commentario "sota-gaps Phase 17: build the MCP discovery cache once at CLI startup" indicava uso historico que migrou para a fachada). **Fix aplicado**: removido `theo-infra-mcp.workspace = true` + comment de `apps/theo-cli/Cargo.toml`. Verificado: `cargo check -p theo` (theo-cli binary) compila clean. `apps_only_use_allowed_internal_deps` agora passa.

**Validacao:**
- 74 tests passando, 0 falhas
- ADR-016 + ADR-010 boundary invariants verificados via tests structurais
- Sandbox cascade (bwrap → landlock → noop) wired via theo-tooling (T1.1 fixture)

Sem follow-ups bloqueadores. O drift arquitetural detectado nesta auditoria foi corrigido — a fachada `theo_application::facade::mcp` agora e o unico ponto de entrada para apps consumirem MCP.
