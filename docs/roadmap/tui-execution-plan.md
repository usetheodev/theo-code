# Plano de Execucao — TUI ratatui (Todas as 7 Fases)

**Referencia:** ADR-003 (`docs/adr/003-tui-ratatui-migration.md`)
**Aprovado em:** Meeting 2026-04-14
**Revisao 1:** 2026-04-14 — correcoes pos-auditoria de 7 findings
**Meta:** Experiencia IGUAL OU SUPERIOR a Claude Code, Gemini CLI, Codex e OpenCode

---

## Errata Rev.1 — Correcoes pos-auditoria

Auditoria identificou 7 findings (3 HIGH, 4 MEDIUM). Correcoes aplicadas:

### Finding 1 (HIGH): Tool trait nao suporta streaming de stdout
**Problema:** `Tool::execute()` retorna `Result<ToolOutput, ToolError>` (`theo-domain/src/tool.rs:239-244`).
O plano original dizia "BashTool retorna `mpsc::Receiver<String>`" — impossivel sem quebrar o trait.
**Correcao:** Canal lateral via `ToolContext`. Adicionar campo `stdout_tx: Option<mpsc::Sender<String>>`
ao struct `ToolContext` (`theo-domain/src/tool.rs:145`). BashTool envia linhas por ele se presente.
O Tool trait NAO muda. O ToolCallManager injeta o tx antes de chamar `execute()`.
**Tasks afetadas:** F1-T03 e F1-T04 reescritas abaixo.

### Finding 2 (HIGH): DecisionControlPlane interativo nao existe
**Problema:** So existe `CapabilityGate` (check binario allow/deny em `capability_gate.rs:30-55`).
Nao ha handshake interativo, oneshot channel, nem "esperar TUI aprovar".
**Correcao:** F4 reclassificada como **"requer nova arquitetura"**. Exige ADR-004 proprio antes
de qualquer codigo. Tasks F4-T01 e F4-T02 marcadas como bloqueadas por ADR-004.
**Estimativa F4 revisada:** de 3 semanas para 4-5 semanas (inclui design + implementacao).

### Finding 3 (HIGH): UndoTool e destrutivo
**Problema:** `git checkout -- <file>` descarta mudancas. Se agente fez 3 edits no mesmo arquivo,
undo do primeiro apaga os outros dois.
**Correcao:** Substituir por **snapshot-based undo**. Antes de cada edit, salvar conteudo original
em `~/.config/theo/undo/{call_id}.bak`. UndoTool restaura do backup, nao do git.
Ainda passa pelo DCP. **Task F4-T05 reescrita abaixo.**

### Finding 4 (MEDIUM): Eventos inexistentes no runtime
**Problema:** `LlmCallEnd` nunca e publicado pelo run_engine.rs (grep retorna 0 matches).
`RunStateChanged` tem from/to como strings genéricas, sem fase/iteracao explicita.
**Correcao:** Nova task **F1-T00: Preencher lacunas de eventos no runtime** adicionada como
primeira task da Fase 1, bloqueando F1-T08 (que depende desses eventos para StatusLine).

### Finding 5 (MEDIUM): TodoUpdated/GRAPHCTX/subagents nao publicados
**Problema:** `TodoUpdated` existe no enum mas nenhum crate publica. `GraphContextService` nao
integra com EventBus. Subagentes sao strings em RunStateChanged.
**Correcao:** Cada fase que depende desses dados recebe task previa de "backend: emissao de eventos".
F5 inteira recebe bloco de prerequisitos. F2 sidebar removida (era F5, nao F2).

### Finding 6 (MEDIUM): Session picker assume formato inexistente
**Problema:** Sessoes sao `Vec<Message>` em JSON sem metadados. Data, preview e hash nao estao no JSON.
**Correcao:** F2-T03 reescrita para trabalhar com formato real: data = mtime do arquivo,
hash = filename, preview = primeiro Message.content truncado.

### Finding 7 (MEDIUM): Inconsistencias de API/nomes
**Problema:** `tui::run(agent_config)` usa tipo Config inexistente (CLI usa AgentConfig).
opaline nao esta no workspace. Deps sem auditoria de licenca.
**Correcao:** Assinaturas corrigidas para usar tipos reais. Cada fase que adiciona dep
recebe sub-task de auditoria (`cargo-deny` / `cargo-license`).

### Classificacao de trabalho

O plano agora separa explicitamente 3 classes:

| Classe | Descricao | Exemplo |
|--------|-----------|---------|
| **A: Implementavel agora** | Consome APIs e eventos que ja existem | TUI view, broadcast bridge, fix renderer |
| **B: Extensao moderada** | Adiciona campos/eventos a structs existentes sem mudar contratos | stdout_tx em ToolContext, LlmCallEnd no run_engine |
| **C: Arquitetura nova** | Requer ADR novo, muda modelo de execucao | DCP interativo, approval handshake |

Cada task agora tem tag [A], [B] ou [C].

---

## Convencoes

- **Tamanho:** S = <2h, M = 2-4h, L = 4-8h, XL = 8-16h
- **Criterio de aceite:** Given/When/Then
- **DoD por task:** lista de checklist binaria (passa/nao passa)
- **Bloqueio:** task X → task Y significa X deve estar done antes de Y comecar
- **Crate afetado:** entre parenteses
- **Classe:** [A] implementavel agora, [B] extensao moderada, [C] arquitetura nova

---

# FASE 1 — Shell Vivo

**Timebox:** 2 semanas
**Meta:** abrir `theo --tui`, ver tool cards ao vivo com timer, texto streaming com cursor, StatusLine atualizada.
**Corte se atrasar:** StatusLine → stdout streaming → placeholder (nessa ordem)

---

## F1-T00: Preencher lacunas de eventos no runtime [B]

**Tamanho:** M
**Arquivos:** `crates/theo-agent-runtime/src/run_engine.rs`, `crates/theo-domain/src/event.rs`
**Depende de:** nenhum
**Bloqueia:** F1-T08 (StatusLine depende desses eventos)

### Contexto (Finding 4)

O runtime NAO publica `LlmCallEnd` (grep no run_engine.rs retorna 0 matches). A TUI precisa
de LlmCallEnd para saber quando a resposta do LLM terminou e atualizar tokens na StatusLine.
Tambem: `RunStateChanged` nao inclui iteration count no payload.

### Microtasks

1. No `run_engine.rs`, apos receber resposta completa do LLM (apos o loop de streaming de ContentDelta),
   publicar `EventType::LlmCallEnd` com payload `{"iteration": N, "tokens_in": X, "tokens_out": Y}`
   (localizar: provavelmente apos a chamada `llm_client.chat()` que retorna a response, antes de processar tool calls)
2. Enriquecer payload de `RunStateChanged` em `transition_run()` (run_engine.rs:1381):
   adicionar `"iteration"` e `"max_iterations"` ao JSON
3. Testes: verificar que LlmCallEnd e publicado exatamente 1 vez por chamada LLM

### Criterio de aceite

```
Given: AgentRunEngine executa uma iteracao com chamada LLM
When:  LLM retorna resposta
Then:  EventBus contem LlmCallStart seguido de LlmCallEnd
  AND  LlmCallEnd.payload contem "iteration" e "tokens_out"

Given: AgentRunEngine transiciona de Planning para Executing
When:  transition_run() e chamado
Then:  RunStateChanged.payload contem "iteration" e "max_iterations"
```

### DoD

- [ ] `cargo test -p theo-agent-runtime` passa
- [ ] LlmCallEnd publicado apos cada resposta LLM
- [ ] RunStateChanged enriquecido com iteration/max_iterations
- [ ] 2 testes novos: `llm_call_end_published`, `run_state_changed_has_iteration`

---

## F1-T01: Adicionar ToolCallStdoutDelta ao EventType (theo-domain) [B]

**Tamanho:** S
**Arquivo:** `crates/theo-domain/src/event.rs`
**Bloqueia:** F1-T02, F1-T03, F1-T06, F1-T07, F1-T08

### Microtasks

1. Adicionar variante `ToolCallStdoutDelta` ao enum `EventType` (entre `ContentDelta` e `TodoUpdated`)
2. Adicionar match arm em `Display::fmt` retornando `"ToolCallStdoutDelta"`
3. Atualizar `ALL_EVENT_TYPES` de `[EventType; 14]` para `[EventType; 15]`
4. Atualizar teste `display_all_event_types` (array `expected` na linha 126)

### Criterio de aceite

```
Given: o enum EventType existe com 14 variantes
When:  adiciono ToolCallStdoutDelta e atualizo ALL_EVENT_TYPES
Then:  ALL_EVENT_TYPES.len() == 15
  AND  EventType::ToolCallStdoutDelta.to_string() == "ToolCallStdoutDelta"
  AND  serde_json roundtrip de ToolCallStdoutDelta funciona
  AND  todos os testes existentes continuam passando
```

### DoD

- [ ] `cargo test -p theo-domain` passa (0 falhas)
- [ ] Zero warnings novos
- [ ] `serde_roundtrip_all_event_types` cobre 15 variantes
- [ ] `display_all_event_types` atualizado e verde
- [ ] Nenhum outro crate quebra compilacao (`cargo check --workspace`)

---

## F1-T02: Broadcast bridge no EventBus (theo-agent-runtime) [B]

**Tamanho:** M
**Arquivo:** `crates/theo-agent-runtime/src/event_bus.rs`
**Depende de:** F1-T01
**Bloqueia:** F1-T08

### Microtasks

1. Adicionar `tokio` como dep em `theo-agent-runtime/Cargo.toml` com feature `sync` (para `broadcast`)
2. Criar struct `BroadcastListener { tx: tokio::sync::broadcast::Sender<DomainEvent> }`
3. Implementar `EventListener for BroadcastListener` — `on_event` faz `let _ = self.tx.send(event.clone())`
4. Adicionar metodo `pub fn subscribe_broadcast(&self, capacity: usize) -> tokio::sync::broadcast::Receiver<DomainEvent>`
5. Escrever 4 testes

### Criterio de aceite

```
Given: um EventBus com subscribe_broadcast(1024)
When:  publico 3 eventos via bus.publish()
Then:  receiver.try_recv() retorna os 3 eventos na ordem

Given: um EventBus com subscribe_broadcast(2)
When:  publico 5 eventos sem consumir o receiver
Then:  receiver.recv() retorna Err(Lagged(n)) onde n >= 1

Given: um broadcast receiver que foi dropado
When:  publico mais eventos no bus
Then:  bus.publish() nao panica E bus.len() continua incrementando

Given: um CapturingListener registrado ANTES do subscribe_broadcast
When:  publico 1 evento
Then:  CapturingListener recebe o evento E broadcast receiver tambem recebe
```

