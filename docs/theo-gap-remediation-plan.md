# Theo Gap Remediation Plan

Este plano transforma os gaps identificados no cross-validation em trabalho executavel.

Objetivo:

- fechar os gaps entre documentacao e implementacao
- elevar a coesao do runtime principal
- remover stubs da superficie publica
- consolidar sessao e persistencia
- reduzir warnings no caminho critico

Escopo:

- todos os gaps apontados em `docs/theo-vs-claw-cross-validation.md`

Nao e um roadmap generico. E um plano de entrega com:

- epicos
- microtasks
- criterios de aceite
- Definition of Done por bloco

## Visao geral

### Gaps a resolver

1. Alinhar catalogo documentado com o registry real
2. Remover ou encapsular melhor o legado deprecated do runtime
3. Fechar ou esconder stubs operacionais
4. Endurecer contratos de persistencia
5. Reduzir warnings do workspace central

### Ordem recomendada

1. Documentacao e inventario real
2. Stubs e superficie publica
3. Runtime legado/deprecated
4. Persistencia e sessao
5. Warnings e hardening final

### Principio de execucao

Nenhuma feature deve continuar em um estado onde:

- README sugere "pronto"
- registry nao expoe
- tool existe mas devolve placeholder
- comentario promete uma garantia que o codigo nao cumpre

## Epic 1: Single Source of Truth da superficie do produto

### Objetivo

Fazer com que docs, CLI help, tooling README e runtime descrevam a mesma superficie real.

### Microtasks

#### 1.1 Inventariar a superficie real do Theo

Tarefas:

- listar tools do `create_default_registry()`
- listar meta-tools adicionadas em `tool_bridge`
- listar tools existentes no codigo que nao entram no registry
- listar tools que entram no registry mas estao experimentais/stub
- listar skills bundled e como elas entram no runtime

Criterios de aceite:

- existe um documento ou tabela gerada manualmente com:
  - `tool_id`
  - origem (`registry`, `meta-tool`, `experimental`, `hidden`)
  - status (`implemented`, `partial`, `stub`)
  - onde entra no runtime

DoD:

- tabela revisada e salva em `docs/`
- nenhum item ambigamente classificado

#### 1.2 Corrigir README principal

Tarefas:

- revisar a secao “21+ Built-in Tools”
- separar claramente:
  - built-in registry tools
  - meta-tools
  - experimental tools
- ajustar claims sobre subagents e intelligence tools

Criterios de aceite:

- README nao chama de “built-in tools” algo que so existe como meta-tool
- README nao trata stub como funcionalidade pronta
- exemplos de uso so usam superficies que existem de verdade

DoD:

- diff do README aprovado
- tabela de tools alinhada ao inventario da microtask 1.1

#### 1.3 Corrigir `crates/theo-tooling/README.md`

Tarefas:

- marcar explicitamente o status de cada tool:
  - implemented
  - experimental
  - stub
  - not in default registry
- ajustar coluna de testes quando houver teste apenas de descricao ou schema, mas nao de execucao real

Criterios de aceite:

- nenhuma tool stub aparece como se estivesse operacional sem nota de status
- o README deixa claro o que entra no default registry

DoD:

- documentacao refletindo o codigo atual

#### 1.4 Expor um inventario em codigo

Tarefas:

- criar uma estrutura central de manifest para tools/meta-tools
- opcionalmente adicionar comando CLI de debug, ex.: `theo tools --manifest`
- fazer docs consumirem esse manifest manualmente ou via geracao

Criterios de aceite:

- ha uma origem unica para o inventario real
- diffs futuros nao exigem atualizar 3 lugares desconectados sem suporte

DoD:

- manifest acessivel por codigo
- cobertura de teste minima garantindo contagem e classificacao basica

### DoD do Epic 1

- README principal alinhado ao runtime real
- tooling README alinhado ao registry real
- existe inventario unificado de superficie
- nenhum stub e vendido como feature pronta

## Epic 2: Fechar, promover ou esconder stubs operacionais

### Objetivo

Eliminar o estado intermediario em que a tool aparece na narrativa principal, mas nao entrega funcionalidade real.

### Subdecisao obrigatoria

Para cada tool abaixo, escolher uma das tres estrategias:

- implementar de verdade
- manter experimental e esconder da narrativa principal
- remover do caminho principal ate ficar pronta

### Microtasks

#### 2.1 `websearch`

Tarefas:

- definir provider/API de busca
- implementar execucao real
- tratar erros, timeouts, truncation, rate limit
- definir contrato de output
- adicionar testes de unidade e integracao com doubles/mocks

