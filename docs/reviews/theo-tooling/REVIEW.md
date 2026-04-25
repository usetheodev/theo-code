# theo-tooling — Revisao

> **Contexto**: 21+ tools + sandbox (bwrap/landlock). Bounded Context: Infrastructure.
>
> **Dependencias permitidas**: `theo-domain`.
>
> **Invariantes**: todo tool declara `schema()` + `category()`. Sandbox obrigatorio em bash (cascade bwrap → landlock → noop).
>
> **Status global**: deep-review concluido em 2026-04-25. 292 tests passando, 0 falhas. `cargo clippy --lib --tests` silent (zero warnings em codigo proprio).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `apply_patch` | Tool: aplicar patch unificado (GNU diff format). | Revisado |
| 2 | `bash` | Tool: executar bash sob sandbox (cascade bwrap/landlock/noop). | Revisado |
| 3 | `batch` | Tool: executar multiplas tool-calls em batch. | Revisado |
| 4 | `codebase_context` | Tool: obter contexto agregado da codebase. | Revisado |
| 5 | `codesearch` | Tool: busca semantica em codigo. | Revisado |
| 6 | `edit` | Tool: edicao cirurgica (find/replace exato). | Revisado |
| 7 | `env_info` | Tool: inspecao de variaveis de ambiente (filtradas). | Revisado |
| 8 | `external_directory` | Tool: acesso controlado a diretorios externos ao workspace. | Revisado |
| 9 | `git` | Tool: operacoes git (status, log, diff, blame). | Revisado |
| 10 | `glob` | Tool: busca por pattern de path. | Revisado |
| 11 | `grep` | Tool: busca por regex em conteudo (ripgrep). | Revisado |
| 12 | `http_client` | Tool: HTTP client low-level. | Revisado |
| 13 | `invalid` | Tool: handler para tool-calls invalidas (hardening). | Revisado |
| 14 | `ls` | Tool: listar diretorio. | Revisado |
| 15 | `lsp` | Tool: integracao Language Server Protocol. | Revisado |
| 16 | `memory` | Tool: acesso a memoria do agent (read/write). | Revisado |
| 17 | `multiedit` | Tool: multiplas edicoes no mesmo arquivo em uma chamada. | Revisado |
| 18 | `mutation_queue` | Fila de mutacoes pendentes para aplicacao ordenada. | Revisado |
| 19 | `path` | Utilidades de path seguras (canonicalize, allowlist). | Revisado |
| 20 | `plan` | Tool: planning tool (criar/atualizar plano). | Revisado |
| 21 | `question` | Tool: fazer pergunta ao usuario (pausa no loop). | Revisado |
| 22 | `read` | Tool: leitura de arquivos (com limit/offset). | Revisado |
| 23 | `reflect` | Tool: reflexao/meta-cognicao do agent. | Revisado |
| 24 | `registry` | Registry central de tools (descoberta + dispatch). | Revisado |
| 25 | `sandbox::bwrap` | Backend bwrap (bubblewrap — Linux). | Revisado |
| 26 | `sandbox::command_validator` | Validador de comandos antes de execucao. | Revisado |
| 27 | `sandbox::denied_paths` | Lista de paths proibidos. | Revisado |
| 28 | `sandbox::env_sanitizer` | Sanitizacao de env vars (strip secrets). | Revisado |
| 29 | `sandbox::executor` | Executor comum sobre os backends de sandbox. | Revisado |
| 30 | `sandbox::macos` | Backend macOS (sandbox-exec / seatbelt). | Revisado |
| 31 | `sandbox::network` | Regras de rede dentro do sandbox. | Revisado |
| 32 | `sandbox::probe` | Probe para detectar qual backend esta disponivel. | Revisado |
| 33 | `sandbox::rlimits` | Limites de recurso (CPU, memoria, arquivos). | Revisado |
| 34 | `shell_tool` | Tool generica de shell (wrapper). | Revisado |
| 35 | `skill` | Tool: executar skill (comandos nomeados). | Revisado |
| 36 | `task` | Tool: criar/atualizar task (sub-agent dispatch). | Revisado |
| 37 | `think` | Tool: thinking/reasoning explicito antes de agir. | Revisado |
| 38 | `todo` | Tool: gerenciamento de to-dos. | Revisado |
| 39 | `tool_manifest` | Manifest declarativo de tools (discovery). | Revisado |
| 40 | `truncate` | Truncamento de outputs de tool-call. | Revisado |
| 41 | `undo` | Desfazer ultima operacao (onde suportado). | Revisado |
| 42 | `webfetch` | Tool: fetch de URL. | Revisado |
| 43 | `websearch` | Tool: busca web. | Revisado |
| 44 | `wiki_tool` | Tool: query do Code Wiki. | Revisado |
| 45 | `write` | Tool: escrever arquivo (overwrite controlado). | Revisado |

---

## Notas de Deep-Review

> Auditoria orientada a: (1) cada tool implementa `Tool` trait + declara `schema()`/`category()`, (2) sandbox cascade bwrap → landlock → noop em bash, (3) zero `unwrap()` em paths producao, (4) cobertura de testes.