### DoD

- [ ] `cargo test -p theo-agent-runtime` passa (0 falhas)
- [ ] 4 testes novos: `broadcast_receives_events`, `broadcast_lagged`, `broadcast_drop_no_crash`, `broadcast_coexists_with_sync_listeners`
- [ ] Zero `unwrap()` em codigo de producao (testes podem usar)
- [ ] Capacity e parametro exposto (nao hardcoded)
- [ ] `cargo check --workspace` limpo

---

## F1-T03: Stdout streaming via ToolContext (theo-domain + theo-tooling) [B→borderline C]

**Tamanho:** XL (revisado — Finding 2 Rev.2: path sandbox invade abstracoes)
**Arquivos:** `crates/theo-domain/src/tool.rs`, `crates/theo-tooling/src/bash/mod.rs`, `crates/theo-tooling/src/sandbox/executor.rs`
**Depende de:** F1-T01
**Bloqueia:** F1-T04

### Contexto (Finding 1)

O plano original dizia "BashTool retorna mpsc::Receiver<String>". Isso e IMPOSSIVEL
porque `Tool::execute()` retorna `Result<ToolOutput, ToolError>` (`tool.rs:239-244`)
e o trait e o contrato central de TODAS as tools.

**Solucao real:** canal lateral via `ToolContext`. O `ToolContext` (`tool.rs:145-154`) ja
e o ponto de injecao de dependencias (tem `abort`, `graph_context`). Adicionamos
`stdout_tx: Option<mpsc::Sender<String>>`. BashTool envia linhas por ele se presente.
O Tool trait NAO muda. Nenhum outro tool e afetado.

### Microtasks

1. Adicionar campo ao `ToolContext` em `theo-domain/src/tool.rs:145`:
   ```rust
   pub struct ToolContext {
       // ... campos existentes ...
       /// Optional channel for streaming stdout lines during execution.
       /// If Some, tools that support streaming send lines here.
       /// If None, tools execute normalmente (backward compatible).
       pub stdout_tx: Option<tokio::sync::mpsc::Sender<String>>,
   }
   ```
2. Atualizar `ToolContext::test_context()` (tool.rs:168) para incluir `stdout_tx: None`
3. Atualizar todos os call sites que constroem ToolContext (grep `ToolContext {` no workspace)
   para adicionar `stdout_tx: None` (backward compatible — ninguem usa streaming ainda)
4. **Path sem sandbox** (bash/mod.rs linhas 228-236): substituir `Command::output()` por
   `Command::spawn()` + `BufReader::lines()` em loop async. Se `ctx.stdout_tx` e `Some(tx)`,
   enviar cada linha por `tx`. Se `None`, acumular em String e retornar como antes.
   Este path e direto — tokio::process nativo, sem mudar nenhum trait.

5. **Path com sandbox** (bash/mod.rs linhas 214-219 + sandbox/executor.rs):
   ATENCAO: `SandboxExecutor::execute_sandboxed()` retorna `Result<SandboxResult, SandboxError>`
   com stdout completo no final (executor.rs:22-26). O trait NAO suporta streaming nativo.

   **Duas opcoes, em ordem de preferencia:**

   **Opcao A (menos invasiva, recomendada para F1):** NAO mudar o trait SandboxExecutor.
   Em vez disso, no BashTool path sandbox, usar `std::process::Command::spawn()` diretamente
   (em vez de delegar para executor) com `Stdio::piped()` + thread dedicada drenando stdout.
   Aplicar as mesmas restricoes de sandbox (rlimits, env sanitizer, command validator) manualmente
   antes do spawn. O SandboxExecutor continua existindo para o path "sem streaming".
   Desvantagem: duplica parte da logica de isolamento.

   **Opcao B (mais correta, mais invasiva):** Adicionar metodo opt-in ao trait:
   ```rust
   fn execute_sandboxed_streaming(
       &self, command: &str, working_dir: &Path, config: &SandboxConfig,
       line_tx: std::sync::mpsc::Sender<String>,
   ) -> Result<SandboxResult, SandboxError> {
       // default: ignora line_tx, delega para execute_sandboxed
       self.execute_sandboxed(command, working_dir, config)
   }
   ```
   Backward compatible via default impl. NoopExecutor e LandlockExecutor sobrescrevem
   com implementacao que drena pipe linha a linha.
   Desvantagem: muda trait publico (precisa /meeting).

   **Decisao para F1:** usar Opcao A. Registrar Opcao B como tech debt para F2/F3.
   Se F1 atrasar no sandbox streaming, CORTAR este item (manter stdout streaming apenas
   no path sem sandbox, que ja cobre o caso mais comum: dev local sem bwrap).

6. Ambos os paths: manter o retorno `Result<ToolOutput, ToolError>` IDENTICO ao atual.
   O ToolOutput.output continua contendo stdout completo (para o LLM consumir).
   O stdout_tx e ADICIONAL (para TUI exibir ao vivo).
7. Guards de seguranca em ambos os paths:
   - Truncar linhas > 10KB (`[truncated]`)
   - Rate limit: max 100 linhas/s (tokio interval 10ms entre sends em burst)
   - Limite total: 1MB por tool call (depois para de enviar, mas continua executando)
   - Sanitizar: aplicar `sanitize_stdout_line()` em cada linha antes de send
8. **Criar funcao `sanitize_stdout_line()`** em `crates/theo-tooling/src/sandbox/env_sanitizer.rs`:
   ```rust
   /// Redact known secret patterns from a stdout line before streaming to TUI.
   /// Uses ALWAYS_STRIPPED_ENV_PREFIXES to identify variable names, then checks
   /// if any current env var value appears in the line and replaces with [REDACTED].
   pub fn sanitize_stdout_line(line: &str) -> String {
       let mut result = line.to_string();
       for (key, value) in std::env::vars() {
           if is_always_stripped(&key) && !value.is_empty() && value.len() >= 8 {
               result = result.replace(&value, "[REDACTED]");
           }
       }
       result
   }
   ```
   NOTA: `sanitize_value()` referenciada em versoes anteriores do plano NAO EXISTE no codigo.
   A funcao existente `sanitized_env()` (env_sanitizer.rs:15) opera sobre env vars pre-execucao,
   nao sobre stdout. `sanitize_stdout_line()` e uma funcao NOVA que precisa ser criada.
   Testes: linha com valor de AWS_SECRET_ACCESS_KEY -> substituido por [REDACTED];
   linha sem segredos -> inalterada; valor curto (<8 chars) -> nao redactado (evita false positives).
9. Testes: BashTool com `stdout_tx: Some(tx)` e com `stdout_tx: None`

### Criterio de aceite

```
Given: BashTool com ctx.stdout_tx = Some(tx)
When:  executo "seq 1 10" (10 linhas)
Then:  tx recebe 10 linhas na ordem
  AND  ToolOutput.output contem stdout completo (como antes)
  AND  retorno e Ok(ToolOutput) — contrato do Tool trait intacto

Given: BashTool com ctx.stdout_tx = None
When:  executo "echo hello"
Then:  funciona IDENTICO ao antes (ToolOutput.output contem "hello\n")
  AND  nenhum panic, nenhum channel error

Given: BashTool com ctx.stdout_tx = Some(tx)
When:  executo comando que gera linha de 15KB
Then:  tx recebe linha truncada para 10KB com "[truncated]"

Given: BashTool com ctx.stdout_tx = Some(tx)
When:  executo comando que gera 2MB de output
Then:  tx para de enviar apos 1MB
  AND  ToolOutput.output contem output completo (ate limite de truncate_output)

Given: ToolContext::test_context(dir)
When:  construido
Then:  stdout_tx == None (backward compatible)
```

### DoD

- [ ] `Tool::execute()` trait NAO mudou (mesmo signature)
- [ ] `ToolContext` tem campo `stdout_tx: Option<mpsc::Sender<String>>`
- [ ] Todos os call sites de ToolContext atualizados com `stdout_tx: None`
- [ ] `cargo test -p theo-tooling` passa (0 falhas, 13 testes existentes intactos)
- [ ] `cargo check --workspace` limpo (nenhum crate quebrado pelo campo novo)
- [ ] 6 testes novos: `streaming_via_ctx`, `no_streaming_backward_compat`, `truncate_long_line`, `total_bytes_limit`, `sanitize_stdout_line_redacts_secret`, `sanitize_stdout_line_preserves_normal`
- [ ] `sanitize_stdout_line()` criada em `env_sanitizer.rs` com testes proprios
- [ ] Zero `unwrap()` em codigo de producao
- [ ] Guards implementados: 10KB/linha, 100 linhas/s, 1MB total, sanitizacao via `sanitize_stdout_line()`

---

## F1-T04: ToolCallManager injeta stdout_tx e converte em DomainEvents [B]

**Tamanho:** M
**Arquivo:** `crates/theo-agent-runtime/src/tool_call_manager.rs`, `crates/theo-agent-runtime/src/tool_bridge.rs`
**Depende de:** F1-T03
**Bloqueia:** F1-T08

### Contexto (Finding 1 continuacao)

Com F1-T03, o BashTool envia linhas por `ctx.stdout_tx` se presente. Agora o ToolCallManager
precisa: (1) criar o canal mpsc, (2) injetar o tx no ToolContext, (3) spawnar task que drena
o rx e converte cada linha em `ToolCallStdoutDelta` via EventBus.

O tool_bridge.rs NAO muda de assinatura — continua retornando `(Message, bool)`.
A magia acontece DENTRO do ToolCallManager antes de chamar tool_bridge.

### Microtasks

1. Em `dispatch_and_execute()` (tool_call_manager.rs:93), antes da etapa 3 (execute tool):
   - Criar canal: `let (stdout_tx, mut stdout_rx) = tokio::sync::mpsc::channel::<String>(256);`
   - Clonar ToolContext e setar `ctx.stdout_tx = Some(stdout_tx)`
