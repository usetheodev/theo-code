# Plano: Implementar TODAS as Licoes do Pi-Mono no Theo Code

## Contexto

Pi-mono (badlogic/pi-mono) é um coding agent TypeScript com patterns avançados em agent runtime, LLM abstraction, session management, extensions e TUI. Theo Code é nosso AI coding assistant em Rust. Este plano implementa TODOS os patterns avançados identificados, respeitando a arquitetura Rust, ADR-002 (append-only CLI), regras de dependência e TDD obrigatório.

**Referência base:** `referencias/pi-mono/`

---

## P0 — Core Runtime (5 tasks)

### Task 1: Steering & Follow-up Message Queues
**Descrição:** Duas filas async no agent loop — steering (injeta mid-run entre turns) e follow-up (checa quando agent convergiria).

**Critérios de aceite:**
- `AgentRunEngine` aceita dois closures opcionais: `get_steering_messages` e `get_follow_up_messages`
- Steering checado após cada batch de tool execution, injetado como user messages antes do próximo LLM call
- Follow-up checado quando agent convergiria (sem tool calls + sem steering); se presente, continua
- Ambas retornam `Vec<Message>`, nunca panic (vec vazio em falha)
- Comportamento existente inalterado quando queues não providas
- Testes: steering injection mid-run, follow-up extends converging run, empty queues = no-op

**DoD:** `cargo test -p theo-agent-runtime` passa. Teste de integração demonstra REPL typing durante agent run.

**Arquivos:**
- `crates/theo-agent-runtime/src/run_engine.rs` — adicionar queue checking ao main loop
- `crates/theo-agent-runtime/src/config.rs` — closures opcionais no `AgentConfig`
- `apps/theo-cli/src/repl.rs` — wire REPL input às queues

**Ref pi-mono:** `packages/agent/src/agent-loop.ts:165-229`, `packages/agent/src/types.ts:163-183`
**Complexidade:** L | **Deps:** Nenhuma

---

### Task 2: Dual-Message Abstraction (AgentMessage)
**Descrição:** Enum `AgentMessage` que wrapa LLM messages + UI-only messages. `convert_to_llm()` filtra na fronteira do LLM.

**Critérios de aceite:**
- `AgentMessage` enum em `theo-domain`: `Llm(Message)`, `CompactionSummary { summary, tokens_before }`, `BranchSummary { summary, from_id }`, `BashExecution { command, output, exit_code }`, `Custom { custom_type, content, display }`
- `fn convert_to_llm(messages: &[AgentMessage]) -> Vec<Message>` mapeia/filtra cada variant
- `AgentRunEngine` usa `Vec<AgentMessage>` internamente, chama `convert_to_llm` só na fronteira LLM
- Compaction summary vira `AgentMessage::CompactionSummary` (não mais magic-prefixed user message)
- Property test: `convert_to_llm` nunca produz sequências inválidas

**DoD:** `cargo test -p theo-domain && cargo test -p theo-agent-runtime` passam.

**Arquivos:**
- `crates/theo-domain/src/agent_message.rs` — NOVO: enum + convert_to_llm
- `crates/theo-agent-runtime/src/run_engine.rs` — substituir `Vec<Message>` por `Vec<AgentMessage>`
- `crates/theo-agent-runtime/src/compaction.rs` — usar `AgentMessage::CompactionSummary`

**Ref pi-mono:** `packages/agent/src/types.ts:236-246`, `packages/coding-agent/src/core/messages.ts:1-195`, `packages/agent/src/agent-loop.ts:247-258`
**Complexidade:** XL | **Deps:** Nenhuma

---

### Task 3: Context Overflow Detection e Recovery Reativa
**Descrição:** Detectar erros de context overflow do LLM via regex por provider e compactar reativamente.