### File-system tools (8)
- **read**: leitura com `limit`/`offset` opcional. Char-boundary safe (UTF-8). Path validation via `path::canonicalize_within_workspace`.
- **write**: overwrite controlado. Atomic-write pattern (temp + rename). Path allowlist enforcement.
- **edit**: find/replace exato — falha se string nao encontrada ou ocorrencia ambigua. Preserva idempotencia.
- **multiedit**: N edits batch sobre o mesmo arquivo, all-or-nothing transaction.
- **apply_patch**: aplica patch unificado (GNU diff). Validation pre-apply.
- **ls**: directory listing com filter patterns.
- **glob**: glob-pattern path search.
- **grep**: ripgrep wrapper. Regex validation antes de spawn.

### Search & retrieval tools (3)
- **codebase_context**: agrega snippets ranqueados via theo-engine-retrieval.
- **codesearch**: busca semantica direta (BM25 + dense + RRF).
- **wiki_tool**: query Code Wiki por concept name.

### Execution tools (4)
- **bash**: shell sandbox cascade — bwrap (Linux) → landlock (Linux) → noop (testing/macOS-fallback). T1.1 hardening pinned.
- **shell_tool**: wrapper generico.
- **batch**: agrega N tool calls em uma chamada (max 25 cap, T7.3 batch dimension).
- **invalid**: handler para invalid tool calls — emite tool_result error mas NAO crasha o run.

### Communication tools (3)
- **http_client**: HTTP low-level (GET/POST/etc). Sandbox-aware.
- **webfetch**: high-level URL fetch (HTML strip).
- **websearch**: busca web via provider configuravel.

### Agent meta tools (5)
- **think**: thinking-aloud reasoning, sem side-effects.
- **plan**: cria/atualiza plano em `.theo/plans/`.
- **task**: dispatch de sub-agent task.
- **skill**: executa skill nomeada (bridge para skill registry).
- **reflect**: meta-cognicao explicita.

### Workflow tools (3)
- **todo**: lista de to-dos.
- **mutation_queue**: fila de mutacoes pendentes para apply ordenado.
- **undo**: rollback last operation (where supported).

### Memory & info tools (3)
- **memory**: read/write memorias do agent.
- **env_info**: inspecao filtered de env vars (strip secrets).
- **question**: pausa loop para input do usuario.

### Tooling infra (4)
- **registry**: trait `ToolRegistry` + `create_default_registry()`. Tool descoberta + dispatch.
- **tool_manifest**: schema declarativo (.toml/.yaml) para discovery.
- **truncate**: truncamento de outputs (TOOL_PREVIEW_BYTES, etc.).
- **path**: canonicalize + allowlist + sensitive-pattern detection.

### Specialized (3)
- **lsp**: LSP client para hover/definitions/references.
- **git**: git operations wrapper (status, log, diff, blame).
- **external_directory**: acesso controlado fora do workspace (whitelist explicito).

### Sandbox subsystem (9)
- **sandbox::bwrap** (Linux): bubblewrap subprocess wrapper. Mount namespaces, `--tmpfs /tmp`, `--ro-bind /usr`, `--cap-drop ALL`.
- **sandbox::command_validator**: lexical validation pre-spawn (block dangerous patterns).
- **sandbox::denied_paths**: lista hardcoded de paths sempre proibidos.
- **sandbox::env_sanitizer**: ALWAYS_STRIPPED_ENV_PREFIXES (OPENAI_API_KEY, AWS_*, GITHUB_TOKEN, etc.). T1.1 underpinning.
- **sandbox::executor**: trait `SandboxExecutor` + impls (BwrapExecutor, LandlockExecutor, NoopExecutor).
- **sandbox::macos**: macOS seatbelt/sandbox-exec backend.
- **sandbox::network**: network policy (default deny + allowlist).
- **sandbox::probe**: kernel feature detection (`probe_kernel` retorna SandboxCapabilities). Used pelo run_engine_sandbox para escolher cascade.
- **sandbox::rlimits**: setrlimit pre_exec hook (CPU, memory, fsize, nproc).

**Validacao:**
- 292 tests passando, 0 falhas
- `cargo clippy -p theo-tooling --lib --tests` silent (zero warnings em codigo proprio — sem fixes nesta auditoria)
- ADR dep invariant preservada: theo-domain (workspace) + tokio/serde/serde_json/reqwest/regex/landlock/libc/etc (external)
- Cada tool implementa `Tool` trait + declara `schema()` + `category()` (validacao via `tool_manifest`)
- Sandbox cascade testada: bwrap (Linux com /usr/bin/bwrap), landlock (Linux 5.13+), noop (macOS / minimal containers / fail-closed wrapper)

Sem follow-ups bloqueadores. O crate cobre 21+ tool surfaces consumiveis pelo agent + 9 sandbox primitives + 4 infra tools (registry, manifest, truncate, path), totalizando 45 unidades de revisao bem segregadas.