2. Spawnar task tokio que drena stdout_rx e publica eventos:
   ```rust
   let bus = self.event_bus.clone();
   let cid = call_id.clone();
   let tname = tool_name.clone();
   tokio::spawn(async move {
       while let Some(line) = stdout_rx.recv().await {
           bus.publish(DomainEvent::new(
               EventType::ToolCallStdoutDelta,
               cid.as_str(),
               serde_json::json!({"line": line, "stream": "stdout", "tool_name": tname}),
           ));
       }
   });
   ```
3. Sequencing: ToolCallQueued ja e publicado em `enqueue()` (linha 76), que e chamado ANTES
   de `dispatch_and_execute()`. Portanto Queued < StdoutDelta* e garantido pela ordem de chamada.
   ToolCallCompleted e publicado APOS `execute_tool_call()` retornar (linha 205), que so retorna
   quando BashTool termina (dropando o tx, o que encerra o while let Some acima).
4. Escrever teste com CapturingListener verificando ordem dos eventos

### Criterio de aceite

```
Given: ToolCallManager com EventBus e CapturingListener
When:  enqueue() + dispatch_and_execute() com BashTool que emite 3 linhas via stdout_tx
Then:  CapturingListener recebe na ordem:
       1. ToolCallQueued (de enqueue)
       2. ToolCallDispatched (de dispatch_and_execute)
       3. ToolCallStdoutDelta (linha 1)
       4. ToolCallStdoutDelta (linha 2)
       5. ToolCallStdoutDelta (linha 3)
       6. ToolCallCompleted (apos BashTool retornar)

Given: ToolCallManager com ReadTool (que ignora stdout_tx porque nao e bash)
When:  dispatch_and_execute()
Then:  nenhum ToolCallStdoutDelta publicado (tx dropado sem uso)
  AND  ToolCallCompleted publicado normalmente

Given: tool_bridge::execute_tool_call()
When:  chamado
Then:  assinatura e IDENTICA a antes: retorna (Message, bool)
```

### DoD

- [ ] `tool_bridge::execute_tool_call()` NAO mudou de assinatura
- [ ] `cargo test -p theo-agent-runtime` passa (0 falhas)
- [ ] 2 testes novos: `stdout_delta_events_in_order`, `non_streaming_tool_no_delta`
- [ ] Sequencing Queued < Dispatched < StdoutDelta* < Completed verificado
- [ ] 14 testes existentes de tool_call_manager intactos
- [ ] `cargo check --workspace` limpo

---

## F1-T05: Fix ToolCallQueued no CliRenderer (theo-cli) [A]

**Tamanho:** S
**Arquivo:** `apps/theo-cli/src/renderer.rs`
**Depende de:** nenhum
**Bloqueia:** F1-T08

### Microtasks

1. Remover supressao no match arm `ToolCallQueued` (linhas 38-40)
2. Implementar renderizacao: `eprint!` com formato `"  ┌─ {tool_name} ─ running...\n"`
3. Adicionar `ToolCallStdoutDelta` ao match: eprint da linha com indentacao `"  │ {line}\n"`
4. Criar struct `CapturingRenderer` em `#[cfg(test)]` (ou mover do event_bus.rs para pub teste utils)
5. Escrever 4 testes

### Criterio de aceite

```
Given: CliRenderer recebe DomainEvent de ToolCallQueued para "bash" com tool_name "bash"
When:  on_event() e chamado
Then:  output contem "bash" E indicador de running (nao esta vazio)

Given: CliRenderer recebe ToolCallStdoutDelta com line "Compiling theo v0.1.0"
When:  on_event() e chamado
Then:  output contem "Compiling theo v0.1.0"

Given: CliRenderer recebe ToolCallCompleted para "bash" com success=true
When:  on_event() e chamado
Then:  output contem indicador de sucesso (checkmark ou "ok")

Given: CliRenderer recebe ToolCallQueued seguido de ToolCallCompleted
When:  ambos on_event() sao chamados
Then:  output mostra PRIMEIRO o running, DEPOIS o completed (nao duplica info)
```

### DoD

- [ ] `cargo test -p theo` passa (0 falhas)
- [ ] 4 testes novos: `queued_emits_output`, `stdout_delta_emits_line`, `completed_still_works`, `queued_then_completed_no_duplicate`
- [ ] CliRenderer tem `#[cfg(test)] mod tests` (nao tinha antes)
- [ ] Zero warnings novos
- [ ] REPL legado continua funcionando (`theo "echo hello"` sem --tui)

---

## F1-T06: Workspace deps + Cargo.toml theo-cli (theo-cli) [A]

**Tamanho:** S
**Arquivo:** `Cargo.toml` (root), `apps/theo-cli/Cargo.toml`
**Depende de:** F1-T01 (ToolCallStdoutDelta compilar)
**Bloqueia:** F1-T08

### Microtasks

1. **Auditoria de licencas e seguranca** (Finding 5 Rev.2):
   - Rodar `cargo license` (ou `cargo-deny`) para ratatui, crossterm, tui-textarea, throbber-widgets-tui
   - Verificar: todas MIT ou Apache-2.0 (compativel com projeto)
   - Verificar: nenhuma advisory ativa em `cargo audit`
   - Documentar resultado no PR
2. Adicionar ao `[workspace.dependencies]` do root `Cargo.toml`:
   ```toml
   ratatui = "0.29"
   crossterm = "0.28"
   tui-textarea = "0.7"
   throbber-widgets-tui = "0.8"
   ```
3. Adicionar ao `[dependencies]` de `apps/theo-cli/Cargo.toml`:
   ```toml
   ratatui.workspace = true
   crossterm.workspace = true
   tui-textarea.workspace = true
   throbber-widgets-tui.workspace = true
   ```
4. Verificar que `cargo check -p theo` compila
5. Verificar que `cargo build -p theo` gera binario funcional
6. Confirmar que `referencias/` NAO esta incluida no workspace members

**NOTA:** Toda fase que adiciona deps novas (F2: tui-syntax-highlight, ansi-to-tui, ratatui-toaster;
F4: tachyonfx, similar, tui-overlay; F5: tui-tree-widget, ratatui-image; F7: opaline) DEVE incluir
a mesma sub-task de auditoria (licenca + cargo audit) antes de adicionar a dep.

### Criterio de aceite

```
Given: workspace Cargo.toml com deps ratatui, crossterm, tui-textarea, throbber-widgets-tui
When:  cargo check --workspace
Then:  compila sem erros E sem warnings sobre deps nao usadas

Given: binario theo compilado com deps novas
When:  executo "theo --help"
Then:  saida e identica a antes (deps novas nao afetam CLI existente)

Given: cargo audit executado
When:  verifica deps novas
Then:  zero advisories ativas
```

### DoD

- [ ] Auditoria de licencas documentada no PR (todas MIT/Apache-2.0)
- [ ] `cargo audit` sem advisories para deps novas
- [ ] `cargo check --workspace` limpo
- [ ] `cargo build -p theo` gera binario
- [ ] `theo --help` funciona identico
- [ ] `cargo test --workspace` passa
- [ ] Deps declaradas em workspace.dependencies (nao inline)
- [ ] `referencias/` nao aparece em `[workspace] members`

---

## F1-T07: Flag --tui e dispatch no main.rs (theo-cli) [A]

**Tamanho:** S
**Arquivo:** `apps/theo-cli/src/main.rs`
**Depende de:** F1-T06
**Bloqueia:** F1-T08

### Microtasks

1. Adicionar campo `#[arg(long, help = "Launch experimental TUI mode")]` `tui: bool` ao struct Cli (antes de `prompt`)
2. No match de `cmd_agent()`, checar `cli.tui`:
   - Se true: `eprintln!("[experimental] TUI mode — report bugs"); tui::run(agent_config).await`
   - Se false: caminho existente (Repl)
3. Adicionar `mod tui;` em main.rs (modulo ainda vazio, so `pub async fn run(_config: AgentConfig) -> anyhow::Result<()> { Ok(()) }`)
4. Escrever 3 testes de parsing clap

### Criterio de aceite

```
Given: binario theo compilado
When:  executo "theo --tui"
Then:  output contem "[experimental]" E nao crasha

Given: binario theo compilado
When:  executo "theo agent 'hello'" (sem --tui)
Then:  comportamento identico ao antes (REPL legado)

Given: struct Cli
When:  Cli::try_parse_from(["theo", "--tui"])
Then:  cli.tui == true E cli.prompt.is_empty()

Given: struct Cli
When:  Cli::try_parse_from(["theo", "fix the bug"])
Then:  cli.tui == false E cli.prompt == ["fix", "the", "bug"]
```

### DoD

- [ ] `cargo test -p theo` passa (0 falhas)
- [ ] 3 testes novos: `tui_flag_parsed`, `default_no_tui`, `tui_flag_with_prompt`
- [ ] `theo --help` mostra a flag --tui
- [ ] `theo --tui` imprime warning experimental e sai limpo
- [ ] Caminho sem --tui funciona identico (6 testes de session passando)

---

## F1-T08: Modulo TUI — app.rs, state, msg, update (theo-cli) [A]

**Tamanho:** L
**Arquivo:** `apps/theo-cli/src/tui/mod.rs`, `apps/theo-cli/src/tui/app.rs`
**Depende de:** F1-T00, F1-T02, F1-T04, F1-T05, F1-T06, F1-T07

### Microtasks

1. Criar `apps/theo-cli/src/tui/mod.rs` com `pub mod app; pub mod view; pub mod input; pub mod events; pub mod widgets;`
2. Criar `tui/app.rs` com:
   ```rust
   pub struct TuiState {
       pub should_quit: bool,
       pub messages: Vec<TranscriptEntry>,
       pub tool_cards: HashMap<String, ToolCardState>,
       pub input: tui_textarea::TextArea<'static>,
       pub status: StatusLineState,
       pub cursor_visible: bool,
       pub size: (u16, u16),
       pub agent_running: bool,
       pub events_lost: u64,
   }

   pub enum Msg {
       Quit,
       Resize(u16, u16),
       DomainEvent(DomainEvent),
       DomainEventBatch(Vec<DomainEvent>),
       EventsLost(u64),
       InputKey(crossterm::event::KeyEvent),
       Submit(String),
       Tick,
   }

   pub fn update(state: &mut TuiState, msg: Msg) { ... }
   ```
