# Meeting — 2026-04-14 23:30

## Proposta

Reescrita incremental do theo-cli para TUI ratatui visando experiencia IGUAL OU SUPERIOR a Claude Code, Gemini CLI, Codex e OpenCode. Plano de 7 fases com coexistencia REPL legado. Fase 1 "Shell Vivo" como primeira entrega demonstravel.

## Participantes

- **governance** (Principal Engineer) — veto absoluto, fronteiras arquiteturais
- **runtime** (Staff AI Systems Engineer) — EventBus bridge, async, agent loop impact
- **qa** (QA Staff Engineer) — testabilidade, cobertura, regressao
- **frontend** (Senior UX Engineer) — experiencia do usuario, microinteracoes, barra competitiva
- **tooling** (Systems Engineer) — seguranca de deps, supply chain, stdout streaming

## Rodada 1 (escopo original)

Plano de 8 fases com F1 = esqueleto estatico (zero valor visual). Frontend avaliou score 52/100, ~75% da barra Claude Code. 6 gaps criticos identificados. Tooling REJECT por rat-salsa (supply chain risk).

## Rodada 2 (reconvocacao com barra explicita)

Frontend redesenhou o plano para 7 fases comprimidas. F1 "Shell Vivo" com valor demonstravel imediato. rat-salsa removido. 6 gaps realocados em fases 1-3. 3 diferenciais novos adicionados.

## Analises

### Governance
- APPROVE_WITH_CONDITIONS (15 condicoes)
- ToolCallStdoutDelta aceito como novo DomainEvent (revisao da posicao anterior)
- Fronteiras respeitadas: tooling retorna channel, runtime converte em evento
- UndoTool (F4) deve ser tool registrada passando pelo DCP
- Hot-swap LlmClient (F3) so entre iteracoes do agent loop
- Timebox F1 em 2 semanas

### Runtime
- APPROVE parcial: risk_level HIGH apenas no stdout streaming
- ToolCallQueued JA e publicado pelo EventBus (confirmado em tool_call_manager.rs:76-82) — bug e no renderer
- BashTool usa Command::output() sincrono — streaming requer refatoracao do executor
- Recomenda separar stdout streaming como sub-item isolavel
- Broadcast bridge subscribe_broadcast() e LOW risk

### QA
- APPROVE_WITH_CONDITIONS (7 condicoes de teste)
- renderer.rs tem ZERO testes hoje — precisa de CapturingRenderer + 5 testes minimos
- Timer/cursor testaveis com tokio::time::pause + advance
- ToolCallStdoutDelta testavel via MockBashExecutor
- Executor precisa ser extraido por trait (DIP) para mockabilidade
- Ordem de implementacao: domain types -> executor trait -> renderer fix -> TUI

### Frontend
- Score revisado: 88/100 (90+ apos F3, 95+ apos F4)
- F1 "Shell Vivo" demonstravel: tool cards ao vivo + timer + cursor piscante + StatusLine
- 3 diferenciais novos: Timeline de causalidade, Interrupt com contexto, Undo last tool
- Gap remanescente: fisica (CLI puro vs integrado ao editor) — irresolvel
- tachyonfx: usar em F3/F4 para comunicar informacao, nunca decorativo

### Tooling
- APPROVE_WITH_CONDITIONS (sem rat-salsa)
- Stdout streaming introduz riscos: buffer overflow, credential leak, race condition
- Mitigacoes obrigatorias: 10KB/linha, 100 linhas/s rate limit, 1MB total, sanitizacao
- bwrap nao bloqueia pipes — streaming viavel dentro do sandbox
- Stack F1 aprovada: ratatui 0.29 + crossterm 0.28 + tui-textarea

## Conflitos

1. **Stdout streaming na F1 vs meeting separado**: frontend exige (diferencial visceral) vs runtime sinaliza HIGH risk. Resolucao: entra na F1 como item paralelo cortavel para F2 se SandboxExecutor trait atrasar.
2. **Scope creep F1 (7 entregas)**: governance preocupado com timebox. Resolucao: ordem de corte definida (StatusLine -> stdout streaming -> placeholder) sem perder "wow" core.
3. **ToolCallStdoutDelta em DomainEvent na F1**: governance revisou posicao anterior. Aceito por seguir padrao identico a ContentDelta/ReasoningDelta.

## Veredito

**APPROVED**

## Plano de 7 Fases Registrado (contexto — cada fase exige /meeting proprio)

### F1 — Shell Vivo
1. Broadcast bridge subscribe_broadcast() no EventBus
2. Fix ToolCallQueued: card aparece imediatamente com spinner
3. Timer ao vivo no tool card (100ms tick)
4. Streaming com cursor piscante (ContentDelta)
5. StatusLine permanente: modo/modelo/tokens/iteracao/keybind hint
6. Output parcial de BashTool em tempo real (ToolCallStdoutDelta) — cortavel para F2
7. Placeholder inteligente no input

### F2 — Navegacao e Historia
1. Scrollback ilimitado com paginacao
2. Search inline com / (fuzzy)
3. Session picker na inicializacao
4. Help overlay com ?
5. Output mutante de bash (parser de stdout)

### F3 — Controle do Agente
1. Model switcher em runtime (Ctrl+M)
2. Mode switcher visual (agent/plan/ask)
3. Copy de bloco com y (OSC52)
4. Interrupt com contexto (Ctrl+C resume + "continuar?")
5. Edicao do ultimo prompt (Ctrl+Up)