**Critérios de aceite:**
- `ContextOverflowDetector` em `theo-infra-llm` com regex patterns por provider (OpenAI: `context_length_exceeded`, Anthropic: `prompt is too long`, Google: `exceeds the context window`, genérico)
- `LlmError::ContextOverflow { provider, estimated_tokens }` novo variant
- `AgentRunEngine` captura `ContextOverflow`, trigga compaction emergencial (50% target vs 80%), retry 1x
- Se retry falha com overflow, abort com mensagem clara
- `DomainEvent::ContextOverflowRecovery` publicado
- Testes: regex match para OpenAI, Anthropic, Google; recovery flow

**DoD:** `cargo test -p theo-infra-llm && cargo test -p theo-agent-runtime` passam.

**Arquivos:**
- `crates/theo-infra-llm/src/overflow.rs` — NOVO: detector
- `crates/theo-infra-llm/src/error.rs` — novo variant
- `crates/theo-agent-runtime/src/run_engine.rs` — catch overflow + retry
- `crates/theo-domain/src/event.rs` — novo event type

**Ref pi-mono:** `packages/ai/src/utils/overflow.ts` (inteiro)
**Complexidade:** M | **Deps:** Nenhuma

---

### Task 4: Cross-Provider Message Transformation
**Descrição:** Normalizar histórico ao trocar de modelo mid-session: tool call IDs, thinking blocks, orphaned tool calls.

**Critérios de aceite:**
- `transform_messages(messages: &[Message], target: &ModelInfo) -> Vec<Message>` em `theo-infra-llm`
- Tool call IDs > 64 chars ou com caracteres inválidos: encurtados via SHA-256 prefix determinístico
- Thinking blocks de modelo diferente: convertidos para text content; mesmo modelo: preservados
- Orphaned tool calls (assistant pede tool sem result correspondente): synthetic error result inserido
- Error/aborted assistant messages stripados do replay
- Testes para cada transformação

**DoD:** `cargo test -p theo-infra-llm` passa.

**Arquivos:**
- `crates/theo-infra-llm/src/transform.rs` — NOVO
- `crates/theo-infra-llm/src/types.rs` — adicionar `ModelInfo { provider, model_id }`

**Ref pi-mono:** `packages/ai/src/providers/transform-messages.ts` (inteiro)
**Complexidade:** L | **Deps:** Nenhuma

---

### Task 5: Parallel Tool Execution com Order Preservation
**Descrição:** Refatorar execução de tools: preparar sequencialmente (validação, hooks), executar concorrentemente, emitir resultados na ordem original.

**Critérios de aceite:**
- `ToolExecutionMode` enum: `Sequential` | `Parallel`
- Em modo Parallel: todos tool calls de um LLM response preparados sequencialmente, executados via `tokio::join_all`
- Resultados coletados e adicionados na ordem original (não ordem de completion)
- Meta-tools (`done`, `subagent`, `batch`, `skill`) sempre sequenciais
- `batch` meta-tool preservado como açúcar sintático sobre o mesmo mecanismo
- Testes: parallel mais rápido que sequential para I/O-bound; order preservation

**DoD:** `cargo test -p theo-agent-runtime` passa.

**Arquivos:**
- `crates/theo-agent-runtime/src/run_engine.rs` — refatorar tool execution loop
- `crates/theo-agent-runtime/src/config.rs` — adicionar `tool_execution_mode`

**Ref pi-mono:** `packages/agent/src/agent-loop.ts:390-438`
**Complexidade:** L | **Deps:** Nenhuma

---

## P1 — LLM Layer (5 tasks)

### Task 6: Cost/Dollar Tracking
**Descrição:** Pricing por provider, cálculo de custo por request.

**Critérios de aceite:**
- `ModelCost { input_per_million, output_per_million, cache_read_per_million, cache_write_per_million }` em domain
- `ModelRegistry` mapeia (provider, model_id) → `ModelCost` para modelos conhecidos
- `CostBreakdown { input, output, cache_read, cache_write, total }` em `Usage`
- `MetricsCollector` acumula custo da sessão; `RuntimeMetrics.total_cost_usd`
- Status line mostra custo acumulado quando > $0.001
- Testes: cálculo correto para modelos conhecidos, zero para desconhecidos