3. Implementar `update()` para cada variante de Msg:
   - `Quit` → `should_quit = true`
   - `Resize(w,h)` → atualiza `size`
   - `DomainEvent` → dispatch para handler interno por EventType
   - `DomainEventBatch` → iterar e aplicar cada evento
   - `EventsLost(n)` → incrementar `events_lost`, adicionar marcador visual no transcript
   - `InputKey` → delegar para `tui_textarea`
   - `Submit` → extrair texto, limpar input, adicionar ao transcript como UserMessage
   - `Tick` → toggle cursor_visible a cada 500ms, incrementar timers de tool cards
4. Implementar handlers de DomainEvent:
   - `ContentDelta` → append texto ao ultimo assistant message + set cursor_visible=true
   - `ReasoningDelta` → append ao bloco de reasoning (collapsed)
   - `ToolCallQueued` → criar ToolCardState { tool_name, status: Running, started_at: now, lines: vec![] }
   - `ToolCallStdoutDelta` → append linha ao ToolCardState.lines (max ultimas 5 visiveis)
   - `ToolCallCompleted` → atualizar ToolCardState { status: Done/Failed, duration_ms }
   - `RunStateChanged` → atualizar StatusLineState (fase, iteracao)
   - `LlmCallStart/End` → atualizar StatusLineState (tokens)
   - `BudgetExceeded` → adicionar warning ao transcript
   - `Error` → adicionar mensagem de erro ao transcript
5. Escrever 10 testes unitarios para `update()`

### Criterio de aceite

```
Given: TuiState inicial vazio
When:  update(Msg::Quit)
Then:  state.should_quit == true

Given: TuiState com messages vazio
When:  update(Msg::DomainEvent(ContentDelta { text: "hello" }))
Then:  state.messages.last() contem AssistantMessage com text "hello"

Given: TuiState com messages vazio
When:  update(Msg::DomainEvent(ToolCallQueued { tool_name: "bash" }))
Then:  state.tool_cards contem entrada para o call_id com status Running

Given: TuiState com tool_card "c-1" em Running
When:  update(Msg::DomainEvent(ToolCallStdoutDelta { call_id: "c-1", line: "Compiling..." }))
Then:  state.tool_cards["c-1"].lines contem "Compiling..."

Given: TuiState com tool_card "c-1" em Running
When:  update(Msg::DomainEvent(ToolCallCompleted { call_id: "c-1", success: true, duration: 3200 }))
Then:  state.tool_cards["c-1"].status == Done AND duration_ms == 3200

Given: TuiState com cursor_visible=true, last_cursor_toggle=0
When:  update(Msg::Tick) apos 500ms
Then:  state.cursor_visible == false

Given: TuiState com events_lost=0
When:  update(Msg::EventsLost(5))
Then:  state.events_lost == 5 AND transcript contem marcador "[5 eventos perdidos]"

Given: TuiState com input contendo "fix the bug"
When:  update(Msg::Submit("fix the bug"))
Then:  state.input esta vazio AND state.messages.last() == UserMessage("fix the bug")
```

### DoD

- [ ] `cargo test -p theo` passa (0 falhas)
- [ ] 10 testes unitarios para update(): quit, resize, content_delta, reasoning_delta, tool_queued, tool_stdout_delta, tool_completed, cursor_blink, events_lost, submit
- [ ] TuiState e Msg sao tipos proprios (nao closures)
- [ ] update() e funcao pura (sem IO, sem async, sem eprint)
- [ ] Zero `unwrap()` em update()

---

## F1-T09: Modulo TUI — view.rs com layout (theo-cli) [A]

**Tamanho:** L
**Arquivo:** `apps/theo-cli/src/tui/view.rs`
**Depende de:** F1-T08

### Microtasks

1. Criar funcao `pub fn draw(frame: &mut Frame, state: &TuiState)`
2. Layout com `Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(3), Constraint::Length(1)])`:
   - Chunk 0: Header
   - Chunk 1: Transcript (scrollavel)
   - Chunk 2: Input area
   - Chunk 3: StatusLine
3. Implementar `render_header()`:
   - Spans: `"theo"` bold, separador `" · "`, modo (Agent/Plan/Ask), modelo, branch, cwd, tokens, custo
   - Alinhado: esquerda (app/modo/modelo) + direita (tokens/custo)
4. Implementar `render_transcript()`:
   - Iterar `state.messages` e `state.tool_cards`
   - UserMessage: `"> "` + texto cyan
   - AssistantMessage: texto branco + cursor piscante se streaming
   - ToolCard running: `"┌─ {tool_name} ─ {elapsed}s... ──┐"` + throbber + ultimas linhas de stdout
   - ToolCard done: `"┌─ {tool_name} ─ {duration}s ✓ ──┐"` verde ou `"✗"` vermelho
   - Auto-scroll: se usuario esta no fundo, seguir; se scrollou pra cima, manter posicao
5. Implementar `render_input()`:
   - tui-textarea widget com placeholder contextual:
     - agent_running=true → "aguardando agente..." (dimmed, nao editavel)
     - agent_running=false → "Digite uma tarefa ou /help" (dimmed)
6. Implementar `render_status_line()`:
   - Spans: modo, fase (LOCATE/EDIT/VERIFY/DONE), iteracao/max, N tools rodando, keybind hints
   - Formato: `"AGENT │ LOCATE→EDIT │ 3/40 iter │ 2 tools │ ? ajuda  Ctrl+C para"`
7. Escrever 2 testes de snapshot com TestBackend

### Criterio de aceite

```
Given: TuiState com 1 user message "hello" e 1 assistant message "world"
When:  draw() em TestBackend 80x24
Then:  buffer contem "hello" na area de transcript
  AND  buffer contem "world" na area de transcript
  AND  primeira linha contem "theo"
  AND  ultima linha contem "AGENT"

Given: TuiState com 1 tool_card em Running com tool_name="bash"
When:  draw() em TestBackend 80x24
Then:  buffer contem "bash" E "running" ou throbber char

Given: TuiState com agent_running=false
When:  draw() em TestBackend 80x24
Then:  area de input contem placeholder "/help"

Given: TuiState vazio
When:  draw() em TestBackend 200x50
Then:  layout se adapta sem crash E header usa toda a largura
```

### DoD

- [ ] `cargo test -p theo` passa (0 falhas)
- [ ] 2 snapshot tests: `snapshot_80x24` e `snapshot_200x50`
- [ ] draw() e funcao pura (recebe Frame + TuiState, sem side effects)
- [ ] Header, transcript, input e status line renderizam corretamente
- [ ] Auto-scroll funciona (quando no fundo, segue)
- [ ] Zero `unwrap()` em draw()

---

## F1-T10: Modulo TUI — events.rs e input.rs (theo-cli) [A]

**Tamanho:** M
**Arquivo:** `apps/theo-cli/src/tui/events.rs`, `apps/theo-cli/src/tui/input.rs`
**Depende de:** F1-T02, F1-T08

### Microtasks

1. Criar `events.rs` — Event Task:
   ```rust
   pub async fn event_loop(
       mut rx: broadcast::Receiver<DomainEvent>,
       tx: mpsc::Sender<Msg>,
   ) {
       let mut batch = Vec::new();
       let mut interval = tokio::time::interval(Duration::from_millis(16));
       loop {
           tokio::select! {
               event = rx.recv() => match event {
                   Ok(e) => batch.push(e),
                   Err(RecvError::Lagged(n)) => { tx.send(Msg::EventsLost(n)).await.ok(); },
                   Err(RecvError::Closed) => break,
               },
               _ = interval.tick() => {
                   if !batch.is_empty() {
                       let b = std::mem::take(&mut batch);
                       tx.send(Msg::DomainEventBatch(b)).await.ok();
                   }
               }
           }
       }
   }
   ```
2. Criar `input.rs` — Input Task:
   ```rust
   pub async fn input_loop(tx: mpsc::Sender<Msg>) {
       let mut reader = crossterm::event::EventStream::new();
       while let Some(Ok(event)) = reader.next().await {
           match event {
               Event::Key(key) => {
                   if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                       tx.send(Msg::Quit).await.ok();
                   } else {
                       tx.send(Msg::InputKey(key)).await.ok();
                   }
               }
               Event::Resize(w, h) => { tx.send(Msg::Resize(w, h)).await.ok(); }
               _ => {}
           }
       }
   }
   ```
3. Parametrizar input_loop para aceitar trait/channel em testes (DIP — nao depender de stdin real)
4. Escrever 3 testes

### Criterio de aceite

```
Given: event_loop com broadcast receiver recebendo 5 eventos em <16ms
When:  16ms passam (via tokio::time::pause + advance)
Then:  tx envia um unico Msg::DomainEventBatch com 5 eventos

Given: event_loop com broadcast receiver que retorna Lagged(3)
When:  evento e recebido
Then:  tx envia Msg::EventsLost(3)

Given: input_loop com KeyEvent(Ctrl+C)
When:  evento chega
Then:  tx envia Msg::Quit
```

### DoD

- [ ] `cargo test -p theo` passa (0 falhas)
- [ ] 3 testes: `batching_16ms`, `events_lost_on_lagged`, `ctrl_c_sends_quit`
- [ ] Todos os timers usam `tokio::time::sleep/interval` (nao std::thread::sleep)
- [ ] input_loop parametrizado para nao depender de stdin em testes
- [ ] Zero `unwrap()` em producao

---

## F1-T11: Modulo TUI — run() entrypoint (theo-cli) [A]

**Tamanho:** M
**Arquivo:** `apps/theo-cli/src/tui/mod.rs`
**Depende de:** F1-T08, F1-T09, F1-T10

### Microtasks

1. Implementar `pub async fn run(config: AgentConfig) -> anyhow::Result<()>`:
   - Setup terminal: `enable_raw_mode()`, `EnterAlternateScreen`, `Terminal::new(CrosstermBackend::new(stdout()))`
   - Criar EventBus, AgentRunEngine (reusar logica de cmd_agent em main.rs)
   - `let rx = event_bus.subscribe_broadcast(1024);`
   - Criar canais mpsc para Msg
   - Spawn 3 tasks: input_loop, event_loop, render tick
   - Render loop:
     ```rust
     let mut interval = tokio::time::interval(Duration::from_millis(33)); // 30fps
     loop {
         // Drain all pending messages
         while let Ok(msg) = msg_rx.try_recv() {
             app::update(&mut state, msg);
         }
         terminal.draw(|f| view::draw(f, &state))?;
         if state.should_quit { break; }
         interval.tick().await;
     }
     ```
   - Cleanup: `disable_raw_mode()`, `LeaveAlternateScreen`
   - Cleanup em panic tambem (via `std::panic::set_hook`)
