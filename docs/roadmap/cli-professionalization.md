# CLI Professionalization Plan

> **Objetivo**: Transformar o `theo-cli` em um terminal agent de qualidade profissional (referência: Claude Code, Aider, Cursor CLI), aproveitando `crossterm 0.28`, `pulldown-cmark 0.13`, `rustyline 15` já adicionados ao workspace.
>
> **Data**: 2026-04-11
> **Status**: Planejado — aguarda aprovação em `/meeting`
> **Fontes**: `outputs/reports/rust-terminal-ecosystem.md`, análise interna de `apps/theo-cli/src/`

---

## 1. Diagnóstico Atual (baseline)

O `theo-cli` tem **arquitetura boa**, mas **renderização amadora**:

| Área | Problema | Evidência |
|---|---|---|
| ANSI hardcoded | 35+ sequências `\x1b[...]` espalhadas | `renderer.rs`, `repl.rs`, `commands.rs`, `pilot.rs`, `main.rs` |
| `crossterm` não usado | Dependência declarada, zero imports | `apps/theo-cli/Cargo.toml` |
| `pulldown-cmark` não usado | Disponível, mas LLM output não é renderizado como markdown | `renderer.rs` usa `eprint!` cru |
| Sem syntax highlighting | Code blocks em responses saem como texto cru | n/a |
| Sem TTY detection | Pipeline quebra colors quando piped | `eprintln!` direto |
| Sem spinners/progress | Long ops (GRAPHCTX build, init, pilot) sem feedback visual | `init.rs`, `pilot.rs` |
| Slash commands limitados | 7 comandos, sem completion, sem aliases | `commands.rs` |
| Sem permission UI | Tool approvals não existem (executa tudo) | `renderer.rs` |
| Hard-coded truncation | `80`, `70`, `78` chars espalhados | `renderer.rs` L78, L248, L257 |
| Paths hardcoded | `~/.config/theo/...` ignora XDG | `repl.rs`, `commands.rs` |
| Sem multi-line input | REPL só aceita single-line | `repl.rs` |

**Métricas base**: 2384 LOC em `apps/theo-cli/src/`, 23 testes unitários, 7 slash commands, 15 tool events cobertos.

---

## 2. Arquitetura Alvo

