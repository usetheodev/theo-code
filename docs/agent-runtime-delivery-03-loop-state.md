# Agent Runtime Delivery 03 — Loop State

Esta entrega continua o cleanup do nucleo de `crates/theo-agent-runtime`, com foco em tirar `AgentState` e `Phase` deprecated do caminho ativo de execucao.

## Objetivo

Trocar o estado de diagnostico/context loop do `AgentRunEngine` para a API nova em `loop_state.rs`, mantendo `state.rs` apenas como camada de compatibilidade.

## Mudancas entregues

### 1. Novo estado principal do engine

O `AgentRunEngine` agora usa:

- `ContextLoopState`

no lugar de:

- `AgentState`

Consequencia:

- o loop ativo do runtime nao depende mais de alias deprecated para rastrear fase, leituras, buscas, edits e sinais de convergencia

### 2. `run_engine.rs` migrado para `loop_state`

Os seguintes pontos foram atualizados para usar `context_loop_state`:

- budget exceed summary
- context loop injection
- phase transitions do loop
- compaction context
- retorno em caminhos de erro/sucesso
- gate de `done`
- agregacao de edits vindos de `subagent`, `subagent_parallel` e `skill`
- doom-loop abort
- sensores de read/search/edit no tracking do loop

### 3. `agent_loop.rs` sem dependencia deprecated no helper local

O helper `phase_nudge(...)` e seus testes agora usam:

- `ContextLoopState`
- `LoopPhase`

Isso remove dependencia deprecated tambem desse caminho auxiliar do loop.

### 4. Compatibilidade isolada em `state.rs`

`state.rs` continua existindo apenas como alias de compatibilidade:

- `Phase -> LoopPhase`
- `AgentState -> ContextLoopState`

Os testes desse modulo foram mantidos e anotados para nao poluir a validacao com warnings esperados da propria camada deprecated.

### 5. Reexports do crate continuam limpos

`theo-agent-runtime::lib` nao reexporta mais:

- `AgentEvent`
- `EventSink`
- `AgentState`
- `Phase`

O acesso legado passa a ser explicito por modulo de compatibilidade.

## Validacao executada

Comandos:

```bash
cargo fmt --all
cargo test -q -p theo-agent-runtime
cargo test -q -p theo
```

Resultados:

- `theo-agent-runtime`: 335 testes passando
- `theo`: 23 testes passando

Observacoes:

- ainda existem warnings fora deste escopo em crates como `theo-engine-retrieval` e `theo-application`
- `theo-application` ainda usa `EventSink` deprecated de forma explicita, o que ja estava mapeado como gap separado

## DoD validado

### DoD 1

O caminho ativo de `AgentRunEngine` nao depende mais de `AgentState` / `Phase`.

Status:

- validado

Evidencia:

- `run_engine.rs` agora usa `ContextLoopState`
- nao restam referencias a `crate::state` no caminho ativo do engine

### DoD 2

O helper de loop em `agent_loop.rs` nao depende mais de tipos deprecated.

Status:

- validado

Evidencia:

- `phase_nudge(...)` usa `ContextLoopState` e `LoopPhase`

### DoD 3

`state.rs` ficou restrito a compatibilidade explicita.

Status:

- validado

Evidencia:

- modulo reduzido a aliases deprecated
- testes de compatibilidade mantidos

### DoD 4

O comportamento do runtime central permaneceu estavel apos a migracao.

Status:

- validado

Evidencia:

- `cargo test -q -p theo-agent-runtime`
- `cargo test -q -p theo`

## O que ainda falta neste eixo

Esta entrega nao conclui todo o cleanup do runtime. Ainda restam:

- reduzir o uso de `events::EventSink` deprecated nos consumidores externos
- decidir o destino final de `events.rs` como superficie de compatibilidade
- avaliar se `state.rs` deve permanecer publico ou migrar para compatibilidade interna/documentada

## Avaliacao da entrega

Entrega aprovada.

O loop central agora usa um modelo de estado nao deprecated no runtime ativo, enquanto a compatibilidade antiga ficou mais claramente isolada e com fronteiras menores.
