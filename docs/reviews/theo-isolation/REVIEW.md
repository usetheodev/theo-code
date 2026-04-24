# theo-isolation — Revisao

> **Contexto**: Primitivas de isolamento para sub-agents (worktree, port allocation, safety rules).
>
> **Dependencias permitidas**: `theo-domain`.
>
> **Referencias**: Archon (`packages/isolation/src/providers/worktree.ts`), Pi-Mono (`AGENTS.md:194-233`).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `port` | `allocate_port` — alocacao deterministica de porta por hash de worktree. | Pendente |
| 2 | `safety` | `safety_rules()` + `IsolationMode` — texto Pi-Mono injetado em system prompts de sub-agents. | Pendente |
| 3 | `worktree` | `WorktreeProvider`, `WorktreeHandle`, `IsolationError` — wrapper sobre `git worktree`. | Pendente |
