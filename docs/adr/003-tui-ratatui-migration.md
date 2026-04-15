# ADR-003: Migracao do CLI para TUI ratatui

**Status:** Aceito
**Data:** 2026-04-14
**Autor:** Paulo (multi-agent meeting com governance, runtime, qa, frontend, tooling)
**Escopo:** theo-domain (variantes novas de EventType + stdout_tx em ToolContext), theo-agent-runtime (broadcast bridge + emissao de LlmCallEnd), theo-tooling (streaming via ToolContext), theo-cli (novo modulo TUI)
**Rev.1:** 2026-04-14 — correcoes pos-auditoria (stdout_tx via ToolContext, DCP requer ADR-004, undo snapshot-based, eventos ausentes)

---

## Contexto

O Theo Code CLI (`apps/theo-cli`, ~2.1k linhas) usa `rustyline` + `eprint!` direto. Funciona como REPL basico, mas a arquitetura de "log streaming" impede UX competitiva com Claude Code, Gemini CLI, Codex e OpenCode.

### Gaps identificados vs competidores

| # | Gap | Impacto |
|---|---|---|
| 1 | Sem transcript scrollavel | Usuario perde contexto em sessoes longas |
| 2 | Sem tool cards visuais ao vivo | Agente parece morto durante execucao |
| 3 | ToolCallQueued suprimido no renderer | Cards so aparecem pos-conclusao |
| 4 | Sem output parcial de bash | Comandos longos parecem travados |
| 5 | Sem StatusLine permanente | Usuario nao sabe estado do agente |
| 6 | Sem autocomplete /, @, # | Discoverability zero |
| 7 | Sem diff viewer / approval modal | Governance invisivel |
| 8 | Sem model switcher em runtime | Trocar modelo exige reiniciar |
| 9 | Sem scrollback search | Impossivel encontrar output anterior |
| 10 | Sem session picker /resume | Persistencia existe mas e invisivel |

### Evidencia: renderer atual

`apps/theo-cli/src/renderer.rs:38-40`:
```rust
EventType::ToolCallQueued => {
    // Suppressed — shown on ToolCallCompleted
}
```
Bug pre-existente: tool cards so aparecem quando a ferramenta termina. Concorrentes mostram o card imediatamente com spinner e timer ao vivo.

### Evidencia: BashTool sem streaming

`crates/theo-tooling/src/bash/mod.rs:228-236` (path sem sandbox):
```rust
let output = Command::new("sh")
    .arg("-c").arg(&command)
    .stdout(Stdio::piped()).stderr(Stdio::piped())
    .output().await  // bloqueia ate o fim
```

`crates/theo-tooling/src/bash/mod.rs:214-219` (path com sandbox):
```rust
let sandbox_result = tokio::task::spawn_blocking(move || {
    executor_clone.execute_sandboxed(&cmd, &cwd_clone, &config_clone)
}).await  // bloqueia ate o fim
```

Ambos os caminhos capturam stdout completo no final. Zero streaming.

---

## Decisao

Migrar o CLI de rustyline + eprint para **ratatui + crossterm** em 7 fases incrementais, coexistindo com o REPL legado via flag `--tui` (experimental ate F7, quando vira default).

### Principios

1. **Valor demonstravel em cada fase.** F1 ja mostra agente "vivo" com tool cards, timer e streaming.
2. **Legado preservado.** REPL rustyline funciona ate F7. `--tui` e opt-in.
3. **Backend incremental.** Fase 1 adiciona broadcast bridge, ToolCallStdoutDelta, LlmCallEnd, e `stdout_tx` em ToolContext. Fases posteriores adicionam mais variantes de EventType (ProviderSwitched, GovernanceDecision*, GraphContextUpdated, SubAgentSpawned/Completed) — cada uma com /meeting proprio.
4. **Elm/Redux puro.** `(State, Msg) -> State` — render e funcao do estado, sem side-effects no draw.
5. **Fronteiras respeitadas.** Streaming de stdout usa canal lateral via `ToolContext.stdout_tx` (nao muda Tool trait). BashTool envia linhas por ctx.stdout_tx; ToolCallManager converte em DomainEvent. Tooling nunca importa EventBus.
6. **Governance visivel.** Approval modal, impact score e timeline de causalidade sao features, nao afterthought.

---

## Stack

### Fase 1

| Crate | Versao | Uso |
|---|---|---|
| ratatui | 0.29 | Framework TUI (widgets, layout, buffer) |
| crossterm | 0.28 | Backend terminal (input, raw mode, alt screen) |
| tui-textarea | 0.7 | Input multiline com cursor |

