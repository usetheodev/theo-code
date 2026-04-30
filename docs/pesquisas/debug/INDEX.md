# Debug (DAP) — Pesquisa SOTA

## Escopo
11 debug tools via DAP (Debug Adapter Protocol): launch, breakpoint, continue, step, eval, stack_trace, scopes, variables, threads, status, terminate. E2E untested — Gap 6.1 CRITICAL.

## Crates alvo
- `theo-tooling` — 11 debug_* tools registered but no smoke test

## Referências-chave
| Fonte | O que extrair |
|-------|---------------|
| DAP spec (Microsoft) | Protocol compliance, message types |
| lldb-vscode | Rust/C++ debugging adapter |
| debugpy | Python debugging adapter |
| dlv (delve) | Go debugging adapter |

## Arquivos nesta pasta
- (pesquisas sobre DAP integration vão aqui)

## Gaps para pesquisar
- E2E smoke test against real debugger (Gap 6.1 CRITICAL in maturity analysis)
- Which DAP adapters to support first (lldb for Rust, debugpy for Python?)
- Tool schema: what inputs/outputs for each debug command?
- Security: sandboxing debugger access