**DoD:** `cargo test -p theo-infra-llm && cargo test -p theo-agent-runtime` passam.

**Arquivos:**
- `crates/theo-domain/src/budget.rs` — `ModelCost`, `CostBreakdown`
- `crates/theo-infra-llm/src/types.rs` — estender `Usage`
- `crates/theo-agent-runtime/src/metrics.rs` — acumular custo
- `apps/theo-cli/src/status_line/format.rs` — exibir custo

**Ref pi-mono:** `packages/ai/src/types.ts:167-179`, `packages/ai/src/models.ts:39-46`
**Complexidade:** M | **Deps:** Nenhuma

---

### Task 7: Dynamic API Key Resolution
**Descrição:** API keys resolvidas per-request via callback async (OAuth tokens que expiram).

**Critérios de aceite:**
- `ApiKeyResolver` trait: `async fn resolve(&self, provider: &str) -> Option<String>`
- `AgentConfig` aceita `Option<Arc<dyn ApiKeyResolver>>`
- `LlmClient` chama resolver antes de cada request, fallback para key estática
- Default: lê de env vars (comportamento atual)
- Testes: resolver chamado a cada vez, fallback funciona, None usa estática

**DoD:** `cargo test -p theo-infra-llm && cargo test -p theo-agent-runtime` passam.

**Arquivos:**
- `crates/theo-infra-llm/src/client.rs` — aceitar resolver
- `crates/theo-agent-runtime/src/config.rs` — adicionar resolver

**Ref pi-mono:** `packages/agent/src/types.ts:152-157`, `packages/agent/src/agent-loop.ts:265-268`
**Complexidade:** S | **Deps:** Nenhuma

---

### Task 8: Streaming JSON Parsing
**Descrição:** Best-effort partial JSON para tool call arguments durante streaming.

**Critérios de aceite:**
- `PartialJsonParser` que aceita chunks incrementais e tenta parse best-effort
- `StreamCollector::partial_tool_args(index) -> Option<serde_json::Value>`
- `StreamDelta::ToolCallDelta` carrega opcional `partial_parsed: Option<Value>`
- CLI pode exibir file path de tool call parcial antes do response completo
- Testes: JSON incompleto retorna parcial, completo retorna full, malformed retorna None

**DoD:** `cargo test -p theo-infra-llm` passa.

**Arquivos:**
- `crates/theo-infra-llm/src/partial_json.rs` — NOVO
- `crates/theo-infra-llm/src/stream.rs` — integrar parser no `StreamCollector`

**Ref pi-mono:** `packages/ai/src/utils/json-parse.ts`
**Complexidade:** M | **Deps:** Nenhuma

---

### Task 9: Enhanced Faux Provider
**Descrição:** Mock completo com streaming simulation, token estimation, prompt cache simulation.

**Critérios de aceite:**
- `FauxProvider` suporta `chat()` e `chat_stream()` com streaming realista (tokens/sec configurável)
- Response factory: `Fn(&ChatRequest) -> ChatResponse` para respostas dinâmicas
- Estimativa de usage baseada em content length
- Prompt cache simulation com session_id
- Abort support via `CancellationToken`
- `MockLlmProvider` preservado como thin wrapper

**DoD:** `cargo test -p theo-infra-llm` passa.

**Arquivos:**
- `crates/theo-infra-llm/src/mock.rs` — reescrever como faux provider

**Ref pi-mono:** `packages/ai/src/providers/faux.ts` (inteiro)
**Complexidade:** L | **Deps:** Nenhuma

---

### Task 10: Lazy Provider Loading
**Descrição:** Providers carregados sob demanda no primeiro uso.

