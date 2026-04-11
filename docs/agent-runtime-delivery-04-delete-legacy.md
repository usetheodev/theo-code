# Agent Runtime Delivery 04 — Delete Legacy

Esta entrega remove a camada legada restante do `theo-agent-runtime`.

## Objetivo

Eliminar completamente as APIs e bridges de compatibilidade do runtime:

- `events.rs`
- `state.rs`
- `EventSink`
- `AgentEvent`
- `with_legacy_event_sink(...)`
- `EventSinkBridge`

e migrar os consumidores imediatos para a API nativa baseada em `EventBus` e `EventListener`.

## Mudancas entregues

### 1. `AgentLoop` sem legado

Removidos de [agent_loop.rs](/home/paulo/Projetos/usetheo/theo-code/crates/theo-agent-runtime/src/agent_loop.rs):

- campo `legacy_event_sink`
- metodo `with_legacy_event_sink(...)`
- `EventSinkBridge`
- testes dedicados ao bridge legado

O `AgentLoop` agora expõe apenas a integracao nativa:

- `with_event_listener(...)`

### 2. Runtime sem modulos legados

Removidos do crate:

- [events.rs](/home/paulo/Projetos/usetheo/theo-code/crates/theo-agent-runtime/src/events.rs)
- [state.rs](/home/paulo/Projetos/usetheo/theo-code/crates/theo-agent-runtime/src/state.rs)

E removidos de [lib.rs](/home/paulo/Projetos/usetheo/theo-code/crates/theo-agent-runtime/src/lib.rs):

- `pub mod events`
- `pub mod state`

O estado ativo do loop permanece em:

- [loop_state.rs](/home/paulo/Projetos/usetheo/theo-code/crates/theo-agent-runtime/src/loop_state.rs)

### 3. Consumidores migrados

#### Application layer

[run_agent_session.rs](/home/paulo/Projetos/usetheo/theo-code/crates/theo-application/src/use_cases/run_agent_session.rs)

- antes: recebia `Arc<dyn EventSink>`
- agora: recebe `Arc<dyn EventListener>`

#### Desktop

[events.rs](/home/paulo/Projetos/usetheo/theo-code/apps/theo-desktop/src/events.rs)

- antes: `TauriEventSink` baseado em `AgentEvent`
- agora: `TauriEventListener` baseado em `DomainEvent`

[chat.rs](/home/paulo/Projetos/usetheo/theo-code/apps/theo-desktop/src/commands/chat.rs)

- passa `TauriEventListener` para `run_agent_session`
- emite `FrontendEvent::Done` no encerramento da execucao

[Cargo.toml](/home/paulo/Projetos/usetheo/theo-code/apps/theo-desktop/Cargo.toml)

- adicionada dependencia em `theo-domain`

#### Binario do runtime

[theo-agent.rs](/home/paulo/Projetos/usetheo/theo-code/crates/theo-agent-runtime/src/bin/theo-agent.rs)

- antes: `PrintEventSink`
- agora: `PrintEventListener`

### 4. Limpeza associada

Removidos allows e nomenclatura de compatibilidade em:

- [run_engine.rs](/home/paulo/Projetos/usetheo/theo-code/crates/theo-agent-runtime/src/run_engine.rs)
- [subagent/mod.rs](/home/paulo/Projetos/usetheo/theo-code/crates/theo-agent-runtime/src/subagent/mod.rs)
- [pilot.rs](/home/paulo/Projetos/usetheo/theo-code/crates/theo-agent-runtime/src/pilot.rs)

## Validacao executada

Comandos:

```bash
cargo fmt --all
cargo test -q -p theo-agent-runtime
cargo test -q -p theo-application
cargo test -q -p theo-code-desktop
cargo test -q -p theo
```

Resultados:

- `theo-agent-runtime`: 325 testes passando
- `theo-application`: 80 testes efetivos passando, 1 ignorado
- `theo-code-desktop`: build/test passando
- `theo`: 23 testes passando

Observacoes:

- permanecem warnings fora deste escopo em crates como `theo-engine-retrieval` e `theo-application`
- nao restaram warnings de deprecacao ligados a `EventSink`, `AgentEvent`, `state.rs` ou `events.rs`

## DoD validado

### DoD 1

Nao existe mais API legada de eventos no runtime.

Status:

- validado

Evidencia:

- `events.rs` deletado
- `with_legacy_event_sink(...)` removido
- `EventSinkBridge` removido

### DoD 2

Nao existe mais API legada de estado/fase no runtime.

Status:

- validado

Evidencia:

- `state.rs` deletado
- `loop_state.rs` virou unica API de estado do loop

### DoD 3

Consumidores imediatos do runtime foram migrados para a API nova.

Status:

- validado

Evidencia:

- `theo-application` usa `EventListener`
- desktop usa `TauriEventListener`
- binario usa `PrintEventListener`

### DoD 4

O workspace continua compilando e os caminhos principais seguem validados.

Status:

- validado

Evidencia:

- `cargo test -q -p theo-agent-runtime`
- `cargo test -q -p theo-application`
- `cargo test -q -p theo-code-desktop`
- `cargo test -q -p theo`

## Avaliacao da entrega

Entrega aprovada.

O `theo-agent-runtime` deixou de expor uma superficie dupla entre runtime novo e compatibilidade antiga. O nucleo agora fala uma linguagem unica:

- `EventBus`
- `EventListener`
- `ContextLoopState`
- `LoopPhase`