2. Implementar submit handler: quando Msg::Submit, chamar `agent.run_with_history()` em task separada
3. Warning experimental no stderr antes de entrar em raw mode

### Criterio de aceite

```
Given: theo --tui executado
When:  TUI inicia
Then:  terminal entra em raw mode + alternate screen
  AND  header mostra "theo" + modelo + modo
  AND  StatusLine mostra "AGENT" + keybind hints
  AND  input area mostra placeholder

Given: TUI rodando
When:  usuario pressiona Ctrl+C
Then:  TUI sai limpo (raw mode desabilitado, alternate screen restaurado)

Given: TUI rodando
When:  terminal e redimensionado
Then:  layout se adapta sem crash

Given: TUI rodando e agent executa task
When:  agent emite ContentDelta + ToolCallQueued + ToolCallCompleted
Then:  transcript atualiza em tempo real com texto + tool cards
```

### DoD

- [ ] `theo --tui` abre TUI, mostra layout, responde a Ctrl+C
- [ ] Terminal restaurado limpo em exit normal E em panic
- [ ] Agent loop funciona: digitar tarefa → ver resposta streaming
- [ ] Tool cards aparecem ao vivo com timer
- [ ] stdout de bash aparece linha a linha (se F1-T03 completou)
- [ ] `cargo test -p theo` passa (0 falhas)
- [ ] Teste manual em 3 terminais: Alacritty/WezTerm/tmux (documentar resultado)

---

## F1-T12: Testes de regressao e smoke test (theo-cli) [A]

**Tamanho:** S
**Arquivo:** `apps/theo-cli/src/main.rs` (testes), manual
**Depende de:** F1-T11

### Microtasks

1. Rodar `cargo test --workspace` — tudo passa
2. Rodar `theo "echo hello"` (sem --tui) — REPL funciona identico
3. Rodar `theo agent "echo hello"` — REPL funciona identico
4. Rodar `theo --tui` — TUI abre, responde, sai com Ctrl+C
5. Documentar matrix de terminais testados

### Criterio de aceite

```
Given: todas as tasks F1-T01 a F1-T11 completas
When:  cargo test --workspace
Then:  0 falhas, 0 warnings novos

Given: theo "echo hello" executado
When:  agente responde
Then:  output e identico ao antes da Fase 1

Given: theo --tui executado em Alacritty
When:  digito tarefa e agente executa
Then:  tool cards ao vivo, texto streaming, StatusLine atualizada
```

### DoD

- [ ] `cargo test --workspace` passa (0 falhas)
- [ ] 6 testes de session persistence em repl.rs passam
- [ ] 13 testes de BashTool passam
- [ ] 14 testes de EventBus passam (+ 4 novos de broadcast)
- [ ] Smoke test manual em 3 terminais documentado
- [ ] Zero warnings novos em todo o workspace

---

# FASE 2 — Navegacao e Historia

**Timebox:** 2 semanas
**Depende de:** F1 completa

---

## F2-T01: Scrollback ilimitado com viewport virtual (theo-cli) [A]

**Tamanho:** L
**Arquivo:** `tui/view.rs`, `tui/app.rs`

### Microtasks
1. Substituir renderizacao linear do transcript por viewport virtual (so renderiza linhas visiveis)
2. Adicionar `scroll_offset: usize` e `scroll_locked_to_bottom: bool` ao TuiState
3. Implementar PgUp/PgDn/j/k/Home/End para navegacao
4. Quando `scroll_locked_to_bottom` e novas mensagens chegam, auto-scroll
5. Quando usuario scrolla para cima, desativar auto-scroll
6. Mouse scroll com roda (crossterm MouseEvent)

### Criterio de aceite
```
Given: transcript com 200 mensagens em terminal 80x24
When:  PgUp pressionado
Then:  transcript scrolla uma pagina (24 linhas) para cima

Given: transcript scrollado para cima
When:  nova mensagem chega do agente
Then:  transcript NAO pula para o final (auto-scroll desativado)

Given: transcript scrollado para cima
When:  End pressionado
Then:  transcript pula para o final E auto-scroll reativado
```

### DoD
- [ ] Scroll funciona com PgUp/PgDn/j/k/Home/End
- [ ] Mouse scroll funciona
- [ ] Auto-scroll desativa quando usuario navega para cima
- [ ] Performance: 10.000 mensagens renderiza em <16ms (viewport virtual)
- [ ] Snapshot test atualizado

---

## F2-T02: Search inline com / (theo-cli) [A]

**Tamanho:** M
**Arquivo:** `tui/app.rs`, `tui/view.rs`

### Microtasks
1. Adicionar modo `SearchMode` ao TuiState (ativado por `/` quando nao estiver digitando)
2. Campo `search_query: String` e `search_results: Vec<usize>` (indices de mensagens)
3. Renderizar barra de busca sobre o transcript (overlay de 1 linha)
4. Fuzzy match sobre texto do transcript
5. Navigate entre resultados com n/N (next/prev)
6. Esc para sair do search mode
7. Highlight dos matches no transcript

### Criterio de aceite
```
Given: transcript com mensagem contendo "scoring.rs"
When:  usuario digita / e depois "scor"
Then:  mensagem contendo "scoring.rs" e highlighted E scroll posiciona la
```

### DoD
- [ ] / ativa search, Esc cancela
- [ ] n/N navega entre resultados
- [ ] Matches highlighted no transcript
- [ ] 2 testes: search_finds_match, search_no_match_shows_empty

---

## F2-T03: Session picker na inicializacao (theo-cli) [A]

**Tamanho:** M
**Arquivo:** `tui/app.rs`, `tui/view.rs`, novo `tui/session_picker.rs`

### Contexto (Finding 6)

O formato de persistencia real (`repl.rs:279-323`) e `Vec<Message>` em JSON sem metadados.
Nao ha data, preview ou hash dentro do JSON. O plano original assumia formato rico.

**Dados reais disponiveis:**
- Hash do projeto: e o FILENAME do arquivo de sessao (ex: `a1b2c3d4.json`)
- Data: mtime do arquivo no filesystem (`std::fs::metadata().modified()`)
- Preview: parsear `Vec<Message>`, pegar primeiro Message com role=user, truncar content em 60 chars
- Numero de mensagens: `vec.len()`

### Microtasks
1. Listar arquivos em `~/.config/theo/sessions/` com `glob("*.json")`
2. Para cada arquivo:
   - Hash do projeto = filename sem extensao
   - Filtrar: so mostrar se hash == hash do cwd atual (reusar `project_hash()` de repl.rs:269)
   - Data = `fs::metadata(path)?.modified()?` → formatar como "2026-04-14 15:30"
   - Carregar JSON, parsear como `Vec<Message>`, contar len()
   - Preview = primeiro Message com role "user" → truncar content em 60 chars
3. Ordenar por data (mais recente primeiro)
4. Renderizar lista com j/k/Enter/Esc:
   - `2026-04-14 15:30 · 12 msgs · "Fix the scoring bug in retr..."`
5. Enter: carregar sessao selecionada no TuiState
6. Esc ou `n`: nova sessao vazia
7. Mostrar picker apenas se existem sessoes para o projeto

### Criterio de aceite
```
Given: 3 arquivos JSON em ~/.config/theo/sessions/ com hash do projeto atual
When:  theo --tui inicia
Then:  picker mostra 3 sessoes ordenadas por mtime (mais recente primeiro)
  AND  cada entrada mostra data + num mensagens + preview do primeiro prompt

Given: picker visivel
When:  usuario seleciona sessao com Enter
Then:  transcript carrega Vec<Message> da sessao

Given: nenhum arquivo JSON com hash do projeto atual
When:  theo --tui inicia
Then:  picker NAO aparece, vai direto para sessao nova
```

### DoD
- [ ] Picker renderiza corretamente
- [ ] Usa formato real de persistencia (Vec<Message> em JSON)
- [ ] Data vem de mtime, nao de dentro do JSON
- [ ] Preview truncado em 60 chars
- [ ] Esc cria sessao nova
- [ ] 2 testes: picker_shows_sessions, picker_skipped_when_empty

---

## F2-T04: Help overlay com ? (theo-cli) [A]

**Tamanho:** S
**Arquivo:** `tui/view.rs`, `tui/app.rs`

### Microtasks
1. `?` quando nao estiver digitando abre overlay central
2. Renderizar tabela de keybinds agrupados:
   - Navegacao: PgUp/PgDn, j/k, Home/End, mouse scroll
   - Busca: /, n/N, Esc
   - Controle: Ctrl+C sair, Ctrl+L limpar
   - Input: Enter enviar, Shift+Enter nova linha
3. Esc fecha overlay
4. Overlay usa tui-overlay ou custom com ZIndex

### Criterio de aceite
```
Given: TUI rodando
When:  ? pressionado
Then:  overlay mostra tabela de keybinds centralizada

Given: help overlay aberto
When:  Esc pressionado
Then:  overlay fecha e TUI volta ao normal
```

### DoD
- [ ] Overlay renderiza centralizado
- [ ] Todas as keybinds listadas
- [ ] Snapshot test do overlay

---

## F2-T05: Markdown e syntax highlight no transcript (theo-cli) [A]

**Tamanho:** XL
**Arquivo:** novo `tui/markdown.rs`, `tui/view.rs`

### Microtasks
1. Criar `markdown.rs` — parser pulldown-cmark que converte markdown para ratatui Spans
2. Headings: bold + cor
3. Code inline: background gray
4. Code blocks: syntax highlight via `tui-syntax-highlight` com deteccao de linguagem
5. Listas: indentacao + bullet
6. Bold/italic: style correspondente
7. Links: underline cyan
8. Integrar markdown renderer no transcript view (assistant messages)
9. Manter user messages como texto plano (sem markdown)

### Criterio de aceite
```
Given: assistant message contendo "```rust\nfn main() {}\n```"
When:  renderizada no transcript
Then:  code block tem syntax highlight de Rust (keywords coloridos)

Given: assistant message contendo "**bold** and *italic*"
When:  renderizada no transcript
Then:  "bold" esta em bold E "italic" esta em italic
```

### DoD
- [ ] Markdown rendering para: headings, code inline, code blocks, bold, italic, lists, links
- [ ] Syntax highlight para pelo menos: Rust, TypeScript, Python, bash, JSON
- [ ] 5 testes de snapshot: plain text, code block, mixed markdown, nested list, long code
- [ ] Performance: rendering de 1000 linhas markdown em <16ms