Criterios de aceite:

- `websearch` executa busca real ou mockada em teste
- output e deterministicamente parseavel
- falhas de rede nao quebram o runtime

DoD:

- implementado com testes
- README atualizado

Alternativa valida:

- se nao for implementar agora, remover da narrativa principal e marcar como `stub/not shipped`

#### 2.2 `codesearch`

Tarefas:

- decidir se `codesearch` usa retrieval interno ou API externa
- evitar sobreposicao semantica confusa com `codebase_context`
- implementar execucao real
- testar query simples, query sem resultado e truncation

Criterios de aceite:

- tool entrega resultado real e util
- diferenca entre `codesearch` e `codebase_context` fica explicita

DoD:

- implementado ou removido da superficie principal

#### 2.3 `lsp`

Tarefas:

- definir escopo minimo:
  - `definition`
  - `references`
  - `hover`
- decidir bootstrap de servidor e contrato de erro
- implementar operacoes basicas
- ou mover explicitamente para experimental interno fora da docs principal

Criterios de aceite:

- se exposta ao usuario, ao menos uma operacao funciona end-to-end
- mensagens de erro distinguem:
  - sem servidor
  - linguagem sem suporte
  - request invalida

DoD:

- tool funcional com testes, ou escondida da superficie principal

#### 2.4 `task` / spawn real de subagente

Tarefas:

- substituir placeholder por integracao real com runtime de subagentes
- definir:
  - spawn
  - retorno
  - timeout
  - isolamento
  - agregacao de resultados
- garantir que a tool nao apenas ecoa input

Criterios de aceite:

- invocar `task` cria execucao real de subagente ou falha explicitamente com capability disabled
- output retorna identificador, estado e resumo
- comportamento e consistente com docs de subagent

DoD:

- tool conectada ao mecanismo real de subagente
- testes cobrindo spawn, erro e completion

#### 2.5 Dynamic tool loading

Tarefas:

- implementar `load_custom_tools_from_dir()`
- definir formato de discovery
- validar schema e conflito de nomes
- testar carregamento feliz e cenarios de erro

Criterios de aceite:

- tools custom entram no registry
- conflitos de nome falham com erro claro
- schema invalido nao quebra o processo inteiro

DoD:

- funcionalidade implementada com testes

#### 2.6 Revisao da narrativa de features

Tarefas:

- apos 2.1-2.5, revisar README, CLI help e docs internas
- garantir que so o que esta pronto entra nos exemplos principais

Criterios de aceite:

- exemplos publicos executam de verdade

DoD:

- documentacao consistente com o estado final das tools

### DoD do Epic 2

- nenhuma tool principal e placeholder silencioso
- toda tool publica esta:
  - implementada
  - ou marcada claramente como experimental
  - ou retirada da narrativa principal

## Epic 3: Concluir a migracao do runtime legada

### Objetivo

Remover a dependencia conceitual do sistema deprecated de eventos/estado no caminho principal do runtime.

### Microtasks

#### 3.1 Mapear legado ainda usado

Tarefas:

- localizar todos os `#[allow(deprecated)]`
- localizar todos os usos de `AgentEvent`, `EventSink`, `AgentState`, `Phase`
- classificar:
  - compatibilidade publica
  - dependencia interna evitavel
  - testes antigos

Criterios de aceite:

- lista completa dos pontos de legado

DoD:

- documento ou issue checklist por arquivo

#### 3.2 Definir target architecture do runtime

Tarefas:

- declarar arquitetura final:
  - `DomainEvent`
  - `EventBus`
  - listeners
  - runtime state
- definir o que substitui `AgentState/Phase`

Criterios de aceite:

- existe uma nota de arquitetura curta e objetiva

DoD:

- aprovada e referenciada nas tasks seguintes

#### 3.3 Migrar `agent_loop.rs`

Tarefas:

- reduzir `EventSinkBridge`
- mover o caminho principal para `EventBus/EventListener`
- manter adaptador legado apenas na borda, se necessario

Criterios de aceite:

- caminho normal de execucao nao depende de tipos deprecated
- bridge legado, se existir, fica isolado em adaptador claro

DoD:

- `agent_loop.rs` sem dependencia central em eventos deprecated

#### 3.4 Migrar CLI e desktop da API legada

Tarefas:

- ajustar consumidores que dependem de `EventSink`
- consumir `DomainEvent`/listener padrao
- remover reexports deprecated de `lib.rs` se possivel

Criterios de aceite:

- binaries continuam funcionando
- compatibilidade e explicita e isolada

DoD:

- superficie publica principal sem deprecated

#### 3.5 Revisar testes

Tarefas:

- atualizar testes que ainda exercitam o modelo velho sem necessidade
- manter apenas testes de compatibilidade deliberados

Criterios de aceite:

- testes distinguem “legacy compatibility” de “main path”

DoD:

- suite verde
- legado com escopo intencional

### DoD do Epic 3

- `AgentRunEngine` e `AgentLoop` usam arquitetura final sem dependencia estrutural do legado
- deprecated fica apenas em adaptadores ou e removido
- `lib.rs` nao expõe modulos mortos como se fossem parte viva do design

## Epic 4: Consolidar sessao, bootstrap e persistencia

### Objetivo

Transformar sessao/persistencia em um subsistema coeso, com contratos verdadeiros e menos fragmentacao conceitual.

### Microtasks

#### 4.1 Modelar os tipos de persistencia existentes

Tarefas:

- levantar todos os artefatos persistidos:
  - snapshots
  - progress
  - metrics
  - episodes
  - wiki outputs
- descrever quem grava, quando grava e para que serve

Criterios de aceite:

- matriz clara de persistencia

DoD:

- documento salvo em `docs/`

#### 4.2 Definir modelo conceitual de sessao

Tarefas:

- responder claramente:
  - o que e uma `session` no Theo?
  - o que e um `run`?
  - o que e `progress`?
  - o que e snapshot?
- alinhar nomes de API e docs

Criterios de aceite:

- nao ha mais confusao entre sessao conversacional, run operacional e progresso acumulado

DoD:

- nomenclatura acordada e aplicada nas docs e comentarios

#### 4.3 Corrigir contratos/documentacao incorretos

Tarefas:

- ajustar comentario de advisory locking em `session_bootstrap`
- revisar comentarios que prometem garantias nao implementadas

Criterios de aceite:

- nenhum comentario de contrato mente sobre locking, atomicidade ou consistencia

DoD:

- comentarios auditados nos modulos de persistencia

#### 4.4 Endurecer `session_bootstrap`

Tarefas:

- decidir se havera lock de verdade ou comentario corrigido
- adicionar protecao contra corrupcao parcial
- adicionar estrategia de recovery para JSON invalido

Criterios de aceite:

- comportamento em arquivo corrompido e previsivel
- progresso nao e perdido silenciosamente sem rastreio

DoD:

- testes de corrupcao, recovery e roundtrip

#### 4.5 Revisar `SnapshotStore`

Tarefas:

- verificar se checksum cobre tudo que deve cobrir
- definir estrategia de evolucao/versionamento de snapshot
- validar concorrencia basica

Criterios de aceite:

- snapshots possuem contrato explicito de consistencia
- mudancas futuras nao quebram restore sem mecanismo de versao

DoD:

- campos de versao ou estrategia documentada
- testes adicionais se necessario

#### 4.6 Padronizar locais e convencoes em `.theo/`

Tarefas:

- definir layout oficial:
  - `progress`
  - `metrics`
  - `episodes`
  - `snapshots`
  - `skills`
- documentar quem e dono de cada diretorio

Criterios de aceite:

- layout em disco documentado e consistente

DoD:

- docs atualizadas
- codigo sem caminhos “magicos” espalhados sem explicacao

### DoD do Epic 4

- persistencia com modelo conceitual claro
- comentarios alinhados ao codigo
- recovery e consistencia testados
- layout `.theo/` documentado e padronizado

## Epic 5: Reduzir warnings no caminho critico

### Objetivo

Diminuir ruido e elevar disciplina de manutencao no workspace central.

### Estrategia

Nao tentar zerar warnings do monorepo todo de uma vez. Priorizar caminho critico:

- `theo-agent-runtime`
- `theo-tooling`
- `theo-application`
- `theo-engine-retrieval`
- `apps/theo-cli`

### Microtasks

#### 5.1 Congelar baseline atual

Tarefas:

- rodar testes/build e registrar warnings por crate
- agrupar warnings por tipo:
  - unused imports
  - unused vars
  - dead code
  - deprecated
  - unused mut

Criterios de aceite:

- baseline registrada

DoD:

- arquivo de baseline salvo em `docs/` ou issue tracking

#### 5.2 Limpar warnings de `theo-agent-runtime`

Tarefas:

- remover imports/vars mortos
- isolar ou remover deprecated onde nao necessario
- revisar modulos `doc(hidden)` mortos

Criterios de aceite:

- `cargo test -q -p theo-agent-runtime` com warnings significativamente reduzidos

DoD:

- nenhuma warning trivial restante no crate principal

#### 5.3 Limpar warnings de `theo-tooling`

Tarefas:

- remover warnings triviais
- revisar exports e testes que geram ruido

Criterios de aceite:

- crate passa com ruido baixo

DoD:

- warnings remanescentes apenas quando justificadas

#### 5.4 Limpar warnings de retrieval/application

Tarefas:

- atacar unused imports e vars
- separar warnings aceitaveis de backlog real

Criterios de aceite:

- caminho principal de retrieval muito menos ruidoso

DoD:

- warnings significativamente reduzidos

#### 5.5 Introduzir guardrail de CI

Tarefas:

- adicionar etapa de CI para impedir regressao de warnings em crates chave
- opcao:
  - `-D warnings` apenas em crates estabilizados
  - ou script de budget de warnings

Criterios de aceite:

- novas PRs nao reintroduzem ruido sem visibilidade

DoD:

- CI com regra objetiva

### DoD do Epic 5

- warnings reduzidos nos crates centrais
- baseline conhecida
- regressao controlada por CI

## Epic 6: Fechamento de produto e consistencia final

### Objetivo

Garantir que o Theo, apos os epics anteriores, tenha uma superficie publica coerente e defensavel.

### Microtasks

#### 6.1 Auditoria final de claims

Tarefas:

- revisar README, docs e help do CLI
- verificar toda claim sobre:
  - tools
  - subagents
  - skills
  - sandbox
  - session persistence
  - context compaction

Criterios de aceite:

- cada claim aponta para funcionalidade real, ou vem marcada com status

DoD:

- auditoria concluida sem inconsistencias abertas

#### 6.2 Auditoria final de experiencia do usuario

Tarefas:

- validar fluxos principais:
  - REPL
  - single-shot
  - pilot
  - tools centrais
  - bootstrap de projeto
- verificar mensagens de erro e status

Criterios de aceite:

- falhas comuns sao compreensiveis
- comportamento principal bate com a narrativa publica

DoD:

- checklist de UX tecnica concluido

#### 6.3 Auditoria final de coesao arquitetural

Tarefas:

- verificar se runtime, tooling e docs convergiram
- revisar se ainda ha modulos mortos expostos sem necessidade

Criterios de aceite:

- camada principal do agente pode ser explicada sem “mas isso ainda usa o modelo velho”

DoD:

- nota final de arquitetura atualizada

### DoD do Epic 6

- Theo pode ser descrito de forma honesta, clara e curta
- a superficie publica e coerente com o que o codigo faz
- o nucleo do agente esta tecnicamente defendavel

## Matriz de prioridade

### Prioridade P0

- Epic 1
- Epic 2 para `task`, `websearch`, `codesearch`, `lsp`
- Epic 3.1, 3.2, 3.3
- Epic 4.3

### Prioridade P1

- restante do Epic 3
- Epic 4 completo
- Epic 5.1, 5.2, 5.3

### Prioridade P2

- Epic 5.4, 5.5
- Epic 6

## Sequenciamento sugerido em sprints

### Sprint 1

- Epic 1 completo
- Epic 2 decisao por tool
- Epic 4.3

### Sprint 2

- Epic 2 implementacoes ou hide/removal
- Epic 3.1 a 3.3

### Sprint 3

- Epic 3.4 a 3.5
- Epic 4.1 a 4.4

### Sprint 4

- Epic 4.5 a 4.6
- Epic 5.1 a 5.3

### Sprint 5

- Epic 5.4 a 5.5
- Epic 6

## Checklist final de saida

O plano so pode ser considerado concluido quando todas as condicoes abaixo forem verdadeiras:

- docs principais refletem a superficie real
- nenhuma tool principal e stub silencioso
- runtime principal nao depende estruturalmente do sistema deprecated
- persistencia possui contratos verdadeiros e testados
- warnings do caminho critico foram reduzidos e controlados
- README, CLI e runtime contam a mesma historia

## Definicao global de Done

O programa de remediacao inteiro esta Done quando:

1. Um novo engenheiro consegue ler o README e encontrar no codigo exatamente a mesma superficie prometida.
2. O caminho principal do agente nao depende mais de compatibilidade legada para operar.
3. Toda tool publica principal executa funcionalidade real ou esta explicitamente marcada como experimental.
4. Sessao, snapshot e progresso possuem papeis claros, documentados e testados.
5. O workspace central compila/testa com baixo ruido de warnings.
6. O Theo pode ser comparado ao Claw sem precisar adicionar ressalvas sobre incoerencia doc-codigo ou stubs ocultos.