```
apps/theo-cli/src/
├── main.rs                    # Entry point (refactor leve)
├── repl.rs                    # REPL com rustyline + completion + multiline
├── config/
│   ├── mod.rs                 # TheoConfig (serde), XDG paths
│   └── theme.rs               # Color theme (default + custom)
├── tty/
│   ├── mod.rs                 # TtyCaps (is_tty, width, colors, unicode)
│   └── resize.rs              # AtomicU16 width cache, resize listener
├── render/
│   ├── mod.rs                 # Renderer trait, factory
│   ├── style.rs               # Style constants via crossterm (NO raw ANSI)
│   ├── streaming.rs           # Incremental LLM content stream renderer
│   ├── markdown.rs            # pulldown-cmark → crossterm output
│   ├── code_block.rs          # syntect-based syntax highlighting
│   ├── tool_result.rs         # Per-tool result rendering (ex render_tool_completed)
│   ├── diff.rs                # Edit/patch diff rendering (+/-)
│   ├── table.rs               # Aligned tables for /status, /skills, /memory
│   ├── progress.rs            # indicatif spinners and bars
│   └── banner.rs              # Startup banner
├── commands/
│   ├── mod.rs                 # Command registry + dispatcher
│   ├── help.rs                # /help with grouped output
│   ├── status.rs              # /status (table)
│   ├── model.rs               # /model (switch model)
│   ├── cost.rs                # /cost (token + $ tracking)
│   ├── clear.rs               # /clear
│   ├── compact.rs             # /compact (summarize session)
│   ├── memory.rs              # /memory list/search/delete
│   ├── skills.rs              # /skills
│   ├── mode.rs                # /mode agent|plan|ask
│   ├── init.rs                # /init (re-run init)
│   ├── review.rs              # /review (changed files)
│   ├── bug.rs                 # /bug (capture session to file)
│   ├── doctor.rs              # /doctor (env/deps diagnostics)
│   └── export.rs              # /export (session → md/json)
├── input/
│   ├── mod.rs                 # Input processor
│   ├── completer.rs           # Tab completion (commands + files via @)
│   ├── hinter.rs              # Rustyline hints
│   ├── highlighter.rs         # Rustyline syntax highlighter
│   ├── multiline.rs           # Multi-line input (``` or Alt+Enter)
│   └── mention.rs             # @file parsing
├── permission/
│   ├── mod.rs                 # PermissionGate
│   └── prompt.rs              # dialoguer-based y/n/always
├── status_line/
│   ├── mod.rs                 # Status line renderer (tokens, model, cost)
│   └── format.rs              # Segments + layout
├── init.rs                    # (mantido, leve refactor)
└── pilot.rs                   # (mantido, usa progress.rs)
```

---

## 3. Dependencies a Adicionar

Já no workspace: `crossterm`, `pulldown-cmark`, `rustyline`.

**Adicionar** (workspace):
```toml
syntect = { version = "5", default-features = false, features = ["default-fancy"] }
indicatif = "0.18"
console = "0.16"
dialoguer = "0.11"
textwrap = { version = "0.16", features = ["terminal_size"] }
comfy-table = "7"              # tabelas alinhadas
dirs = "5"                     # XDG paths
```

**DoD**: `cargo check` passa, `cargo tree` não mostra conflitos de versão, todas as crates com licença compatível (MIT/Apache-2.0).

---

## 4. Fases e Microtasks

### FASE 0 — Foundation (enabler, sem features visíveis)

#### T0.1 — Adicionar dependências ao workspace
- **Ação**: Editar `Cargo.toml` root `[workspace.dependencies]` com as 7 crates acima.
- **Critérios de aceite**:
  - `cargo build -p theo` compila sem warnings novos.
  - `cargo tree -p theo | grep -E "(syntect|indicatif|console|dialoguer|textwrap|comfy-table|dirs)"` lista todas.
  - Tempo de build delta < 30s.
- **DoD**: PR aprovado, build verde no vast.ai.

#### T0.2 — Criar módulo `tty/` com detecção
- **Ação**: Criar `apps/theo-cli/src/tty/mod.rs` com struct `TtyCaps { is_tty, colors, unicode, width }` e função `TtyCaps::detect()` usando `console::Term::stderr()` + `crossterm::terminal::size()`.
- **Criar** `tty/resize.rs` com `AtomicU16 TERM_WIDTH` e função `install_resize_listener(tokio::task)`.
- **Critérios de aceite**:
  - `TtyCaps::detect()` retorna `is_tty=false` quando stderr piped (testar com `2>/dev/null` via integration test).
  - `colors=false` quando `NO_COLOR` env var presente.
  - `width` atualiza em `SIGWINCH` (testar enviando signal em integration test).
  - Unit tests cobrindo as 3 branches.
- **DoD**: `cargo test -p theo tty::` verde, >= 4 testes.

#### T0.3 — Criar módulo `render/style.rs`
- **Ação**: Definir constantes de estilo usando `crossterm::style::{Color, Attribute}` + wrappers que respeitam `TtyCaps.colors`:
  ```rust
  pub fn success() -> Style { ... }
  pub fn error() -> Style { ... }
  pub fn warn() -> Style { ... }
  pub fn dim() -> Style { ... }
  pub fn accent() -> Style { ... }  // cyan, prompts
  pub fn tool_name() -> Style { ... }
  pub fn code_bg() -> Style { ... }
  ```
- Macros `theo_print!`, `theo_println!` que usam `queue!` + flush.
- **Critérios de aceite**:
  - Nenhuma string `"\x1b["` fora de `render/style.rs` (enforço via grep em CI).
  - Quando `TtyCaps.colors=false`, output passa em `strip_ansi_codes` idempotente.
  - Testes snapshot com `insta` para cada estilo.
- **DoD**: `grep -rn "\\\\x1b\\[" apps/theo-cli/src/ | grep -v style.rs` retorna vazio.

#### T0.4 — Criar `config/mod.rs` com TheoConfig serde
- **Ação**: Struct `TheoConfig` com campos: `theme`, `model`, `provider`, `max_iterations`, `session_max_messages`, `permission_mode`, `truncation_limits`, `keybindings`.
- Load de `$XDG_CONFIG_HOME/theo/config.toml` (via `dirs::config_dir()`) com fallback para defaults.
- Support para `$THEO_CONFIG` override.
- **Critérios de aceite**:
  - Config inexistente → defaults sem erro.
  - Config malformada → erro claro com linha/coluna, não panic.
  - Testes: defaults, load válido, load corrompido, override por env.
- **DoD**: 6+ testes, doc comment em cada campo explicando default.

---

### FASE 1 — Renderização Profissional (core visual)

#### T1.1 — Migrar `renderer.rs` para usar `render/style.rs`
- **Ação**: Substituir todos os `\x1b[...]` em `renderer.rs` por chamadas via `render/style.rs`. Dividir o arquivo: `render/tool_result.rs` (per-tool rendering), `render/streaming.rs` (ContentDelta/ReasoningDelta).
- **Critérios de aceite**:
  - Output visual **idêntico** antes/depois em ambiente TTY (comparação manual + snapshot test com `insta`).
  - Output em `2>/dev/null` não contém sequências ANSI.
  - Todos os 23 testes existentes continuam verdes.
- **DoD**: Regressão zero, `cargo test -p theo` verde.

#### T1.2 — Implementar `render/markdown.rs` (não-streaming)
- **Ação**: Função `render_markdown(text: &str, caps: &TtyCaps) -> String` usando pulldown-cmark → crossterm styled output. Suporte para:
  - Headers (H1-H3 com bold + cor)
  - Inline `**bold**`, `*italic*`, `` `code` ``
  - Listas (`- `, `1. `) com indent
  - Blockquotes com borda `│`
  - Links `[text](url)` com underline + URL dim
  - `---` horizontal rule
- **Critérios de aceite**:
  - Snapshot tests cobrindo cada elemento.
  - Tabela markdown renderizada alinhada via `comfy-table`.
  - Output fallback ASCII quando `caps.unicode=false`.
- **DoD**: 12+ snapshot tests, doc com exemplo visual.

#### T1.3 — Implementar `render/code_block.rs` com syntect
- **Ação**: Struct `CodeHighlighter` com `SyntaxSet` + `ThemeSet` carregados uma vez (lazy `OnceLock`). Método `highlight(code: &str, lang: &str) -> String` que retorna ANSI-colorized output.
- Render com box border (caracteres Unicode `─`, `│`) + language label.
- Tema default: `base16-ocean.dark`, configurável via `TheoConfig.theme`.
- **Critérios de aceite**:
  - Load `SyntaxSet` < 50ms (benchmark `cargo bench`).
  - Suporte para pelo menos: rust, python, js, ts, go, java, bash, json, yaml, toml, html, css, sql.
  - Fallback para `plain text` em linguagens não reconhecidas (sem erro).
  - Testes para cada linguagem listada.
- **DoD**: Benchmark registrado, 12+ testes, output visual validado.

#### T1.4 — Implementar `render/streaming.rs` (incremental markdown)
- **Ação**: Struct `StreamingMarkdownRenderer` com buffer de texto parcial. Lógica:
  - Texto inline (**, *, `): renderiza imediatamente quando tag fecha
  - Code block (```): buffera até fence fechar, aí chama `code_block::highlight`
  - Headers, listas: renderiza quando newline chega
- Preserva idempotência: duas streams idênticas produzem mesma saída.
- **Critérios de aceite**:
  - Teste: stream char-by-char de "Hello **world**" produz output com "world" em bold.
  - Teste: stream char-by-char de ` ```rust\nfn main(){}\n``` ` produz bloco syntax-highlighted **só após fence fechar**.
  - Teste: stream interrompido no meio de bold não vaza estado (reset em `flush()`).
  - Latência por chunk < 1ms.
- **DoD**: 15+ testes cobrindo edge cases, property test com `proptest` para aleatoriedade.

#### T1.5 — Integrar streaming renderer no `CliRenderer::ContentDelta`
- **Ação**: `renderer.rs` agora delega `ContentDelta` para `StreamingMarkdownRenderer`. Add `flush()` no final do turn (ao receber `RunStateChanged::Idle`).
- **Critérios de aceite**:
  - Integration test: agent responde com markdown → terminal mostra formatado.
  - Response com ` ```rust ... ``` ` aparece highlighted.
  - Performance: não aumenta latência de first-token > 10ms.
- **DoD**: 3+ integration tests no `apps/theo-cli/tests/`.

#### T1.6 — Implementar `render/diff.rs` para Edit/Patch
- **Ação**: Renderer de diff com:
  - `+` linhas em verde
  - `-` linhas em vermelho
  - Context lines dim
  - Line numbers à esquerda
  - Syntax highlighting condicional (linguagem do arquivo) via syntect
- **Critérios de aceite**:
  - Substitui lógica inline de `render_tool_completed` para Edit/Patch (hoje L182-230 em `renderer.rs`).
  - Output aceita max_width = terminal width, trunca com `…`.
  - Testes cobrindo: simple edit, multi-hunk patch, long lines.
- **DoD**: 8+ snapshot tests, integration test com Edit real.

#### T1.7 — Implementar `render/table.rs`
- **Ação**: Wrapper sobre `comfy-table::Table` com estilo consistente. Usado por `/status`, `/skills`, `/memory list`, `/cost`.
- **Critérios de aceite**:
  - Tabelas alinhadas, bordas Unicode em TTY / ASCII em piped.
  - Auto-fit ao terminal width.
- **DoD**: Helper reusável, 4+ testes.

#### T1.8 — Implementar `render/progress.rs` com indicatif
- **Ação**: Helpers para spinner, progress bar, multi-progress. Integração com `TtyCaps` — no-op quando piped.
- Uso em: GRAPHCTX build (`repl.rs` L75-88), `init.rs` AI enrichment, `pilot.rs` loop.
- **Critérios de aceite**:
  - Spinner não aparece em piped output.
  - Spinner cessa em `Ctrl+C` sem deixar linha órfã.
  - Multi-progress para sub-agents paralelos.
- **DoD**: Usado em pelo menos 3 call sites, 4+ testes.

---

### FASE 2 — Comandos e Interatividade

#### T2.1 — Refatorar `commands.rs` em `commands/` registry
- **Ação**: Criar trait `SlashCommand { name, aliases, description, category, async execute(&self, ctx, args) }`. Registry carrega todos os comandos em vec. Dispatcher usa lookup por nome/alias.
- **Critérios de aceite**:
  - Todos os 7 comandos existentes portados sem regressão.
  - `/help` agora lista por categoria (Session, Info, Config, Action).
  - Adicionar um comando = 1 arquivo + 1 linha no registry.
- **DoD**: Todos os testes antigos verdes, 2+ novos testes de registry.

#### T2.2 — Adicionar comandos profissionais
Por ordem de prioridade:

| Comando | Ação | DoD |
|---|---|---|
| `/model [name]` | Lista/troca modelo em runtime | Lista via provider registry, troca valida, persiste em session |
| `/cost` | Mostra tokens+custo da sessão | Pega de event bus (LlmCallEnd), formata via table.rs |
| `/clear` | Limpa tela + opcional session | Usa `crossterm::terminal::Clear`, flag `--session` |
| `/compact` | Compacta sessão (summarization) | Invoca LLM para resumir msgs antigas, mantém últimas 10 |
| `/init` | Re-run init | Wrapper de `init.rs`, idempotente |
| `/review` | Review de arquivos changed | `git diff HEAD` → feed ao agent |
| `/bug` | Captura sessão atual para bug report | JSON com config + últimas N mensagens (PII-free) |
| `/doctor` | Diagnósticos (env, deps, provider reach) | Checa env vars, provider auth, sandbox (bwrap/landlock), write em `table.rs` |
| `/export <md\|json>` | Exporta sessão | Gera markdown legível ou JSON estruturado |

- **Critério global**: Cada comando com 3+ testes (happy path, error, edge).
- **DoD por comando**: `/help <cmd>` mostra descrição + exemplos, 3+ testes, integration test.

#### T2.3 — Tab completion via rustyline
- **Ação**: Implementar `input/completer.rs` com trait `rustyline::completion::Completer`:
  - Prefix `/` → completa slash commands do registry
  - Prefix `@` → completa file paths (usa `ignore` crate para respeitar .gitignore)
  - Flag `--` → completa flags do comando atual
- **Critérios de aceite**:
  - Tab após `/st` completa para `/status`.
  - Tab após `@src/` lista arquivos do dir.
  - Tab após `/model ` lista modelos disponíveis.
- **DoD**: 10+ unit tests mockando rustyline context.

#### T2.4 — Hints e Highlighter no rustyline
- **Ação**:
  - `input/hinter.rs`: sugere próximo comando com base em history (dim text).
  - `input/highlighter.rs`: pinta `/cmd` em cyan, `@file` em accent, `--flag` em yellow.
- **Critérios de aceite**:
  - Hints só aparecem em TTY, não em piped.
  - Highlighter não quebra multi-line input.
- **DoD**: 6+ testes.

#### T2.5 — Multi-line input
- **Ação**: Detectar abertura de ``` no input → entrar em modo multi-line até fechar. Alternativa: `Alt+Enter` força newline. Persist em history como bloco único.
- **Critérios de aceite**:
  - `echo '```\nfoo\nbar\n```' | theo agent` funciona como input único.
  - Multi-line preserva indentação.
  - `Ctrl+C` no meio aborta cleanly.
- **DoD**: 5+ testes, integration test.

#### T2.6 — @file mention parsing
- **Ação**: `input/mention.rs` detecta `@path/to/file` no input e injeta conteúdo como contexto antes de enviar ao agent.
- **Critérios de aceite**:
  - Path absoluto e relativo suportados.
  - Respeita `.gitignore` e `.theoignore`.
  - Arquivo não existe → warning, não aborta.
  - Máx 10 mentions por turn (anti-abuse).
- **DoD**: 8+ testes, doc no `/help`.

---

### FASE 3 — Permission Gate & Status Line

#### T3.1 — Permission prompt UI via dialoguer
- **Ação**: `permission/prompt.rs` com struct `PermissionPrompt`. Para cada tool call que exige aprovação:
  - Mostra tool + args resumidos (e.g. `bash: rm -rf target/`)
  - Opções: `[y] Yes  [n] No  [a] Always  [d] Deny always`
  - Persiste escolhas "Always" em `session.acl`.
- **Critérios de aceite**:
  - Plug no `theo-governance::PolicyEngine`.
  - Modo `auto-accept` bypass tudo (para CI/scripts).
  - Ctrl+C cancela prompt = deny.
  - Testes com dialoguer mockado.
- **DoD**: Integration test com tool mock, 6+ unit tests.

#### T3.2 — Status line renderer
- **Ação**: `status_line/mod.rs` renderiza uma linha persistente no bottom com:
  - `[mode]` agent/plan/ask
  - `[model]` provider/model
  - `[tokens]` in/out/total
  - `[cost]` $ running
  - `[time]` elapsed turn
- Usa alternate screen region OU re-desenha no newline (sem alternate screen — manter streaming append-only).
- **Critérios de aceite**:
  - Atualiza em cada `LlmCallEnd`.
  - Não interfere com streaming text.
  - Desabilitável via config.
- **DoD**: Visual test (manual), unit tests de formatação.

#### T3.3 — Banner renderizado
- **Ação**: `render/banner.rs` substitui `print_banner` em `repl.rs`. Mostra:
  - ASCII art do logo (opt-in, pequeno)
  - Versão, provider, model, mode
  - Dica rápida (`/help for commands`)
  - Warning se provider não autenticado
- **Critérios de aceite**:
  - Usa `render/style.rs` (zero raw ANSI).
  - Adapta a largura do terminal.
- **DoD**: Snapshot test, visual OK.

---

### FASE 4 — Polish & DX

#### T4.1 — Textwrap para explanation text
- **Ação**: Aplicar `textwrap::wrap` no conteúdo de respostas não-markdown para respeitar terminal width.
- **DoD**: Wrapping funciona, code blocks intocados.

#### T4.2 — OSC 8 hyperlinks (opcional)
- **Ação**: Detectar suporte via env var `FORCE_HYPERLINK` ou capability detection. Emitir `\x1b]8;;URL\x07text\x1b]8;;\x07`.
- **DoD**: Feature flag, fallback para URL dim.

#### T4.3 — Keybinding config
- **Ação**: `TheoConfig.keybindings` permite custom key → command. Default: Ctrl+L = /clear, Ctrl+R = history search, Alt+Enter = newline.
- **DoD**: 4+ tests, docs.

#### T4.4 — Session export/import
- **Ação**: `/export session.md` gera markdown legível. `theo --resume session.json` restaura.
- **DoD**: Round-trip test.

#### T4.5 — Hardcoded paths → XDG
- **Ação**: Replace hardcoded `~/.config/theo/` em `repl.rs` e `commands.rs` por `dirs::config_dir()`. Respeita `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`.
- **DoD**: Testes com env vars mockados.

#### T4.6 — Error messages estruturados
- **Ação**: Todos os erros user-facing passam por `render::error()` com:
  - Ícone/cor
  - Mensagem curta
  - Hint de próximo passo
  - Link para docs (quando aplicável)
- **DoD**: Snapshot tests de cada tipo de erro.

---

## 5. Sequenciamento e Critérios de Go/No-Go

| Fase | Duração estimada | Gate para próxima |
|---|---|---|
| Fase 0 — Foundation | 1 sprint | Todos os testes verdes + nenhum raw ANSI fora de style.rs |
| Fase 1 — Rendering | 2 sprints | Visual inspection + integration tests + benchmark syntect < 50ms |
| Fase 2 — Commands | 2 sprints | 16+ slash commands funcionais + tab completion |
| Fase 3 — Permission + Status | 1 sprint | Permission flow testado end-to-end |
| Fase 4 — Polish | 1 sprint | Config completa + export/import + XDG compliance |

**Regra**: Cada fase só avança se `cargo test -p theo` verde e `/meeting` aprovou a próxima.

---

## 6. Métricas de Sucesso

| Métrica | Baseline | Meta |
|---|---|---|
| Raw ANSI sequences fora de `style.rs` | 35+ | 0 |
| Slash commands | 7 | 16+ |
| Unit tests no theo-cli | 23 | 120+ |
| Integration tests | 0 | 15+ |
| Time to first token (TTFT) | X ms | X ms (sem regressão) |
| Code coverage (tarpaulin) | ? | ≥ 75% |
| Comandos com tab completion | 0 | todos |
| Config file support | ❌ | ✅ TOML + XDG |
| TTY detection | ❌ | ✅ |
| Syntax highlighting | ❌ | ✅ 12+ langs |
| Permission prompts | ❌ | ✅ |
| Markdown rendering | ❌ | ✅ streaming + static |

---

## 7. Riscos e Mitigações

| Risco | Impacto | Mitigação |
|---|---|---|
| Syntect lento em debug | Dev experience ruim | `default-fancy` feature, benchmark, lazy load |
| Streaming markdown quebra em edge cases | Output corrompido | Property tests, fallback para plain text |
| Breaking change em rustyline 14→15 | Compile errors | Ler CHANGELOG, migrar incrementalmente |
| Permission prompts bloqueando CI | Testes travados | Flag `--auto-accept` ou `THEO_AUTO_ACCEPT=1` |
| Config schema evolution | Configs antigas quebram | Versioning + migration function |
| Overhead de indicatif em não-TTY | Logs poluídos | TtyCaps gate em todos os spinners |
| Ratatui tentação | Scope creep | Decisão documentada: não usar, só crossterm |

---

## 8. Out of Scope (não fazer agora)

- Ratatui/TUI completo (split panes, vim mode) — só se Theo CLI evoluir para full-screen
- Image rendering (Kitty/iTerm2 protocol)
- Mouse support
- Plugin system para custom renderers
- Screen reader accessibility audit
- i18n de mensagens
- Remote session sharing

---

## 9. Validação de Pronto

**Definition of Done global**:
1. Todos os testes do workspace verdes no vast.ai.
2. `grep -rn "\\\\x1b\\[" apps/theo-cli/src/` retorna apenas `render/style.rs`.
3. Snapshot tests com `insta` review-aprovados.
4. `theo --help` mostra 16+ slash commands.
5. `cargo build --release` < 2min total delta vs baseline.
6. CHANGELOG.md atualizado com `Added`, `Changed`.
7. Doc em `docs/current/cli-rendering.md` explicando arquitetura final.
8. Review FAANG aprovado via `/meeting`.

---

## 10. Referências

- **Pesquisa**: `outputs/reports/rust-terminal-ecosystem.md`
- **Análise atual**: resposta do Explore agent (embutida no histórico desta conversa)
- **Inspirações**: Claude Code, Aider, bat, delta, gitui
- **ADRs relacionados**: `docs/adr/` (a criar: ADR-XXX Streaming Markdown Rendering)
- **Memory**: `Context Engineering`, `FAANG review learnings`, `Extend not duplicate`