---

## F2-T06: Toast notifications (theo-cli) [A]

**Tamanho:** S
**Arquivo:** `tui/view.rs`, `tui/app.rs`

### Microtasks
1. Integrar `ratatui-toaster` ou custom toast widget
2. Toast aparece no canto superior direito, max 60 chars largura
3. Tipos: info (azul), warning (amarelo), error (vermelho)
4. Auto-dismiss apos 3 segundos
5. BudgetExceeded gera toast warning
6. Error gera toast error
7. EventsLost gera toast info

### Criterio de aceite
```
Given: BudgetExceeded evento recebido
When:  processado pelo update
Then:  toast amarelo aparece "Budget exceeded" por 3 segundos
```

### DoD
- [ ] Toasts renderizam no canto correto
- [ ] Auto-dismiss funciona
- [ ] 2 testes: toast_shows_on_budget, toast_autodismiss

---

# FASE 3 — Controle do Agente

**Timebox:** 2 semanas
**Depende de:** F2 completa

---

## F3-T01: Model switcher em runtime (theo-agent-runtime, theo-cli) [C]

**Tamanho:** XL
**Arquivos:** `crates/theo-agent-runtime/src/run_engine.rs`, `crates/theo-infra-llm/`, `tui/`

### Microtasks
1. Mudar campo `client` em `AgentRunEngine` de `LlmClient` para `Arc<tokio::sync::RwLock<LlmClient>>`
2. Adicionar metodo `pub async fn swap_client(&self, new_client: LlmClient)` — adquire write lock SOMENTE entre iteracoes
3. Adicionar variante `ProviderSwitched` ao EventType (theo-domain)
4. Na TUI: Ctrl+M abre modal com lista de modelos do provider atual
5. Selecao de modelo chama `run_engine.swap_client(new_client)` + publica ProviderSwitched
6. StatusLine atualiza modelo apos swap

### Criterio de aceite
```
Given: agente rodando com gpt-4o
When:  Ctrl+M e usuario seleciona claude-3.5-sonnet
Then:  proxima iteracao do agent loop usa claude-3.5-sonnet
  AND  StatusLine mostra "claude-3.5-sonnet"
  AND  conversacao (historico) preservada

Given: agente no MEIO de uma chamada LLM
When:  swap_client() e chamado
Then:  chamada corrente termina com client antigo
  AND  proxima chamada usa client novo
```

### DoD
- [ ] Swap funciona sem perder contexto
- [ ] Swap durante chamada LLM e NO-OP ate completar
- [ ] Modal mostra modelos disponiveis
- [ ] Teste: swap_between_iterations, swap_during_call_is_noop
- [ ] /meeting proprio aprovado antes de implementar

---

## F3-T02: Copy de bloco via OSC52 (theo-cli) [A]

**Tamanho:** M
**Arquivo:** `tui/app.rs`, `tui/view.rs`

### Microtasks
1. Adicionar modo `SelectMode` ao TuiState
2. `v` entra em select mode, j/k move cursor de selecao, `y` copia
3. Copy via OSC52 escape sequence (funciona em tmux, SSH, etc): `\x1b]52;c;{base64}\x07`
4. Fallback: arboard para clipboard local
5. Visual feedback: area selecionada highlighted

### Criterio de aceite
```
Given: assistant message com code block
When:  v para select, move para code block, y para copiar
Then:  conteudo do code block esta no clipboard
  AND  toast confirma "Copiado para clipboard"
```

### DoD
- [ ] OSC52 funciona em terminais que suportam
- [ ] arboard fallback funciona
- [ ] Visual feedback de selecao
- [ ] 1 teste: select_and_copy_updates_state

---

## F3-T03: Interrupt com contexto (theo-agent-runtime, theo-cli) [B]

**Tamanho:** L
**Arquivo:** `crates/theo-agent-runtime/src/run_engine.rs`, `tui/app.rs`

### Microtasks
1. Ctrl+C durante execucao do agente envia sinal "soft interrupt" (nao SIGKILL)
2. RunEngine checa flag `interrupt_requested: AtomicBool` entre iteracoes
3. Se interrompido: salva context summary ("feito: X, Y. pendente: Z"), emite RunStateChanged com payload
4. TUI mostra: "Interrompido. Feito: X, Y. Continuar de onde parei? [s/n]"
5. Se "s": agente continua com contexto preservado
6. Se "n": sessao termina normalmente

### Criterio de aceite
```
Given: agente executando com 3 tool calls feitos e 2 pendentes
When:  Ctrl+C pressionado
Then:  agente para entre iteracoes (nao no meio de tool call)
  AND  TUI mostra resumo do que foi feito
  AND  prompt pergunta "continuar?"
```

### DoD
- [ ] Interrupt e graceful (entre iteracoes)
- [ ] Context summary gerado automaticamente
- [ ] Continuar preserva estado
- [ ] 2 testes: interrupt_saves_context, continue_resumes

---

## F3-T04: Mode switcher visual (theo-cli) [A]

**Tamanho:** S
**Arquivo:** `tui/app.rs`, `tui/view.rs`

### Microtasks
1. Shift+Tab cicla entre agent/plan/ask
2. StatusLine atualiza modo
3. Visual indicator: modo ativo highlighted, outros dimmed
4. Modo "plan" desabilita execucao de tools (so planeja)
5. Modo "ask" desabilita tools e state machine (so responde)

### Criterio de aceite
```
Given: TUI em modo agent
When:  Shift+Tab pressionado
Then:  modo muda para plan E StatusLine reflete
```

### DoD
- [ ] 3 modos funcionam
- [ ] StatusLine atualiza
- [ ] 1 teste: mode_cycles_correctly

---

## F3-T05: Edicao do ultimo prompt (theo-cli) [A]

**Tamanho:** S
**Arquivo:** `tui/app.rs`

### Microtasks
1. Ctrl+Up quando input vazio copia ultimo prompt do usuario para o input
2. Usuario pode editar e re-enviar
3. Se input nao esta vazio, Ctrl+Up faz nada

### Criterio de aceite
```
Given: transcript com user message "fix bug in scoring.rs" e input vazio
When:  Ctrl+Up pressionado
Then:  input area contem "fix bug in scoring.rs" editavel
```

### DoD
- [ ] Funciona com input vazio
- [ ] Nao faz nada com input preenchido
- [ ] 1 teste: ctrl_up_restores_last_prompt

---

# FASE 4 — Decision Control Plane Visivel

**Timebox:** 4-5 semanas (revisado — Finding 2: arquitetura nova)
**Depende de:** F3 completa + ADR-004 aprovado
**Classe:** [C] — requer nova arquitetura

---

## F4-T00: ADR-004 — Approval Gate interativo [C]

**Tamanho:** XL
**Arquivo:** `docs/adr/004-interactive-approval-gate.md`
**Bloqueia:** F4-T01, F4-T02

### Contexto (Finding 2)

O runtime atual so tem `CapabilityGate` (`capability_gate.rs:30-55`) que faz check
binario allow/deny. NAO existe handshake interativo, oneshot channel, nem "esperar TUI
aprovar". O plano original assumia uma infra que nao existe.

Este ADR deve definir:
- Trait `ApprovalGate` com metodo `async fn request_approval(...) -> ApprovalOutcome`
- Como integra com CapabilityGate existente (composicao, nao substituicao)
- Protocolo: runtime emite Pending, pausa via oneshot, TUI resolve, runtime continua
- Onde o gate e inserido no ToolCallManager (entre Dispatched e Running?)
- Como funciona em modo CLI legado (auto-approve? deny? prompt stdin?)
- Testes de timeout (se TUI nao responde em 5min, auto-reject)

### Criterio de aceite
```
Given: ADR-004 escrito
When:  revisado por governance + runtime
Then:  aprovado em /meeting
  AND  define trait ApprovalGate
  AND  define integracao com CapabilityGate
  AND  define protocolo oneshot
  AND  define fallback para CLI legado
```

### DoD
- [ ] ADR escrito em docs/adr/004-interactive-approval-gate.md
- [ ] /meeting aprovado com governance + runtime

---

## F4-T01: Implementar ApprovalGate trait e runtime integration [C]

**Tamanho:** XL
**Arquivos:** `theo-domain/`, `theo-agent-runtime/src/tool_call_manager.rs`
**Depende de:** F4-T00 (ADR-004)

### Microtasks
1. Definir trait `ApprovalGate` em theo-domain (ou theo-agent-runtime)
2. Adicionar variantes GovernanceDecisionPending e GovernanceDecisionResolved ao EventType
3. Implementar `TuiApprovalGate` que usa oneshot channel
4. Implementar `AutoApproveGate` para CLI legado e testes
5. Integrar no ToolCallManager: entre Dispatched e Running, chamar gate.request_approval()
6. Timeout de 5 minutos com auto-reject

### Criterio de aceite
```
Given: ToolCallManager com TuiApprovalGate
When:  dispatch_and_execute() para tool de alto risco
Then:  GovernanceDecisionPending publicado
  AND  execucao pausa
  AND  apos TUI enviar Approved via oneshot: tool executa

Given: ToolCallManager com AutoApproveGate
When:  dispatch_and_execute()
Then:  tool executa sem pausa (backward compatible)
```

### DoD
- [ ] ApprovalGate trait definido
- [ ] TuiApprovalGate e AutoApproveGate implementados
- [ ] Testes: approval_flow, rejection_flow, timeout_auto_reject, auto_approve_gate
- [ ] CapabilityGate existente intacto (composicao)
- [ ] /meeting proprio aprovado

---

## F4-T02: Approval modal na TUI [A, bloqueada por F4-T01 C]

**Tamanho:** L
**Arquivo:** `tui/widgets/approval_modal.rs`, `tui/app.rs`, `tui/view.rs`
**Depende de:** F4-T01

### Microtasks
1. Modal centralizado aparece quando GovernanceDecisionPending chega via EventBus
2. Mostra: tool_name, risk_level (cor), summary do que vai ser feito
3. Keys: `a` approve, `r` reject, `d` show diff (se apply_patch)
4. Modal bloqueia input para transcript (focus trap)
5. Apos decisao, envia resposta via oneshot channel ao runtime (usando TuiApprovalGate)