**Critérios de aceite:**
- `ProviderRegistry` usa `OnceCell`/`LazyLock` por provider entry
- Inicialização (HTTP client, headers) roda só no primeiro `chat()`/`chat_stream()`
- Melhoria de startup mensurável (benchmark)
- Testes: provider não inicializado até primeiro call, múltiplos calls usam mesma instância

**DoD:** `cargo test -p theo-infra-llm` passa. Benchmark mostra melhoria.

**Arquivos:**
- `crates/theo-infra-llm/src/provider/registry.rs` — lazy init
- `crates/theo-infra-llm/src/client.rs` — defer creation

**Ref pi-mono:** `packages/ai/src/providers/register-builtins.ts`
**Complexidade:** S | **Deps:** Nenhuma

---

## P2 — Session & Persistence (3 tasks)

### Task 11: Session Tree Structure (JSONL com id/parentId)
**Descrição:** Substituir snapshots flat por JSONL tree-structured com branching.

**Critérios de aceite:**
- `SessionManager` em `theo-agent-runtime` com JSONL append-only
- Cada entry tem `id: String` (8-char hex) e `parent_id: Option<String>`
- `SessionHeader` como primeira linha: `{ type: "session", version: 1, id, timestamp, cwd }`
- Entry types: message, compaction, model_change, thinking_level_change, branch_summary, custom
- `build_session_context(entries, leaf_id) -> SessionContext` walk root→leaf
- `branch(from_id)` move leaf pointer sem modificar histórico
- `create_branched_session(leaf_id)` cria novo JSONL com root→leaf path
- Migração para sessões flat existentes
- Testes: append, tree traversal, branching, compaction, reload

**DoD:** `cargo test -p theo-agent-runtime` passa.

**Arquivos:**
- `crates/theo-agent-runtime/src/session_manager.rs` — NOVO (core)
- `crates/theo-domain/src/session.rs` — estender com `SessionEntryId`

**Ref pi-mono:** `packages/coding-agent/src/core/session-manager.ts` (inteiro)
**Complexidade:** XL | **Deps:** Task 2 (beneficial, não blocker)

---

### Task 12: Compaction Preserva Histórico
**Descrição:** Compaction NÃO descarta entries — JSONL mantém tudo, entry de compaction marca `first_kept_entry_id`.

**Critérios de aceite:**
- Entry de compaction armazena `summary`, `first_kept_entry_id`, `tokens_before`
- `build_session_context` produz: [compaction_summary, kept_messages, messages_after]
- Old messages NÃO deletados do JSONL
- Múltiplas compactions em path tratadas (só última ativa)
- Testes: preservação, context build pós-compaction

**DoD:** `cargo test -p theo-agent-runtime` passa.

**Arquivos:**
- `crates/theo-agent-runtime/src/compaction.rs` — entries ao invés de mutação in-place
- `crates/theo-agent-runtime/src/session_manager.rs` — handling no tree traversal

**Ref pi-mono:** `packages/coding-agent/src/core/session-manager.ts:869-888`
**Complexidade:** M | **Deps:** Task 11

---

### Task 13: Branch Summarization
**Descrição:** Ao branchar, gerar summary da branch abandonada como `BranchSummary` message.

**Critérios de aceite:**
- `SessionManager::branch_with_summary(from_id, summary)` appenda `branch_summary` entry
- `build_session_context` converte `branch_summary` em user message com `<summary>` tags
- Summary extrai: files edited, tools usados, erros encontrados
- Testes: summary aparece no context, conteúdo meaningful

**DoD:** `cargo test -p theo-agent-runtime` passa.

**Arquivos:**
- `crates/theo-agent-runtime/src/session_manager.rs` — adicionar `branch_with_summary`

**Ref pi-mono:** `packages/coding-agent/src/core/session-manager.ts:1140-1157`
**Complexidade:** S | **Deps:** Task 11, Task 2

