---
name: meeting-reset
description: Use apos concluir uma tarefa aprovada para resetar o gate. A proxima alteracao exigira nova /meeting. Tambem use quando quiser revogar uma aprovacao.
user-invocable: true
allowed-tools: Bash(rm *), Write
---

# Meeting Reset

Reseta o gate de meeting para que a proxima alteracao exija nova `/meeting`.

## Quando Usar

- Apos concluir a tarefa que foi aprovada na meeting
- Quando quiser revogar uma aprovacao pendente
- No inicio de uma nova sessao de trabalho

## Acao

1. Remove o arquivo `.claude/gate/status`
2. Reporta que o gate foi resetado

O arquivo `.claude/gate/meeting-minutes.md` e PRESERVADO como historico.

Argumento: $ARGUMENTS
