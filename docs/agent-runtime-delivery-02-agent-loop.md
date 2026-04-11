# Agent Runtime Delivery 02 — Agent Loop

Esta entrega restringe o escopo ao nucleo do `agent_loop` em `crates/theo-agent-runtime`.

## Objetivo

Reduzir a dependencia estrutural do caminho principal do `AgentLoop` em APIs deprecated, mantendo compatibilidade explicita apenas onde ainda for necessaria.

## Mudancas entregues

### 1. Novo construtor principal do `AgentLoop`

Antes:

- `AgentLoop::new(config, registry, event_sink)`

Agora:

- `AgentLoop::new(config, registry)`

Estrategia:

- o caminho principal do runtime nao exige mais `EventSink`
- listeners nativos agora podem ser anexados por `with_event_listener(...)`
- compatibilidade legada fica explicita em `with_legacy_event_sink(...)`

### 2. Listener/legacy split dentro do `AgentLoop`

O `AgentLoop` agora separa:

- `listeners: Vec<Arc<dyn EventListener>>`
- `legacy_event_sink: Option<Arc<dyn EventSink>>`

Consequencia:

- `EventBus` e a infraestrutura principal
- `EventSinkBridge` passa a existir apenas como adaptador de compatibilidade

### 3. `run()` e `run_with_history()` usam `attach_listeners()`

O loop agora:

- anexa listeners nativos
- so anexa `EventSinkBridge` se houver sink legado configurado

Isso tira o legado do caminho obrigatorio.

### 4. Migração dos callers principais

Migrados para o novo construtor sem legado:

- `apps/theo-cli/src/repl.rs`
- `apps/theo-cli/src/init.rs`
- `crates/theo-agent-runtime/src/pilot.rs`
- `crates/theo-agent-runtime/src/subagent/mod.rs`

Mantidos em compatibilidade explicita:

- `crates/theo-agent-runtime/src/bin/theo-agent.rs`
- `crates/theo-application/src/use_cases/run_agent_session.rs`

Nesses casos, a compatibilidade continua via:

- `.with_legacy_event_sink(...)`

### 5. Limpeza associada

- removidos imports obsoletos em `pilot.rs` e `subagent/mod.rs`
- removido acoplamento deprecated de imports de topo em `agent_loop.rs`
- atualizado teste de contrato em `run_engine.rs` para a assinatura nova

## Validacao executada

Comandos:

```bash
cargo fmt --all
cargo test -q -p theo-agent-runtime
cargo test -q -p theo
```

Resultados:

- `theo-agent-runtime`: 334 testes passando
- `theo`: 23 testes passando

## DoD validado

### DoD 1

O caminho principal do `AgentLoop` nao depende mais de `EventSink`.

Status:

- validado

Evidencia:

- novo construtor principal sem sink
- REPL/init/pilot/subagent migrados para ele

### DoD 2

Compatibilidade legada ficou isolada atras de adaptador explicito.

Status:

- validado

Evidencia:

- `with_legacy_event_sink(...)`
- `EventSinkBridge` usado apenas quando solicitado

### DoD 3

`AgentLoop` continua sendo fachada fina sobre `AgentRunEngine`.

Status:

- validado

Evidencia:

- a refatoracao mudou injecao de listeners, nao o fluxo de execucao do engine

### DoD 4

Callers centrais do runtime continuam funcionando apos a mudanca.

Status:

- validado

Evidencia:

- `cargo test -q -p theo-agent-runtime`
- `cargo test -q -p theo`

## O que ainda nao foi resolvido

Esta entrega nao removeu todo o legado do `theo-agent-runtime`.

Ainda restam:

- `events.rs` como superficie deprecated exportada
- `state.rs` / `Phase` / `AgentState` ainda usados em `run_engine` para diagnostico/context loop
- `lib.rs` ainda reexporta tipos deprecated
- `theo-application` e desktop ainda usam compatibilidade de `EventSink`

## Avaliacao da entrega

Entrega aprovada.

O `agent_loop` agora tem um caminho principal mais limpo:

- EventBus primeiro
- compatibilidade legada opcional
- construtor principal sem API deprecated

Isso reduz o acoplamento do runtime central ao modelo antigo sem quebrar os consumidores existentes.