### Fases futuras (a confirmar em meetings proprios)

| Crate | Fase | Uso |
|---|---|---|
| tui-overlay | F4 | Modais, drawers, popovers, toasts |
| opaline | F7 | Theme engine (20 presets built-in) |
| tui-syntax-highlight | F2 | Syntax highlighting em code blocks |
| tui-tree-widget | F5 | Sidebar hierarquica (GRAPHCTX, Todos) |
| throbber-widgets-tui | F1 | Spinners animados em tool cards |
| ratatui-toaster | F2 | Toast notifications |
| tachyonfx | F4 | Effects comunicativos (dissolve, sweep) |
| ratatui-image | F5 | Imagens sixel (mermaid, coverage) |
| similar | F4 | Diff computation |
| ansi-to-tui | F2 | Converter output ANSI de tools |
| arboard | F3 | Clipboard copy/paste |
| fuzzy-matcher | F4 | Autocomplete @file, /command, Ctrl+K |

**Descartados:**
- `rat-salsa` — 1 maintainer, <100 stars, supply chain risk. Update loop manual (~50 linhas).
- `edtui` — avaliar em F4 se vim mode justifica a dep.

---

## Arquitetura

### Event Flow

```
theo-agent-runtime                            theo-cli TUI
┌──────────────────┐                         ┌─────────────────────┐
│ AgentRunEngine    │                         │ Input Task          │
│  publishes        │                         │ (crossterm stream)  │
│  DomainEvent      │                         │  -> UserAction mpsc │
│  via EventBus     │                         └──────────┬──────────┘
└────────┬─────────┘                                     │
         │                                               │
         ▼                                               │
┌──────────────────┐    broadcast::Receiver    ┌─────────▼──────────┐
│ EventBus         │ ──────────────────────►  │ Event Task          │
│  .publish()      │    (DomainEvent clone)    │ (batching 16ms)    │
│  .subscribe()    │                           │  -> TuiMsg mpsc    │
│  .subscribe_     │                           └─────────┬──────────┘
│   broadcast()    │                                     │
└──────────────────┘                                     │
                                                         ▼
                                               ┌─────────────────────┐
                                               │ Render Task (30fps) │
                                               │  drain UserAction   │
                                               │  drain TuiMsg       │
                                               │  update(state, msg) │
                                               │  terminal.draw()    │
                                               └─────────────────────┘
```

### 3 Tasks Tokio

1. **Input Task** — `crossterm::event::EventStream` -> `mpsc::Sender<UserAction>`. Injetavel em testes (substitui por canal controlado).
2. **Event Task** — `broadcast::Receiver<DomainEvent>` -> batching 16ms via `tokio::time::interval` -> `mpsc::Sender<TuiMsg>`. Trata `RecvError::Lagged(n)` com `TuiMsg::EventsLost(n)`.
3. **Render Task** — tick 33ms (30fps). Drena ambos os mpsc. Chama `update(state, msg) -> state` (puro). Chama `terminal.draw(|f| view(f, &state))`.

### Broadcast Bridge (nova API no EventBus)

```rust
// crates/theo-agent-runtime/src/event_bus.rs

pub fn subscribe_broadcast(&self, capacity: usize) -> broadcast::Receiver<DomainEvent> {
    let (tx, rx) = tokio::sync::broadcast::channel(capacity);
    self.subscribe(Arc::new(BroadcastListener { tx }));
    rx
}

struct BroadcastListener {
    tx: broadcast::Sender<DomainEvent>,
}

impl EventListener for BroadcastListener {
    fn on_event(&self, event: &DomainEvent) {
        let _ = self.tx.send(event.clone());
    }
}
```

- Capacity: 1024 (15x o burst estimado de 400 eventos/s com batching 16ms)
- `let _ = self.tx.send(...)` — ignora SendError quando nao ha receivers (nao bloqueia)
- Publish continua sync — agent loop nao e afetado

### Novo DomainEvent: ToolCallStdoutDelta

```rust
// crates/theo-domain/src/event.rs

pub enum EventType {
    // ... existentes ...

    // Streaming de stdout/stderr parcial de tool calls
    ToolCallStdoutDelta,
}
```

Payload: `{"line": "Compiling theo v0.1.0", "stream": "stdout", "tool_name": "bash"}`