---

## P3 — CLI Enhancements (4 tasks)

### Task 14: JSON Output Mode
**Descrição:** Flag `--output json` que emite JSONL no stdout.

**Critérios de aceite:**
- `--output json` produz JSONL no stdout
- Events: `agent_start`, `content_delta`, `tool_call_start`, `tool_call_end`, `agent_end`
- Zero ANSI codes no JSON output
- Stderr ainda recebe status messages
- Testes: parse JSON output, validar schema

**DoD:** `cargo test -p theo-cli` passa. Manual: `theo --output json "hello"` produz JSONL válido.

**Arquivos:**
- `apps/theo-cli/src/renderer.rs` — adicionar `JsonRenderer`
- `apps/theo-cli/src/main.rs` — flag `--output`

**Ref pi-mono:** `packages/coding-agent/src/modes/json-mode.ts`
**Complexidade:** M | **Deps:** Nenhuma

---

### Task 15: Event-Based Extension System
**Descrição:** Formalizar hooks em sistema de extensões tipado com lifecycle events.

**Critérios de aceite:**
- Extension trait: `before_agent_start`, `on_tool_call`, `on_tool_result`, `on_context_transform`, `on_input`
- Extensions carregadas de `.theo/extensions/` (shell scripts primeiro)
- `on_tool_call` pode retornar `Block { reason }` para prevenir execução
- `on_tool_result` pode modificar conteúdo do resultado
- `on_context_transform` pode injetar messages ou modificar contexto
- Backward compatible: `.theo/hooks/` scripts continuam funcionando
- Testes: extension bloqueia tool, modifica result, injeta context

**DoD:** `cargo test -p theo-agent-runtime` passa.

**Arquivos:**
- `crates/theo-agent-runtime/src/extension.rs` — NOVO: Extension trait
- `crates/theo-agent-runtime/src/hooks.rs` — adapter de hooks para extensions

**Ref pi-mono:** `packages/coding-agent/src/core/extensions/index.ts`
**Complexidade:** XL | **Deps:** Task 2

---

### Task 16: Model Cycling (Ctrl+P)
**Descrição:** Shortcut para ciclar entre modelos configurados durante sessão REPL.

**Critérios de aceite:**
- Ctrl+P abre seletor de modelos (lista configurada)
- Selecionar modelo troca LLM client para turns subsequentes
- Model change registrado no session history
- Status line atualiza com modelo atual
- Testes: switch registrado, calls subsequentes usam novo modelo

**DoD:** `cargo test -p theo-cli` passa.

**Arquivos:**
- `apps/theo-cli/src/input/` — keybinding
- `apps/theo-cli/src/repl.rs` — handle switch
- `apps/theo-cli/src/status_line/` — show model

**Ref pi-mono:** `packages/coding-agent/src/core/keybindings.ts`
**Complexidade:** M | **Deps:** Task 11 (para gravar model_change)

---

### Task 17: Session Management Commands
**Descrição:** Slash commands: `/sessions`, `/tree`, `/fork`, `/compact`.

**Critérios de aceite:**
- `/sessions` lista sessões recentes (timestamp, preview, count)
- `/tree` mostra conversation tree com branches e posição atual
- `/fork [entry_id]` cria branch do ponto especificado
- `/compact` trigga compaction manual
- Testes: parsing de comandos, listagem, rendering de tree

**DoD:** `cargo test -p theo-cli` passa.

**Arquivos:**
- `apps/theo-cli/src/commands/` — novos session commands
- `apps/theo-cli/src/repl.rs` — wire commands

**Ref pi-mono:** `packages/coding-agent/src/core/slash-commands.ts`
**Complexidade:** L | **Deps:** Task 11

---

## P4 — Tooling (3 tasks)

### Task 18: File Mutation Queue
**Descrição:** Serializar writes concorrentes ao mesmo arquivo via queue per-path.