### Criterio de aceite
```
Given: GovernanceDecisionPending para "bash rm -rf /tmp/test" com risk HIGH
When:  modal aparece
Then:  mostra risk em vermelho, comando, e opcoes [a/r/d]

Given: modal aberto
When:  usuario pressiona 'a'
Then:  modal fecha, tool executa, GovernanceDecisionResolved(approved) emitido
```

### DoD
- [ ] Modal renderiza centralizado
- [ ] Focus trap funciona
- [ ] a/r/d funcionam
- [ ] Snapshot test do modal

---

## F4-T03: Diff preview inline (theo-cli) [A]

**Tamanho:** L
**Arquivo:** novo `tui/widgets/diff_viewer.rs`

### Microtasks
1. Usar `similar` crate para computar diff unified
2. Renderizar: linhas removidas em vermelho, adicionadas em verde, contexto em cinza
3. Scroll vertical no diff (j/k)
4. Integrar com approval modal: `d` abre diff de apply_patch
5. Side-by-side mode opcional (toggle com `s`)

### Criterio de aceite
```
Given: apply_patch com 3 hunks
When:  diff viewer aberto
Then:  mostra linhas +/- com cores corretas E hunk headers
```

### DoD
- [ ] Unified diff renderiza
- [ ] Side-by-side opcional
- [ ] Scroll funciona
- [ ] 2 testes: diff_unified_snapshot, diff_sidebyside_snapshot

---

## F4-T04: Timeline de causalidade (theo-cli) [A]

**Tamanho:** M
**Arquivo:** `tui/widgets/timeline.rs`, `tui/app.rs`

### Microtasks
1. Rastrear cadeia causal: LLM response -> tool calls -> resultados -> proxima decisao
2. Renderizar como arvore: "grep scoring.rs → 3 matches → Edit scoring.rs → tests pass"
3. Cada no mostra: tool_name, status, duracao
4. Acessivel via `t` (toggle timeline panel)

### Criterio de aceite
```
Given: agente fez grep -> read -> edit -> test em sequencia
When:  timeline aberta com 't'
Then:  mostra cadeia com setas: grep → read → edit → test
  AND  cada no tem status (ok/fail) e duracao
```

### DoD
- [ ] Timeline renderiza cadeia causal
- [ ] Toggle com 't'
- [ ] 1 teste: timeline_shows_chain

---

## F4-T05: Undo last tool via snapshot (theo-tooling, theo-cli) [B]

**Tamanho:** L
**Arquivos:** `theo-tooling/src/undo.rs` (novo), `theo-tooling/src/registry.rs`, `tui/app.rs`

### Contexto (Finding 3)

O plano original usava `git checkout -- <file>`. Isso e destrutivo: se agente fez 3 edits
no mesmo arquivo, undo do primeiro apaga os outros dois. Tambem contradiz postura de
seguranca do projeto (ADR-002).

**Solucao real:** snapshot-based undo. ANTES de cada write/edit, salvar conteudo original
em `~/.config/theo/undo/{session_id}/{call_id}.bak`. UndoTool restaura do backup.

### Microtasks
1. Criar modulo `theo-tooling/src/undo.rs`:
   - `SnapshotStore` struct com metodos `save(call_id, file_path, content)` e `restore(call_id) -> (path, content)`
   - Storage em `~/.config/theo/undo/{session_id}/`
   - Cleanup automatico ao final da sessao (ou apos 24h)
2. Modificar WriteTool e EditTool: antes de escrever, chamar `snapshot_store.save(ctx.call_id, path, old_content)`
3. Criar UndoTool que implementa Tool trait:
   - Schema: `{ call_id: string }` — restaura arquivo do snapshot
   - Category: Execution (passa pelo DCP)
   - Se snapshot nao existe: retorna erro amigavel, NAO faz git checkout
4. Registrar no ToolRegistry
5. `U` na TUI: identifica ultimo tool call de write/edit, cria tool call sintetico para UndoTool
6. Mostra diff revertido no transcript

### Criterio de aceite
```
Given: agente editou scoring.rs (call_id "c-1") e snapshot foi salvo
When:  usuario pressiona U
Then:  UndoTool restaura scoring.rs do snapshot de "c-1"
  AND  passa pelo DCP antes de restaurar
  AND  transcript mostra diff revertido

Given: agente fez 3 edits no MESMO arquivo (c-1, c-2, c-3)
When:  usuario pressiona U (undo c-3)
Then:  arquivo restaura para estado PRE-c-3 (nao pre-c-1)
  AND  edits c-1 e c-2 preservados

Given: snapshot nao existe para call_id
When:  UndoTool.execute()
Then:  retorna Err(ToolError) amigavel, NAO faz git checkout
```

### DoD
- [ ] UndoTool registrada no registry
- [ ] Passa pelo DCP (nao bypassa governanca)
- [ ] NAO usa git checkout (snapshot-based)
- [ ] 3 testes: undo_restores_snapshot, undo_preserves_earlier_edits, undo_missing_snapshot_errors
- [ ] /meeting proprio aprovado

---

## F4-T06: tachyonfx dissolve em tool completion (theo-cli) [A]

**Tamanho:** S
**Arquivo:** `tui/view.rs`

### Microtasks
1. Adicionar dep tachyonfx
2. Quando tool card transiciona de Running para Done: aplicar dissolve_out (300ms)
3. Quando tool card transiciona para Failed: aplicar glitch effect (200ms)
4. Efeitos nao bloqueiam render (async ticks)

### Criterio de aceite
```
Given: tool card em Running
When:  ToolCallCompleted chega com success=true
Then:  card faz dissolve visual de 300ms antes de ficar no estado final
```

### DoD
- [ ] Dissolve funciona
- [ ] Glitch em falha funciona
- [ ] Nao bloqueia render
- [ ] Nao usa efeitos decorativos (so comunicativos)

---

# FASE 5 — Painel de Tarefas e Contexto

**Timebox:** 3 semanas (revisado — Finding 5: requer backend de eventos)
**Depende de:** F4 completa

---

## F5-T00: Backend — Publicar eventos ausentes para sidebar [B]

**Tamanho:** L
**Arquivos:** `theo-agent-runtime/src/run_engine.rs`, `theo-application/src/use_cases/graph_context_service.rs`

### Contexto (Finding 5, revisado com Finding 3 Rev.2)

A sidebar precisa de 3 streams de dados que NAO existem no runtime:
1. `TodoUpdated` — existe no enum (event.rs:32) mas NENHUM crate publica este evento
2. GRAPHCTX — `GraphContextService` (`graph_context_service.rs:96`) nao integra com EventBus,
   so fornece `initialize()` e `query_context()`
3. Subagentes — sinalizados como strings em RunStateChanged ("SubAgent:Explorer", "SubAgentParallel:3"),
   nao como eventos estruturados com spawn/complete lifecycle

**Sobre TodoUpdated:** as tools reais sao `task_create` (`todo/mod.rs:118`) e `task_update`
(`todo/mod.rs:170`), NAO "todo". Elas sao **stateless** — retornam metadados pontuais e
dizem explicitamente que "the runtime should track it" (`todo/mod.rs:147`).
A lista canonica de todos NAO existe no runtime hoje. Opcoes:
- **Opcao A (propagacao incremental):** publicar TodoUpdated com payload do ToolOutput.metadata
  apos cada task_create/task_update. TUI reconstroi estado acumulando eventos. Simples, mas
  TUI pode perder sync se eventos forem dropados (broadcast Lagged).
- **Opcao B (store no runtime):** criar `TodoStore` no RunEngine que acumula todos.
  Publicar TodoUpdated com snapshot completo `{"todos": [...]}` apos cada mudanca.
  TUI sempre tem estado completo. Mais correto, mais codigo.
**Decisao:** Opcao A para F5. Se Lagged causar problemas, migrar para Opcao B.

### Microtasks
1. **TodoUpdated — prerequisito: preservar metadata no tool_bridge.**
   Hoje `tool_bridge::execute_tool_call()` descarta `ToolOutput.metadata` (tool_bridge.rs:217-223):
   so `output.output` e usado para construir `Message::tool_result()`. O `ToolResultRecord`
   (tool_call.rs:132-142) tambem nao tem campo metadata.

   **Correcao necessaria antes de emitir TodoUpdated:**
   a) Adicionar campo `pub metadata: Option<serde_json::Value>` ao `ToolResultRecord` (tool_call.rs:132)
   b) Em `tool_bridge::execute_tool_call()` (tool_bridge.rs:217), preservar metadata:
      retornar `(Message, bool, Option<serde_json::Value>)` — terceiro elemento e `output.metadata`
   c) Em `ToolCallManager::dispatch_and_execute()` (tool_call_manager.rs:143), armazenar
      metadata no ToolResultRecord
   d) Apos tool_bridge retornar, checar metadata.type == "task_create"|"task_update":
      se sim, publicar `TodoUpdated` com payload da metadata

   **Impacto:** tool_bridge muda de `(Message, bool)` para `(Message, bool, Option<Value>)`.
   Todos os call sites de execute_tool_call precisam atualizar (grep mostra ~3 sites).
   Isso e extensao moderada [B], nao trivial.
2. **GRAPHCTX:** adicionar metodo `pub fn subscribe_events(&self, bus: Arc<EventBus>)` ao
   GraphContextService. Publicar evento custom (novo EventType `GraphContextUpdated`) quando
   `initialize()` ou `query_context()` atualiza o grafo. Payload: lista de arquivos + scores
3. **Subagentes:** criar EventType `SubAgentSpawned` e `SubAgentCompleted`. Publicar em
   run_engine.rs onde hoje publica RunStateChanged com "SubAgent:*" (linhas 810-815, 867-873).
   Manter RunStateChanged tambem (backward compat)

### Criterio de aceite
```
Given: agente chama task_create com content "Fix scoring bug"
When:  ToolCallCompleted chega com metadata.type == "task_create"
Then:  TodoUpdated publicado com {"action": "create", "content": "Fix scoring bug"}

Given: agente chama task_update com id "1" e status "completed"
When:  ToolCallCompleted chega com metadata.type == "task_update"
Then:  TodoUpdated publicado com {"action": "update", "id": "1", "status": "completed"}

Given: GraphContextService inicializa indice
When:  initialize() completa
Then:  GraphContextUpdated publicado com lista de arquivos indexados

Given: agente spawna subagente Explorer
When:  run_engine cria subagente (run_engine.rs:810)
Then:  SubAgentSpawned publicado com {role: "Explorer", task_summary: "..."}
  AND  SubAgentCompleted publicado quando subagente termina
```