### F4 — Decision Control Plane Visivel
1. Painel de aprovacao para tool calls de alto risco
2. Impact score visivel (verde/amarelo/vermelho)
3. Diff preview inline para Edit/Write
4. Timeline de causalidade (cadeia de raciocinio entre tool calls)
5. Undo last tool (U — via UndoTool registrada no DCP)

### F5 — Painel de Tarefas e Contexto
1. Sidebar direita: TodoList ao vivo
2. GRAPHCTX status: arquivos indexados, progresso
3. Phase indicator LOCATE/EDIT/VERIFY/DONE
4. Sub-agent spawning visualizado
5. Budget visual (barra de progresso de tokens)

### F6 — Multi-Sessao e Workspace
1. Tabs de sessao (Ctrl+T/W/1..9)
2. Split vertical
3. Export sessao como markdown
4. Busca global em historico
5. Nome de sessao editavel

### F7 — Polimento e Superioridade
1. Temas (dark/light/high-contrast + opaline 20 presets)
2. Configuracao persistente via ~/.config/theo/tui.toml
3. Keybinds configuraveis
4. Notificacao de tarefa concluida (notify-send)
5. Benchmark de latencia de render (< 16ms/frame)

## Escopo Aprovado (Fase 1 apenas)

### Arquivos que PODEM ser alterados:
- `crates/theo-domain/src/event.rs` — adicionar ToolCallStdoutDelta ao enum EventType
- `crates/theo-agent-runtime/src/event_bus.rs` — adicionar subscribe_broadcast()
- `crates/theo-agent-runtime/src/tool_call_manager.rs` — emitir ToolCallStdoutDelta a partir de channel do BashTool
- `crates/theo-tooling/src/bash/mod.rs` — refatorar para retornar channel de stdout (sem importar EventBus)
- `crates/theo-tooling/src/sandbox/executor.rs` — adaptar para streaming (se viavel nesta fase)
- `apps/theo-cli/Cargo.toml` — adicionar ratatui 0.29, crossterm 0.28, tui-textarea
- `apps/theo-cli/src/main.rs` — adicionar flag --tui
- `apps/theo-cli/src/renderer.rs` — fix do bug ToolCallQueued suprimido
- `apps/theo-cli/src/tui/` (novo modulo) — mod.rs, app.rs, view.rs, input.rs, events.rs, widgets/
- `Cargo.toml` (root) — adicionar workspace deps ratatui, crossterm, tui-textarea
- `docs/adr/003-tui-architecture.md` (novo) — ADR obrigatorio ANTES de codigo

### Arquivos que NAO podem ser alterados:
- Qualquer crate nao listado acima
- theo-governance (sem mudancas nesta fase)
- theo-application (sem mudancas nesta fase)
- apps/theo-desktop ou apps/theo-ui

## Condicoes (merge-blocking)

### Governance (15)
1. ADR docs/adr/003-tui-architecture.md ANTES de qualquer codigo
2. ToolCallStdoutDelta emitido SOMENTE por theo-agent-runtime, NUNCA por theo-tooling
3. BashTool retorna channel/stream — fronteira tooling->runtime preservada
4. Zero unwrap() em codigo TUI de producao
5. Flag --tui marcada como experimental (warning no stderr)
6. Capacity do broadcast justificada numericamente (sugestao: 1024)
7. Batching 16ms com tokio::time::interval (nao sleep em loop)
8. Tratamento explicito de RecvError::Lagged com marcador visual
9. referencias/ em .gitignore ou fora do workspace Cargo
10. Cada fase F2..F7 exige /meeting proprio
11. Divida arquitetural theo-cli Cargo.toml registrada no ADR
12. F1 timeboxada em 2 semanas — cortar StatusLine/placeholder se estourar
13. Hot-swap LlmClient (F3) so entre iteracoes — teste obrigatorio
14. UndoTool (F4) registrada no ToolRegistry, passa pelo DCP
15. ProviderSwitched evento (F3) precisa meeting proprio

### QA (7)
1. EventType::ToolCallStdoutDelta adicionado ao enum + ALL_EVENT_TYPES
2. CapturingRenderer criado em #[cfg(test)] de renderer.rs
3. Fix ToolCallQueued acompanhado de pelo menos 2 testes novos
4. SpinnerState e CursorState extraidos como tipos proprios testaveis
5. BashTool com executor injetavel por trait (DIP) para MockBashExecutor
6. Testes de parsing clap antes de qualquer mudanca em main.rs
7. 6 testes de session persistence continuam passando sem modificacao

### Runtime (3)
1. subscribe_broadcast(capacity: usize) com capacity exposto como parametro
2. 4 testes de bridge: publish-recv, lagging, drop-sem-panic, listeners existentes intactos
3. RecvError::Lagged tratado explicitamente na task TUI (nao fatal)

### Tooling (4 — para stdout streaming)
1. Limite de bytes por linha: 10KB (truncar)
2. Rate limit: max 100 linhas/segundo
3. Limite total por tool call: 1MB
4. Sanitizar stdout com mesmo env sanitizer antes de publicar

### Ordem de implementacao obrigatoria
1. ADR (F0)
2. Domain types (ToolCallStdoutDelta)
3. Executor trait DIP (theo-tooling)
4. Broadcast bridge (theo-agent-runtime)
5. Renderer fix + testes (theo-cli)
6. Timer/cursor types
7. TUI integration (ratatui)
