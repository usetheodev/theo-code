# ADR-014: Prefer manual `validate()` fns over `garde` (for now)

**Status:** Aceito
**Data:** 2026-04-23
**Autor:** Audit remediation (iteration 10)
**Escopo:** `theo-agent-runtime::project_config`, future Deserialize DTOs
**Fecha T6.3** do plano de remediação.

---

## Contexto

O plano de remediação inclui T6.3 "Validação declarativa (garde)":

> Introduzir `garde` para DTOs de entrada em theo-api-contracts e adapters HTTP.
> ≥ 80% dos DTOs externos validados via derive.

Levantamento (2026-04-23):

| Crate | DTO com `Deserialize` de fonte externa | Campos validáveis |
| --- | --- | --- |
| theo-api-contracts | `FrontendEvent` | 0 (campo principal é `serde_json::Value` arbitrário) |
| theo-application::observability_ui | — | 0 numéricos não-opcionais |
| theo-application::memory_ui | — | 0 numéricos |
| theo-application::graph_context_service | — | 0 numéricos |
| theo-agent-runtime::project_config | `ProjectConfig` | **~7** (`temperature`, `max_iterations`, `max_tokens`, etc.) |
| theo-infra-llm::routing::metrics | `RoutingCase` | 0 numéricos validáveis |

O corpus real de DTOs que se beneficiaria de validação declarativa é
essencialmente um: `ProjectConfig`. Adicionar `garde` + `garde-derive`
para uma única struct violaria **YAGNI** (§11) e **KISS** (§10) —
trocaríamos uma função de 20 linhas por uma dep transitiva com macros
proc.

## Decisão

**Adiar** a adoção de `garde`. No lugar, introduzir um helper manual
por struct que precise validar:

```rust
impl ProjectConfig {
    /// Return `Err` with a human-readable diagnostic if any field carries
    /// a value outside the accepted domain. Called from `Self::load`
    /// after the TOML deserialize step.
    pub fn validate(&self) -> Result<(), ConfigValidationError> { … }
}
```

### Por quê não garde agora

1. **Um único beneficiário.** T6.3 baseline: apenas `ProjectConfig` tem
   campos numéricos em faixa restrita. Trazer um crate + macro só
   para essa struct é over-engineering.
2. **`garde-derive`** expande a uma cadeia longa de macros — custo de
   build + compile-error verboso que não paga para uma única struct.
3. **Convertibilidade.** Se o corpus de DTOs validáveis passar de 1
   → N, migrar a função manual `validate()` para `#[derive(Garde)]`
   é uma refatoração mecânica (cada campo vira `#[garde(range(…))]`).
4. **Preserva `theo-domain` zero-dep.** `theo-domain` não recebe nada;
   a validação fica onde o DTO vive (theo-agent-runtime para
   `ProjectConfig`).

### Critérios para reabrir

Este ADR deve ser revisitado quando qualquer das condições abaixo for
verdade:

1. **≥ 5 DTOs** novos adotarem validação manual com padrões similares
   (range, length, regex).
2. Aparecer uma fronteira HTTP/gRPC que receba JSON tipado com campos
   numéricos restritos.
3. Uma regressão de input-validation chegar em produção por causa de
   um campo mal-validado manualmente.

## Implementação imediata

- `ProjectConfig::validate()` — ~30 LOC, testado com casos fronteiriços
  (`temperature < 0`, `temperature > 2`, `max_iterations == 0`,
  `max_tokens == 0`, `doom_loop_threshold >= max_iterations`).
- `ConfigValidationError` enum em `theo_agent_runtime::project_config`,
  mensagens específicas por campo.
- `ProjectConfig::load` chama `validate()` e degrada para defaults com
  `eprintln!("[theo] Warning: …")` em caso de violação (preserva o
  comportamento atual de "nunca derrubar a boot por arquivo
  mal-formado").

## Consequências

- **T6.3 fecha** como "manual validation adopted, garde deferred".
- ProjectConfig ganha validação de domínio imediatamente — usuários
  que configuram `temperature = -5.0` recebem um warning claro em
  vez de o sistema silenciosamente aceitar.
- O workspace continua sem `garde`/`garde-derive` e suas deps
  transitivas.
- Se a lista de DTOs crescer, este ADR é reaberto e a migração é
  tratada como uma issue específica.