**Critérios de aceite:**
- `FileMutationQueue` serializa writes ao mesmo path
- Paths diferentes procedem concorrentemente (per-path locking)
- Integrado ao `ToolContext`
- Timeout de 5s na aquisição (previne deadlock)
- Testes: writes concorrentes ao mesmo arquivo serializados, arquivos diferentes paralelos

**DoD:** `cargo test -p theo-tooling` passa.

**Arquivos:**
- `crates/theo-tooling/src/mutation_queue.rs` — NOVO
- `crates/theo-domain/src/tool.rs` — queue no `ToolContext`
- `crates/theo-tooling/src/edit/`, `src/write/` — usar queue

**Ref pi-mono:** Conceito de `withFileMutationQueue` no coding agent tools
**Complexidade:** M | **Deps:** Nenhuma

---

### Task 19: Streaming Tool Output
**Descrição:** Tools emitem resultados parciais durante execução via callback.

**Critérios de aceite:**
- `Tool::execute` estendido com `on_update: Option<Box<dyn Fn(PartialToolResult) + Send>>`
- `PartialToolResult { content: String, progress: Option<f32> }`
- `ToolCallManager` forward partial results como `DomainEvent::ToolCallProgress`
- CLI renderiza partial results em real-time (append-only, ADR-002)
- BashTool usa para streaming command output
- Testes: partial results emitidos, final result completo

**DoD:** `cargo test -p theo-tooling && cargo test -p theo-agent-runtime` passam.

**Arquivos:**
- `crates/theo-domain/src/tool.rs` — `PartialToolResult`, estender `Tool` trait
- `crates/theo-agent-runtime/src/tool_call_manager.rs` — forward
- `crates/theo-tooling/src/bash/` — implementar streaming
- `crates/theo-domain/src/event.rs` — `ToolCallProgress` event

**Ref pi-mono:** `packages/agent/src/types.ts:288-289`, `packages/agent/src/agent-loop.ts:528-548`
**Complexidade:** L | **Deps:** Nenhuma

---

### Task 20: Tool Argument Preparation Hook
**Descrição:** Hook `prepare_arguments` nos tools para normalizar/migrar args antes da validação de schema.

**Critérios de aceite:**
- `Tool` trait ganha `fn prepare_arguments(&self, args: Value) -> Value` (default: identidade)
- Chamado antes da validação no `ToolCallManager::dispatch_and_execute`
- Caso de uso: `edit` tool aceita tanto `filePath` quanto `file_path`
- Testes: preparação roda antes da validação, default é identidade

**DoD:** `cargo test -p theo-tooling` passa.

**Arquivos:**
- `crates/theo-domain/src/tool.rs` — adicionar ao trait
- `crates/theo-agent-runtime/src/tool_call_manager.rs` — chamar prepare antes de validate

**Ref pi-mono:** `packages/agent/src/types.ts:298-299`
**Complexidade:** S | **Deps:** Nenhuma

---

## P5 — TUI Enhancements (3 tasks, respeitando ADR-002)

### Task 21: Input Batching (StdinBuffer)
**Descrição:** Buffer stdin fragmentado em sequências de escape completas.

**Critérios de aceite:**
- `StdinBuffer` acumula bytes stdin e emite sequências completas
- Handles: CSI (`ESC [`), OSC (`ESC ]`), SS3 (`ESC O`), meta keys (`ESC + char`)
- Timeout de 10ms para sequências incompletas (flush as-is)
- Bracketed paste: `ESC[200~`...`ESC[201~` emitido como `Paste(String)` event
- Testes: CSI parcial montado corretamente, paste detection, timeout flush

**DoD:** `cargo test -p theo-cli` passa.

**Arquivos:**
- `apps/theo-cli/src/input/stdin_buffer.rs` — NOVO

**Ref pi-mono:** `packages/tui/src/stdin-buffer.ts` (inteiro)
**Complexidade:** M | **Deps:** Nenhuma

