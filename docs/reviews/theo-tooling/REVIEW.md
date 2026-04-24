# theo-tooling — Revisao

> **Contexto**: 21+ tools + sandbox (bwrap/landlock). Bounded Context: Infrastructure.
>
> **Dependencias permitidas**: `theo-domain`.
>
> **Invariantes**: todo tool declara `schema()` + `category()`. Sandbox obrigatorio em bash (cascade bwrap → landlock → noop).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `apply_patch` | Tool: aplicar patch unificado (GNU diff format). | Pendente |
| 2 | `bash` | Tool: executar bash sob sandbox (cascade bwrap/landlock/noop). | Pendente |
| 3 | `batch` | Tool: executar multiplas tool-calls em batch. | Pendente |
| 4 | `codebase_context` | Tool: obter contexto agregado da codebase. | Pendente |
| 5 | `codesearch` | Tool: busca semantica em codigo. | Pendente |
| 6 | `edit` | Tool: edicao cirurgica (find/replace exato). | Pendente |
| 7 | `env_info` | Tool: inspecao de variaveis de ambiente (filtradas). | Pendente |
| 8 | `external_directory` | Tool: acesso controlado a diretorios externos ao workspace. | Pendente |
| 9 | `git` | Tool: operacoes git (status, log, diff, blame). | Pendente |
| 10 | `glob` | Tool: busca por pattern de path. | Pendente |
| 11 | `grep` | Tool: busca por regex em conteudo (ripgrep). | Pendente |
| 12 | `http_client` | Tool: HTTP client low-level. | Pendente |
| 13 | `invalid` | Tool: handler para tool-calls invalidas (hardening). | Pendente |
| 14 | `ls` | Tool: listar diretorio. | Pendente |
| 15 | `lsp` | Tool: integracao Language Server Protocol. | Pendente |
| 16 | `memory` | Tool: acesso a memoria do agent (read/write). | Pendente |
| 17 | `multiedit` | Tool: multiplas edicoes no mesmo arquivo em uma chamada. | Pendente |
| 18 | `mutation_queue` | Fila de mutacoes pendentes para aplicacao ordenada. | Pendente |
| 19 | `path` | Utilidades de path seguras (canonicalize, allowlist). | Pendente |
| 20 | `plan` | Tool: planning tool (criar/atualizar plano). | Pendente |
| 21 | `question` | Tool: fazer pergunta ao usuario (pausa no loop). | Pendente |
| 22 | `read` | Tool: leitura de arquivos (com limit/offset). | Pendente |
| 23 | `reflect` | Tool: reflexao/meta-cognicao do agent. | Pendente |
| 24 | `registry` | Registry central de tools (descoberta + dispatch). | Pendente |
| 25 | `sandbox::bwrap` | Backend bwrap (bubblewrap — Linux). | Pendente |
| 26 | `sandbox::command_validator` | Validador de comandos antes de execucao. | Pendente |
| 27 | `sandbox::denied_paths` | Lista de paths proibidos. | Pendente |
| 28 | `sandbox::env_sanitizer` | Sanitizacao de env vars (strip secrets). | Pendente |
| 29 | `sandbox::executor` | Executor comum sobre os backends de sandbox. | Pendente |
| 30 | `sandbox::macos` | Backend macOS (sandbox-exec / seatbelt). | Pendente |
| 31 | `sandbox::network` | Regras de rede dentro do sandbox. | Pendente |
| 32 | `sandbox::probe` | Probe para detectar qual backend esta disponivel. | Pendente |
| 33 | `sandbox::rlimits` | Limites de recurso (CPU, memoria, arquivos). | Pendente |
| 34 | `shell_tool` | Tool generica de shell (wrapper). | Pendente |
| 35 | `skill` | Tool: executar skill (comandos nomeados). | Pendente |
| 36 | `task` | Tool: criar/atualizar task (sub-agent dispatch). | Pendente |
| 37 | `think` | Tool: thinking/reasoning explicito antes de agir. | Pendente |
| 38 | `todo` | Tool: gerenciamento de to-dos. | Pendente |
| 39 | `tool_manifest` | Manifest declarativo de tools (discovery). | Pendente |
| 40 | `truncate` | Truncamento de outputs de tool-call. | Pendente |
| 41 | `undo` | Desfazer ultima operacao (onde suportado). | Pendente |
| 42 | `webfetch` | Tool: fetch de URL. | Pendente |
| 43 | `websearch` | Tool: busca web. | Pendente |
| 44 | `wiki_tool` | Tool: query do Code Wiki. | Pendente |
| 45 | `write` | Tool: escrever arquivo (overwrite controlado). | Pendente |
