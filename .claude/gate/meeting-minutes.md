# Meeting — 2026-04-01 (BwrapExecutor)

## Proposta
BwrapExecutor como backend primário de sandbox via bubblewrap. Cascata: bwrap > landlock > noop.

## Participantes
- governance, qa, tooling

## Veredito
**APPROVED**

## Escopo Aprovado
- `crates/theo-tooling/src/sandbox/bwrap.rs` (novo)
- `crates/theo-tooling/src/sandbox/mod.rs`
- `crates/theo-tooling/src/sandbox/probe.rs`
- `crates/theo-tooling/src/sandbox/executor.rs`

## Condicoes
1. Usar /usr/bin/bwrap hardcoded (não PATH lookup)
2. Flags: --ro-bind root, --bind project (write), --tmpfs /tmp, --unshare-pid, --unshare-net, --cap-drop ALL, --die-with-parent, --new-session
3. Command validator + env sanitizer como pré-filtro (antes de bwrap)
4. Probe detecta bwrap via version check
5. create_executor: bwrap > landlock > noop
6. 7 testes novos mínimos (QA spec)
7. Testes com guard `if !bwrap_available() { return; }` (não #[ignore])
8. Regressão: 1044 testes passam