---

### Task 22: Paste Detection & Markers
**Descrição:** Pastes grandes viram markers atômicos.

**Critérios de aceite:**
- Pastes > 100 chars detectadas via bracketed paste ou timing heuristic
- Inserção atômica (não char-by-char)
- Pastes > 5000 chars truncados com warning
- Marker com styling distinto no REPL
- Testes: detection, truncation, atomic insertion

**DoD:** `cargo test -p theo-cli` passa.

**Arquivos:**
- `apps/theo-cli/src/input/stdin_buffer.rs` — paste detection
- `apps/theo-cli/src/repl.rs` — handle paste events

**Ref pi-mono:** `packages/tui/src/components/editor.ts` (paste handling)
**Complexidade:** S | **Deps:** Task 21

---

### Task 23: Enhanced Keyboard Protocol (Kitty + xterm fallback)
**Descrição:** Kitty keyboard protocol para melhor key disambiguation, com fallback xterm.

**Critérios de aceite:**
- Detecção de capability: query Kitty protocol via `CSI ? u`
- Se suportado, enable progressive enhancement
- Fallback para xterm standard
- Key events com modifiers (Ctrl, Alt, Shift) precisos
- Zero dependência ratatui (ADR-002)
- Testes: parsing de Kitty sequences, xterm fallback, modifier detection

**DoD:** `cargo test -p theo-cli` passa.

**Arquivos:**
- `apps/theo-cli/src/input/keyboard.rs` — NOVO: protocol detection + parsing
- `apps/theo-cli/src/tty/caps.rs` — adicionar keyboard protocol detection

**Ref pi-mono:** `packages/tui/src/keys.ts` (inteiro)
**Complexidade:** L | **Deps:** Task 21

---

## Grafo de Dependências

```
P0: [1] [2] [3] [4] [5]           ← todos independentes
P1: [6] [7] [8] [9] [10]          ← todos independentes
P2: [11] → [12] → [13]            ← cadeia linear
P3: [14]  [15→2]  [16→11]  [17→11]
P4: [18] [19] [20]                 ← todos independentes
P5: [21] → [22] → [23]            ← cadeia linear
```

## Ordem de Execução Recomendada

**Sprint 1 (P0):** Tasks 3, 4, 20 (menores, independentes) → depois 5, 1 → depois 2 (XL, fundacional)
**Sprint 2 (P1):** Tasks 7, 10 (S) → 6, 8 (M) → 9 (L)
**Sprint 3 (P2):** Task 11 (XL, blocker) → 12 (M) → 13 (S)
**Sprint 4 (P3+P4):** Tasks 14, 18 (independentes) → 15 (XL, depende de 2) → 16, 17 (dependem de 11) → 19 (L)
**Sprint 5 (P5):** Task 21 → 22 → 23

## Arquivos Críticos (mais tocados)

| Arquivo | Tasks que tocam |
|---------|----------------|
| `crates/theo-agent-runtime/src/run_engine.rs` | 1, 2, 3, 5, 7 |
| `crates/theo-domain/src/agent_message.rs` (NOVO) | 2, 11, 12, 13, 15 |
| `crates/theo-agent-runtime/src/session_manager.rs` (NOVO) | 11, 12, 13, 16, 17 |
| `crates/theo-agent-runtime/src/config.rs` | 1, 5, 7 |
| `crates/theo-infra-llm/src/types.rs` | 4, 6, 8 |
| `crates/theo-domain/src/tool.rs` | 18, 19, 20 |

## Verificação End-to-End

Para cada tier completado:
1. `cargo test` (workspace inteiro)
2. `cargo clippy -- -D warnings`
3. Executar `theo` CLI manualmente com cenário de teste
4. Verificar que features anteriores não regrediram
5. Benchmark de startup (após Task 10) e throughput (após Task 5)