### DoD
- [ ] TodoUpdated publicado pelo runtime (nao so definido no enum)
- [ ] GraphContextUpdated definido e publicado
- [ ] SubAgentSpawned/Completed definidos e publicados
- [ ] Testes para cada evento novo
- [ ] /meeting proprio aprovado (3 variantes novas de EventType)

---

## F5-T01: Sidebar com toggle (theo-cli) [A]

**Tamanho:** M
**Arquivo:** `tui/view.rs`, `tui/app.rs`
**Depende de:** F5-T00

### Microtasks
1. Tab toggle sidebar direita (~40 cols)
2. Layout condicional: se terminal > 120 cols, auto-show; senao manual
3. Sidebar tem 3 tabs: Todos, GRAPHCTX, Agents

### Criterio de aceite
```
Given: terminal 200 cols
When:  TUI inicia
Then:  sidebar visivel automaticamente

Given: terminal 80 cols
When:  Tab pressionado
Then:  sidebar abre/fecha
```

### DoD
- [ ] Toggle funciona
- [ ] Auto-show em terminais largos
- [ ] 3 tabs navegaveis

---

## F5-T02: TodoList ao vivo na sidebar [A]

**Tamanho:** S
**Arquivo:** `tui/widgets/todo_list.rs`
**Depende de:** F5-T00 (TodoUpdated sendo publicado)

### Microtasks
1. Consumir eventos TodoUpdated do EventBus via broadcast
2. Renderizar lista: checkbox + texto + status
3. Atualizar em tempo real

### DoD
- [ ] Todos aparecem em tempo real
- [ ] Status atualiza quando mudado

---

## F5-T03: GRAPHCTX status na sidebar [A]

**Tamanho:** M
**Arquivo:** `tui/widgets/graphctx_panel.rs`
**Depende de:** F5-T00 (GraphContextUpdated sendo publicado)

### Microtasks
1. Consumir eventos GraphContextUpdated do EventBus
2. Mostrar lista de arquivos no contexto: caminho, score
3. Usar tui-tree-widget para hierarquia
4. Atualizar conforme GRAPHCTX muda

### DoD
- [ ] Arquivos mostrados com hierarquia
- [ ] Score visivel
- [ ] Atualiza em tempo real

---

## F5-T04: Phase indicator e budget visual (theo-cli) [A]

**Tamanho:** S
**Arquivo:** `tui/view.rs`

### Microtasks
1. Barra de progresso na StatusLine: LOCATE → EDIT → VERIFY → DONE (segmentos coloridos)
2. Budget: barra de tokens usados/total + custo estimado
3. Atualizar via RunStateChanged e LlmCallEnd

### DoD
- [ ] Fase visivel e atualiza
- [ ] Budget renderiza como barra
- [ ] Cor muda quando budget > 80%

---

## F5-T05: Sub-agent tree (theo-cli) [A]

**Tamanho:** M
**Arquivo:** `tui/widgets/agent_tree.rs`

### Microtasks
1. Rastrear sub-agents spawned pelo runtime
2. Renderizar como arvore: agent principal → sub-agent 1 → sub-sub-agent
3. Cada no: role, status (running/done), duracao

### DoD
- [ ] Arvore renderiza
- [ ] Status atualiza em tempo real

---

# FASE 6 — Multi-Sessao e Workspace

**Timebox:** 2 semanas
**Depende de:** F5 completa

---

## F6-T01: Tabs de sessao (theo-cli) [A]

**Tamanho:** L
**Arquivo:** `tui/app.rs`, `tui/view.rs`

### Microtasks
1. Ctrl+T cria nova sessao (nova tab)
2. Ctrl+W fecha tab atual
3. Ctrl+1..9 navega entre tabs
4. Header mostra tabs: `[1: scoring fix] [2: auth refactor*] [3: new session]`
5. Cada tab tem seu proprio TuiState + EventBus

### Criterio de aceite
```
Given: 1 sessao aberta
When:  Ctrl+T
Then:  nova tab criada, header mostra 2 tabs
```

### DoD
- [ ] Tabs funcionam
- [ ] Cada tab independente
- [ ] Ctrl+W confirma antes de fechar sessao ativa

---

## F6-T02: Export sessao como markdown (theo-cli) [A]

**Tamanho:** S
**Arquivo:** `tui/app.rs`

### Microtasks
1. Ctrl+S ou /export: gera arquivo .md com transcript completo
2. Salva em `~/.config/theo/exports/{date}-{hash}.md`
3. Toast confirma: "Exportado para ~/.config/theo/exports/..."

### DoD
- [ ] Markdown gerado com formatacao correta
- [ ] Toast confirma

---

## F6-T03: Busca global em historico (theo-cli) [A]

**Tamanho:** M
**Arquivo:** `tui/widgets/global_search.rs`

### Microtasks
1. Ctrl+Shift+F ou /search: busca em TODAS as sessoes do projeto
2. Renderiza lista de resultados com preview (data, trecho, sessao)
3. Enter abre sessao no resultado

### DoD
- [ ] Busca funciona across sessoes
- [ ] Preview legivel
- [ ] Enter abre sessao correta

---

# FASE 7 — Polimento e Superioridade

**Timebox:** 2 semanas
**Depende de:** F6 completa

---

## F7-T01: Theme engine com opaline (theo-cli) [A, dep:opaline nao presente]

**Tamanho:** M
**Arquivo:** `tui/theme.rs`

### Microtasks
1. Integrar opaline (20 temas built-in)
2. Configuracao via `~/.config/theo/tui.toml`: `theme = "dracula"`
3. /theme ou Ctrl+Shift+T para picker visual
4. Dark/light/high-contrast como categorias

### DoD
- [ ] 20 temas funcionam
- [ ] Picker visual
- [ ] Config persistente

---

## F7-T02: Keybinds configuraveis (theo-cli) [A]

**Tamanho:** M
**Arquivo:** `tui/keybinds.rs`

### Microtasks
1. Carregar de `~/.config/theo/keybinds.toml`
2. Formato: `quit = "ctrl+c"`, `search = "/"`
3. Default sane: nao precisa de config para funcionar
4. /keybinds mostra configuracao atual

### DoD
- [ ] Custom keybinds funcionam
- [ ] Defaults sao completos
- [ ] /keybinds mostra config

---

## F7-T03: TUI vira default, REPL vira --legacy (theo-cli) [A]

**Tamanho:** S
**Arquivo:** `apps/theo-cli/src/main.rs`

### Microtasks
1. Inverter flag: default = TUI, `--legacy` ativa REPL rustyline
2. Remover warning experimental
3. Atualizar --help

### Criterio de aceite
```
Given: theo "task" executado sem flags
When:  agente inicia
Then:  TUI ratatui e usado (nao REPL)

Given: theo --legacy "task"
When:  agente inicia
Then:  REPL rustyline e usado
```

### DoD
- [ ] Default e TUI
- [ ] --legacy funciona
- [ ] --help atualizado
- [ ] /meeting aprovado para esta mudanca

---

## F7-T04: Benchmark de latencia de render (theo-cli) [A]

**Tamanho:** S
**Arquivo:** `apps/theo-benchmark/` ou teste dedicado

### Microtasks
1. Benchmark: renderizar 100 frames com transcript de 1000 mensagens
2. Medir latencia media e p99
3. Meta: media < 8ms, p99 < 16ms (60fps)
4. Se falhar: identificar bottleneck (provavelmente markdown rendering)

### DoD
- [ ] Benchmark existe e roda
- [ ] p99 < 16ms
- [ ] Resultados documentados

---

## F7-T05: Notificacao de tarefa concluida (theo-cli) [A]

**Tamanho:** S
**Arquivo:** `tui/app.rs`

### Microtasks
1. Quando agente completa tarefa e TUI esta em background (terminal nao tem foco):
   - Linux: `notify-send "Theo" "Tarefa concluida"`
   - macOS: `osascript -e 'display notification "..." with title "Theo"'`
2. Condicional: so notifica se comando demorou > 10s

### DoD
- [ ] Notificacao funciona em Linux
- [ ] Condicional de tempo funciona

---

# Resumo de Metricas (Rev.1)

| Fase | Tasks | Classes | Timebox |
|------|-------|---------|---------|
| F1 | 13 | 7[A] + 6[B] | 2 semanas |
| F2 | 6 | 5[A] + 1[B] | 2 semanas |
| F3 | 5 | 3[A] + 1[B] + 1[C] (model switcher) | 2 semanas |
| F4 | 6 | 2[A] + 2[B] + 2[C] (ADR-004 + approval gate) | 4-5 semanas |
| F5 | 6 | 3[A] + 3[B] (backend eventos ausentes) | 3 semanas |
| F6 | 3 | 3[A] | 2 semanas |
| F7 | 5 | 4[A] + 1[B] | 2 semanas |
| **Total** | **44 tasks** | **27[A] + 14[B] + 3[C]** | **~17-18 semanas** |

**[A] Implementavel agora:** 27 tasks — consomem APIs que ja existem
**[B] Extensao moderada:** 14 tasks — adicionam campos/eventos sem mudar contratos
**[C] Arquitetura nova:** 3 tasks — requerem ADR proprio (F4-T00, F4-T01, F3-T01)

---

# Dependencias Criticas (caminho critico)

```
F1-T01 (domain) ─┬─► F1-T02 (broadcast) ─────────────────────────┐
                  │                                                 │
                  ├─► F1-T03 (streaming executor) ─► F1-T04 (emit) ┤
                  │                                                 │
                  └─► F1-T06 (workspace deps) ─► F1-T07 (flag) ────┤
                                                                    │
F1-T05 (renderer fix) ─────────────────────────────────────────────┤
                                                                    │
                                                          F1-T08 (state/msg) ─► F1-T09 (view) ─► F1-T11 (run)
                                                                    │                                   │
                                                          F1-T10 (events/input) ────────────────────────┤
                                                                                                        │
                                                                                              F1-T12 (smoke)
```

**Caminho critico F1:** T01 → T03 → T04 → T08 → T09 → T11 → T12 (~30h)
**Parallelizavel:** T02, T05, T06 podem rodar em paralelo com T03

---

# Gates por Fase

Cada fase EXIGE antes de iniciar:
1. `/meeting` proprio aprovado
2. Fase anterior com todos os DoDs verdes
3. `cargo test --workspace` passando
4. Zero warnings novos
5. Smoke test manual documentado