**Fronteira critica:** emitido SOMENTE por `theo-agent-runtime` (ToolCallManager), NUNCA por `theo-tooling`. O BashTool envia linhas via `ctx.stdout_tx` (campo do ToolContext). O ToolCallManager cria o canal, injeta o tx no ToolContext, e spawna task que drena o rx e converte cada linha em ToolCallStdoutDelta via EventBus. O Tool trait NAO muda.

### Stdout Streaming no BashTool (via ToolContext)

**IMPORTANTE:** O Tool trait (`Tool::execute() -> Result<ToolOutput, ToolError>`) NAO muda.
O streaming usa canal lateral no `ToolContext`:

```rust
// crates/theo-domain/src/tool.rs — campo novo no struct existente
pub struct ToolContext {
    // ... campos existentes ...
    pub stdout_tx: Option<tokio::sync::mpsc::Sender<String>>,
}
```

O BashTool verifica `ctx.stdout_tx` e envia linhas se presente:

```rust
// crates/theo-tooling/src/bash/mod.rs (conceitual)
let mut child = Command::new("sh")
    .arg("-c").arg(&command)
    .stdout(Stdio::piped()).stderr(Stdio::piped())
    .spawn()?;

if let Some(tx) = &ctx.stdout_tx {
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let mut lines = stdout.lines();
    while let Some(line) = lines.next_line().await? {
        tx.send(line).await.ok(); // envia para ToolCallManager
    }
}
let output = child.wait_with_output().await?;
// Retorna ToolOutput NORMAL (contrato intacto)
```

O ToolCallManager cria o canal, injeta `stdout_tx` no `ToolContext`, e spawna task
que drena o receiver e converte cada linha em `ToolCallStdoutDelta` via EventBus.

Path com sandbox: thread dedicada drenando pipe via `std::io::BufRead`, enviando por `std::sync::mpsc` -> bridge para `tokio::sync::mpsc` no ToolCallManager.

**Guards de seguranca (obrigatorios):**
- Limite de bytes por linha: 10KB (truncar)
- Rate limit: max 100 linhas/segundo
- Limite total por tool call: 1MB (depois para de publicar, log silenciado)
- Sanitizar stdout via nova funcao `sanitize_stdout_line()` (redacta valores de env vars sensiveis usando `ALWAYS_STRIPPED_ENV_PREFIXES`)
- Sequencing: ToolCallQueued DEVE ser publicado ANTES do primeiro StdoutDelta

### Layout

```
┌─ theo · agent · gpt-4o · main · ~/project ──── 12.4k tok · $0.08 ─┐
│                                                                     │
│  user > arrume o bug de scoring                                     │
│                                                                     │
│  ▸ assistant                                                        │
│    Vou comecar lendo o modulo de scoring.                           │
│                                                                     │
│  ┌─ bash · cargo test -p theo-engine-retrieval ──── 3.2s... ──┐    │
│  │ running 14 tests                                            │    │
│  │ test scoring::tests::rrf_basic ... ok                       │    │
│  │ test scoring::tests::rrf_empty ... FAILED                   │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                     │
│  ┌─ read · crates/.../scoring.rs ──── 0.2s ✓ ─────────────────┐    │
│  │ 124 lines                                                    │    │
│  └──────────────────────────────────────────────────────────────┘    │
│                                                                     │
├─────────────────────────────────────────────────────────────────────┤
│ > _                                                                 │
│   Digite uma tarefa ou /help                                        │
├─────────────────────────────────────────────────────────────────────┤
│ AGENT │ LOCATE→EDIT │ 3/40 iter │ 2 tools │ ? ajuda  Ctrl+C para   │
└─────────────────────────────────────────────────────────────────────┘
```

- **Header (1 linha):** app, modo, modelo, branch, cwd, tokens, custo
- **Transcript (flex, scrollavel):** mensagens user/assistant + tool cards inline
- **Input (auto-grow, max 10 linhas):** tui-textarea com placeholder contextual
- **StatusLine (1 linha):** modo, fase state machine, iteracao, tools rodando, keybind hints

---

## Plano de 7 Fases

Cada fase exige `/meeting` proprio. Aprovacao de F1 nao e blank check.

### F1 — Shell Vivo

**Entregas:**
1. Broadcast bridge `subscribe_broadcast(capacity)` no EventBus
2. Fix ToolCallQueued: card aparece imediatamente com throbber
3. Timer ao vivo no tool card (100ms tick)
4. Streaming com cursor piscante (ContentDelta)
5. StatusLine permanente: modo/modelo/tokens/iteracao/keybind hints
6. Output parcial de BashTool em tempo real (ToolCallStdoutDelta) — cortavel para F2
7. Placeholder inteligente no input

