# theo-isolation — Revisao

> **Contexto**: Primitivas de isolamento para sub-agents (worktree, port allocation, safety rules).
>
> **Dependencias permitidas**: `theo-domain`.
>
> **Referencias**: Archon (`packages/isolation/src/providers/worktree.ts`), Pi-Mono (`AGENTS.md:194-233`).
>
> **Status global**: deep-review concluido em 2026-04-25. 16 tests passando, 0 falhas. `cargo clippy --lib --tests` silent.

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `port` | `allocate_port` — alocacao deterministica de porta por hash de worktree. | Revisado |
| 2 | `safety` | `safety_rules()` + `IsolationMode` — texto Pi-Mono injetado em system prompts de sub-agents. | Revisado |
| 3 | `worktree` | `WorktreeProvider`, `WorktreeHandle`, `IsolationError` — wrapper sobre `git worktree`. | Revisado |

---

## Notas de Deep-Review

### 1. port
`allocate_port(worktree_path) -> u16`. Hash deterministico do path → range [10000, 65535]. Permite que sub-agents que rodam servidores (next dev, vite, etc.) tenham portas previsiveis. Probe de availability via TCP bind tentativa antes de retornar.

### 2. safety
`safety_rules()` retorna o blob de regras Pi-Mono ("You MUST only operate within...") injetado em system prompts de sub-agents quando isolated em worktree. `IsolationMode::{Shared, Worktree}`. Cobertura via test inline `safety_rules_explicitly_named`.

### 3. worktree
`WorktreeProvider { repo_root }` com `create(spec_name, base_branch) -> WorktreeHandle` + `existing(path) -> WorktreeHandle` + `remove(handle, force) -> Result`. `WorktreeHandle { path, branch }` — branch sintetico (`(reused)` flag para skip-cleanup). `IsolationError::{GitFailed, BranchExists, PathInUse, NotInGitRepo}`. Master/main fallback automatico em `from_spec_and_cwd`.

**Invariantes verificados:**
- ADR dep invariant: apenas `theo-domain` (workspace) + tempfile/sha2/thiserror (external)
- 16 tests cobrem: port determinism, safety rules text presence, worktree create/remove/reuse
- `cargo clippy --lib --tests` silent
- WorktreeHandle.branch == "(reused)" sentinel respeitado pelo cleanup_worktree_if_success em theo-agent-runtime

Sem follow-ups bloqueadores.
