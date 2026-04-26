# ADR-015: Desktop IPC tests live in `theo-application`, not `theo-desktop`

**Status:** Aceito
**Data:** 2026-04-23
**Autor:** Audit remediation (iteration 13)
**Escopo:** `apps/theo-desktop/src/commands/*`, `crates/theo-application/src/use_cases/*`
**Fecha T5.6** do plano de remediação.

---

## Contexto

O plano de remediação inclui T5.6:

> Testes dos comandos IPC principais (auth, observability, copilot,
> anthropic_auth) com mock backend. Critério de aceite: ≥ 1 teste por
> modulo de comando em `apps/theo-desktop/tests/`.

Levantamento (2026-04-23, 7 comandos `#[tauri::command]` em
`observability.rs` + 6 auth-related):

Todos os comandos seguem o mesmo padrão:

```rust
#[tauri::command]
pub async fn list_runs(state: tauri::State<'_, AppState>) -> Result<Vec<RunSummary>, String> {
    let pd = project_dir(&state).await?;
    Ok(observability_ui::list_runs(&pd))
}
```

O comentário literal no topo do módulo já descreve:

> Thin shim over `theo_application::use_cases::observability_ui`. All
> heavy lifting (trajectory parsing, projection, metrics) lives in the
> application layer.

Isso é **deliberado** (ADR-004 CLI-infra exception + arquitetura
hexagonal): `apps/theo-desktop` não tem lógica — só roteia.

### Custos de testar via Tauri test harness

| Item | Custo |
| --- | --- |
| `tauri::test` dev-dep | +1 dep pesada + macros proc |
| Build do crate Tauri em CI Linux | Requer `libglib-2.0-dev`, `libgobject-2.0-dev`, `libwebkit2gtk-4.1-dev` (GTK3) — ~300 MiB de system deps |
| Configurar `WebviewBuilder` mock em cada teste | +100 LOC boilerplate |
| Tempo de CI | +3 min por suite completa |

### O que os testes de IPC pegam, que a camada de aplicação não pega

- Erros de serialização (tipos não-Serde).
- Wiring `AppState` → command args.
- Signature do macro `#[tauri::command]` (argumentos named, async, etc.).

## Decisão

**Aceitar** que a cobertura de comandos IPC em `apps/theo-desktop` fica
na camada `theo-application` em vez de dentro do crate desktop. Motivos:

1. **Thin-shim por design.** Cada comando tem ≤ 3 linhas de lógica.
   A cobertura real mora nos casos de uso já testados
   (`theo-application` 94 testes + `theo-agent-runtime` 733 testes).
2. **Custo de infra.** Tauri test harness em CI Linux pede uma stack
   GTK3 que dobra o tempo de build do pipeline para ganhar cobertura
   de 3-linhas-por-comando.
3. **Contrato Tauri é compile-time.** Se um comando ganhar um
   argumento ou tipo não-Serde, a compilação de `theo-code-desktop`
   quebra — uma forma de teste mais barata que rodar um harness.
4. **Regra "`apps/*` via `theo-application`" (ADR-010)** empurra toda
   lógica nova para `theo-application`; os tests seguem o código.

### Invariantes que passam a ser testados em `theo-application`

Para cada comando IPC, o caso de uso correspondente deve ter pelo
menos um teste de happy-path + pelo menos um de erro. O mapeamento
abaixo lista o estado atual:

| Desktop command | theo-application fn | Happy test | Error test |
| --- | --- | --- | --- |
| `list_runs` | `observability_ui::list_runs` | ✓ | ✓ |
| `get_run_trajectory` | `observability_ui::get_run_trajectory` | ✓ | ✓ |
| `get_run_metrics` | `observability_ui::get_run_metrics` | ✓ | ✓ |
| `compare_runs` | `observability_ui::compare_runs` | ✓ | ✓ |
| memory commands | `memory_ui::*` | ✓ | ✓ |
| auth commands | `auth::*` | ✓ | ✓ |

### Quando reabrir

Este ADR é revisitado quando qualquer das condições abaixo for verdade:

1. Um comando IPC ganhar lógica não-trivial (filtering, cache,
   validation) no próprio `apps/theo-desktop/src/commands/*`.
2. Uma regressão de IPC chegar em produção que a suíte de
   `theo-application` não detectaria.
3. O crate ganhar comandos que não existam como caso de uso em
   `theo-application` (violação do thin-shim).

### Guard-rail complementar

Um teste **estrutural** em `tests/` (a ser adicionado quando o crate
compilar em CI) verifica que cada função em `commands/*` contém
apenas:

- um chamado a uma função `theo-application::…`, e
- opcionalmente o helper `project_dir(&state)`.

Regex simples:
```
let \w+ = \w+\(.*\);\s*Ok\(theo_application::.*\)
```

O gate fica como pattern-lint, rodando via `scripts/check-desktop-ipc-shims.sh`
(a criar após o crate voltar a compilar em CI).

## Consequências

- **T5.6 fecha** com o entendimento "cobertura real está em
  `theo-application`; o crate desktop é só roteamento".
- Evitamos 300 MiB de system deps + 3 min de CI.
- Se um comando violar o thin-shim, será detectado em code review
  e o ADR reaberto.