**Criterio demo-ready:** abrir `theo --tui`, digitar tarefa, ver tool cards aparecendo ao vivo com timer, texto do LLM streamando com cursor, StatusLine atualizada.

**Flag:** `--tui` (experimental, warning no stderr)

### F2 — Navegacao e Historia

1. Scrollback ilimitado com paginacao (PgUp/PgDn, j/k)
2. Search inline com / (fuzzy sobre texto do transcript)
3. Session picker na inicializacao (lista com data + preview)
4. Help overlay com ? (tabela de keybinds)
5. Output mutante de bash (parser de stdout: "Compilando 47/200...")

### F3 — Controle do Agente

1. Model switcher em runtime (Ctrl+M — lista de modelos do provider)
2. Mode switcher visual (agent/plan/ask) com indicador
3. Copy de bloco com y (OSC52 para clipboard do terminal host)
4. Interrupt com contexto (Ctrl+C resume o que foi feito + "continuar?")
5. Edicao do ultimo prompt (Ctrl+Up)

### F4 — Decision Control Plane Visivel [requer ADR-004]

**NOTA:** O runtime atual so tem CapabilityGate (check binario allow/deny).
NAO existe handshake interativo. F4 requer ADR-004 (Interactive Approval Gate)
definindo: trait ApprovalGate, protocolo oneshot, integracao com CapabilityGate.

1. ADR-004: Interactive Approval Gate (ANTES de qualquer codigo)
2. Implementar ApprovalGate trait + integracao no ToolCallManager
3. Approval modal na TUI
4. Diff preview inline para Edit/Write (similar crate)
5. Timeline de causalidade (cadeia de raciocinio entre tool calls)
6. Undo last tool (U — snapshot-based, NAO git checkout, passa pelo DCP)
6. Dissolve/sweep tachyonfx comunicativos em tool completion

### F5 — Painel de Tarefas e Contexto

1. Sidebar direita (toggle com Tab): TodoList ao vivo
2. GRAPHCTX status: quais arquivos foram indexados, progresso de build
3. Phase indicator LOCATE/EDIT/VERIFY/DONE visivel
4. Sub-agent spawning visualizado (arvore de agentes)
5. Budget visual (barra de progresso de tokens)

### F6 — Multi-Sessao e Workspace

1. Tabs de sessao (Ctrl+T nova, Ctrl+W fecha, Ctrl+1..9 navega)
2. Split vertical para comparar dois contextos
3. Export sessao como markdown (Ctrl+S)
4. Busca global em historico de sessoes
5. Nome de sessao editavel

### F7 — Polimento e Superioridade (TUI vira default)

1. Temas (dark/light/high-contrast + opaline 20 presets)
2. Configuracao persistente via `~/.config/theo/tui.toml`
3. Keybinds configuraveis
4. Notificacao de tarefa concluida via notificacao do sistema
5. Benchmark de latencia de render (< 16ms/frame)
6. REPL rustyline movido para `--legacy`

---

## Diferenciais sobre concorrentes

Features que nenhum dos 4 (Claude Code, Codex, Gemini CLI, OpenCode) oferece:

1. **Governance approval inline** com risk score do Decision Control Plane — tool calls de alto risco mostram impacto antes de executar.
2. **Timeline de causalidade** — mostra "por que" de cada tool call, nao so "o que". Ex: "LLM usou Edit porque grep retornou 3 matches em foo.rs".
3. **Interrupt com contexto** — Ctrl+C nao mata, resume o que foi feito e oferece "continuar de onde parei?".
4. **Undo last tool** — U reverte ultima operacao via snapshot (conteudo original salvo em `~/.config/theo/undo/`) sem sair da TUI, passando pelo DCP. NAO usa git checkout (destrutivo).
5. **Painel GRAPHCTX ao vivo** — mostra arquivos no contexto do agente + por que entraram.
6. **Fase da state machine visivel** (LOCATE/EDIT/VERIFY/DONE) — feedback do promise gate.
7. **Replay deterministico de sessao** a partir do event log persistido.

---

## Divida tecnica pre-existente

`apps/theo-cli/Cargo.toml` importa diretamente `theo-agent-runtime`, `theo-tooling`, `theo-infra-auth`, `theo-infra-llm`, `theo-domain`, violando a regra "apps so importam theo-application e theo-api-contracts" (`.claude/rules/architecture.md`).

Esta divida e pre-existente (ADR-001 define a regra, o CLI nunca migrou). A Fase 1 perpetua o acoplamento (chama `subscribe_broadcast()` direto no `EventBus`). Plano de migracao: ate F7 (quando TUI vira default), re-expor as APIs necessarias via `theo-application`.

---

## Riscos e mitigacoes

| # | Risco | Severidade | Mitigacao |
|---|---|---|---|
| 1 | Backpressure silenciosa no broadcast | HIGH | Tratar `RecvError::Lagged` com marcador visual. Capacity 1024. |
| 2 | Scope creep na F1 (7 entregas) | MEDIUM | Timebox 2 semanas. Ordem de corte: StatusLine -> stdout streaming -> placeholder. |
| 3 | Stdout streaming + sandbox (SandboxExecutor e sync) | HIGH | Thread dedicada com pipe. Se atrasar, cortar para F2. |
| 4 | Cross-terminal compat (tmux, Alacritty, WezTerm, etc) | MEDIUM | Matrix de terminais minima documentada e testada na F1. |
| 5 | Coexistencia rustyline + ratatui (raw mode) | LOW | Caminhos mutuamente exclusivos via flag --tui. |
| 6 | Hot-swap LlmClient (F3) race condition | HIGH | Swap so entre iteracoes. Teste obrigatorio: swap durante chamada = NO-OP. |
| 7 | UndoTool bypass de governance | HIGH | UndoTool registrada no ToolRegistry, passa pelo DCP. Nao e atalho direto. |
| 8 | Supply chain (deps novas) | MEDIUM | cargo-audit em CI. Cada dep avaliada em meeting da fase. |
| 9 | Performance render com transcript grande | MEDIUM | Viewport virtual (so renderiza linhas visiveis). |
| 10 | referencias/ contaminando cargo workspace | LOW | Manter em .gitignore ou confirmar exclusao do workspace. |

---

## Testes

### Broadcast bridge (theo-agent-runtime)
- `subscribe_broadcast_receives_events` — publish N, recv N
- `subscribe_broadcast_lagged_returns_err` — buffer cheio, recv Lagged
- `subscribe_broadcast_drop_receiver_no_crash` — drop rx, publish continua
- `existing_listeners_unaffected_by_broadcast` — CapturingListener antes/depois

### TUI update fn (theo-cli)
- `update_quit_sets_should_quit` — Msg::Quit -> should_quit=true
- `update_new_event_appends_to_buffer` — Msg::Event -> buffer cresce
- `update_resize_updates_dimensions` — Msg::Resize(w,h) -> state atualiza
- `update_input_updates_text` — Msg::Input -> texto muda
- `update_submit_clears_input` — Msg::Submit -> input limpo

### TUI view snapshot (theo-cli)
- Snapshot 80x24 com TestBackend (terminal padrao)
- Snapshot 200x50 com TestBackend (terminal largo)

### CliRenderer fix (theo-cli)
- `tool_call_queued_emits_output` — ToolCallQueued gera saida (nao suprime)
- `tool_call_completed_bash_shows_command` — regressao do path existente

### BashTool streaming (theo-tooling)
- Via MockBashExecutor: emite linhas pre-determinadas, verifica eventos
- Executor extraido por trait (DIP) para injecao

### Regressao REPL
- Testes de session persistence (6 existentes) continuam passando
- Testes de parsing clap para proteger flag --tui vs caminho default

---

## Ordem de implementacao (Fase 1)

1. Este ADR (docs/adr/003-tui-architecture.md)
2. `theo-domain/src/event.rs` — adicionar ToolCallStdoutDelta + ALL_EVENT_TYPES + Display + testes
3. `theo-tooling/src/bash/mod.rs` — extrair executor por trait (DIP), implementar streaming de stdout via channel
4. `theo-agent-runtime/src/event_bus.rs` — subscribe_broadcast() + BroadcastListener + 4 testes
5. `theo-agent-runtime/src/tool_call_manager.rs` — consumir channel do BashTool, emitir ToolCallStdoutDelta
6. `apps/theo-cli/src/renderer.rs` — fix ToolCallQueued + testes
7. `apps/theo-cli/Cargo.toml` — adicionar ratatui, crossterm, tui-textarea + workspace deps
8. `apps/theo-cli/src/tui/` — mod.rs, app.rs (State+Msg+update), view.rs, input.rs, events.rs, widgets/
9. `apps/theo-cli/src/main.rs` — flag --tui despacha para tui::run()
