# Theo vs Claw — Cross Validation por Subsistema

Este documento compara `theo-code` com `claw-code` com foco em qualidade de engenharia, coerencia arquitetural e prontidao real da superficie exposta.

Nao e uma comparacao de marketing. E uma leitura tecnica baseada no estado atual dos dois repositorios.

## Escopo avaliado

### Repositorios

- Theo: `/home/paulo/Projetos/usetheo/theo-code`
- Claw: `/home/paulo/Projetos/usetheo/claw-code`

### Fontes principais

- Theo:
  - `README.md`
  - `Cargo.toml`
  - `crates/theo-agent-runtime`
  - `crates/theo-tooling`
  - `crates/theo-engine-retrieval`
  - `apps/theo-cli`
- Claw:
  - `README.md`
  - `USAGE.md`
  - `PARITY.md`
  - `rust/crates/runtime`
  - `rust/crates/tools`
  - `rust/crates/rusty-claude-cli`

### Validacao executada

No Theo, foram executados:

- `cargo test -q -p theo-agent-runtime`
- `cargo test -q -p theo-tooling`
- `cargo test -q -p theo-engine-retrieval`

Todos passaram, com warnings.

## Resumo executivo

Veredito curto:

- Theo e mais ambicioso e mais modular que Claw.
- Theo tem excelente base arquitetural e uma suite de testes forte.
- Claw ainda apresenta maior coerencia entre documentacao, runtime principal e superficie real exposta.
- Theo tem mais sinais de transicao interna e mais diferenca entre o que a documentacao sugere e o que algumas tools realmente fazem.

Se a pergunta for "Theo tem qualidade de engenharia seria?":

- Sim.

Se a pergunta for "Theo hoje tem a mesma qualidade operacional/coesa do Claw no nucleo do agente?":

- Ainda nao completamente.

## Tabela comparativa

| Subsistema | Theo | Claw | Vantagem atual |
|---|---|---|---|
| Arquitetura geral | Monorepo amplo, bounded contexts claros, dominio separado | Workspace Rust focado no agente CLI | Theo |
| Runtime central do agente | Rico, formalizado, mas ainda em migracao parcial | Mais enxuto e mais coeso | Claw |
| Sessao e persistencia | Snapshot store e bootstrap de progresso, mas menos uniforme | Modelo de sessao mais claro e incremental em JSONL | Claw |
| Tools reais expostas | Catalogo grande, parte stub/experimental | Superficie mais consistente com implementacao | Claw |
| Sandbox | Boa estrutura e preocupacao real de isolamento | Boa implementacao tambem | Empate tecnico |
| Code intelligence | Muito superior: graph, parser, retrieval, wiki | Menos profundo nesse eixo | Theo |
| Subagentes | Conceito forte, skill/meta-tooling boa, parte ainda placeholder | Existe superficie de agent/subagent no runtime | Theo em visao, Claw em coesao |
| Documentacao vs codigo | Alguma sobrepromessa | Mais honesto e mais alinhado via `PARITY.md` | Claw |
| Suite de testes | Muito forte em volume e abrangencia | Boa, mas menor no conjunto | Theo |
| Maturidade do produto principal | Bom, mas heterogeneo | Mais consolidado no agente CLI | Claw |

## 1. Arquitetura geral

### Theo

Pontos fortes:

- separacao clara entre `theo-domain`, `theo-application`, `theo-agent-runtime`, `theo-tooling`, `theo-infra-*`, `engine-*`
- apps (`theo-cli`, `theo-desktop`, `theo-ui`) sobre uma base compartilhada
- README com arquitetura coerente com o workspace real
- `Cargo.toml` confirma um workspace amplo e organizado

Sinal de qualidade:

- a estrutura sugere intencao arquitetural real, nao apenas crescimento organico sem fronteiras

### Claw

Pontos fortes:

- foco maior no CLI agent
- runtime/tools/api/commands bem separados
- menos superficie total para coordenar

Leitura comparativa:

- Theo vence em design arquitetural e separacao por contexto
- Claw vence em foco operacional e menor dispersao

## 2. Runtime central do agente

### Theo

Pontos fortes:

- `AgentRunEngine` formaliza `RunState`, eventos, budget, convergence, metrics, persistence e graph context
- `agent_loop.rs` virou facade fina sobre o engine
- ha preocupacao com working set, context metrics e failure tracking

Pontos de atencao:

- o runtime ainda carrega camada legada de compatibilidade
- `theo-agent-runtime/src/lib.rs` exporta modulos `#[doc(hidden)]` com comentario de dead code
- o sistema antigo de eventos ainda esta exportado e marcado como deprecated
- varios `#[allow(deprecated)]` aparecem no caminho critico

### Claw

Pontos fortes:

- `ConversationRuntime` e mais direto
- loop de turno, permissao, hooks, tools e sessao estao muito proximos e compreensiveis
- menos camada de compatibilidade aparente no nucleo

Leitura comparativa:

- Theo tem um runtime mais ambicioso
- Claw tem um runtime mais coeso e mais limpo hoje

Vantagem atual:

- Claw, no quesito coerencia do nucleo de execucao

## 3. Sessao e persistencia

### Theo

Pontos fortes:

- `SnapshotStore` com checksum
- `RunSnapshot` e persistencia formal de execucao
- `session_bootstrap` traz ideia util de continuidade entre sessoes

Pontos de atencao:

- persistencia esta dividida entre snapshot store, progresso de sessao e outros artefatos `.theo`
- `save_progress()` diz usar advisory locking, mas a implementacao faz apenas write atomico com rename
- o conceito de sessao parece mais fragmentado que no Claw

### Claw

Pontos fortes:

- `Session` e o centro da verdade conversacional
- formato JSONL incremental
- `SessionStore` namespaced por workspace
- `latest`, `resume`, `fork` e persistencia estao bem encaixados

Vantagem atual:

- Claw

Motivo:

- a modelagem de sessao e mais simples, mais uniforme e mais fiel ao runtime real

## 4. Tools e superficie operacional

### Theo

Pontos fortes:

- `ToolRegistry` tipado
- schemas validados no registro
- boa estrutura de sandbox, git, http, memory, edit, patch
- `tool_bridge` adiciona meta-tools relevantes como `done`, `subagent`, `skill`, `batch`

Pontos de atencao:

- parte do catalogo documentado nao entra no registry padrao
- `websearch` nao implementado
- `codesearch` nao implementado
- `lsp` nao implementado
- `task` nao spawna subagente de verdade; hoje devolve um `ToolOutput` placeholder
- `load_custom_tools_from_dir()` ainda esta como TODO

### Claw

Pontos fortes:

- catalogo mais coeso com o que esta documentado no runtime
- `PARITY.md` explicita com mais honestidade o que e profundo e o que ainda e parcial
- tools centrais do agente estao mais alinhadas com a experiencia prometida

Vantagem atual:

- Claw

Motivo:

- menos distancia entre contrato, docs e implementacao

## 5. Sandbox e seguranca operacional

### Theo

Pontos fortes:

- sandbox com estrutura dedicada
- configuracao de env sanitization, denied paths, network, bwrap/landlock
- `BashTool` tenta ativar sandbox de verdade e faz fallback explicito

### Claw

Pontos fortes:

- `sandbox.rs`, `permission_enforcer.rs`, `permissions.rs`
- politicas de permissao integradas ao loop

Leitura comparativa:

- os dois parecem levar esse eixo a serio
- Theo parece ter mais modularizacao de infra de sandbox
- Claw parece integrar melhor permissao + runtime loop

Vantagem atual:

- empate tecnico, com enfoques diferentes

## 6. Code intelligence, retrieval e entendimento de codebase

### Theo

Pontos fortes:

- este e o maior diferencial do sistema
- parser multi-linguagem
- graph engine
- retrieval engine
- wiki e pipeline de contexto
- docs e benchmarks para essa area

Validacao adicional:

- `theo-engine-retrieval` passou nos testes
- ha bastante suite de avaliacao/benchmark

### Claw

Pontos fortes:

- bom harness de agente e tools
- menos foco em graph/retrieval profundo

Vantagem atual:

- Theo, com folga

## 7. Subagentes, skills e orquestracao

### Theo

Pontos fortes:

- a visao de skills e subagentes e forte
- `tool_bridge` adiciona meta-tools especificos para isso
- `skill` e `subagent_parallel` demonstram preocupacao com orquestracao moderna
- existem skills bundled

Pontos de atencao:

- parte da execucao ainda e intermediada por meta-tools fora do registry
- `task` como tool concreta ainda nao implementa spawn real
- parte da narrativa de subagentes esta a frente da implementacao mais direta

### Claw

Pontos fortes:

- subagents/agentes existem como superficie do runtime
- menos ambicao total, mas mais proximidade com a execucao real documentada

Leitura comparativa:

- Theo ganha em visao e desenho de produto
- Claw ganha em coesao imediata

## 8. Documentacao vs implementacao

### Theo

Pontos positivos:

- README forte
- documentacao de arquitetura boa
- `theo-tooling/README` tenta mapear bem o pacote

Pontos de atencao:

- o README passa a impressao de uma superficie mais pronta do que o codigo entrega em algumas tools
- `theo-tooling/README` lista tools que nao estao realmente prontas ou nem registradas por padrao
- ha comentarios de contrato mais fortes que a implementacao, como o caso de advisory locking

### Claw

Pontos positivos:

- `PARITY.md` e um mecanismo excelente de honestidade operacional
- README e USAGE estao mais alinhados ao estado real do workspace Rust

Vantagem atual:

- Claw

## 9. Testes e confiabilidade

### Theo

Resultado observado:

- `theo-agent-runtime`: 333 testes passando
- `theo-tooling`: 237 testes passando
- `theo-engine-retrieval`: 195 testes passando, com alguns ignorados

Pontos fortes:

- volume de testes muito bom
- cobertura em runtime, tooling e retrieval
- qualidade de engenharia claramente acima da media

Pontos de atencao:

- existe um volume consideravel de warnings
- o retrieval mostra varios warnings e alguns testes ignorados

### Claw

Pontos fortes:

- bom conjunto de testes, inclusive mock parity harness
- foco em validar comportamento do agente

Leitura comparativa:

- Theo vence em volume e diversidade de validacao
- Claw vence em foco de paridade do runtime central

## 10. Principais gaps do Theo para atingir o mesmo nivel de coesao do Claw

### Gap 1: alinhar catalogo documentado com o registry real

Hoje o Theo mistura:

- tools reais do registry
- meta-tools do `tool_bridge`
- tools documentadas mas ainda nao implementadas

Acao recomendada:

- documentar explicitamente tres categorias:
  - implemented
  - meta-tools
  - experimental/stub

### Gap 2: remover ou encapsular melhor o legado deprecated do runtime

Acao recomendada:

- concluir a migracao do `AgentLoop` para usar apenas `DomainEvent`/`EventBus`
- parar de reexportar tipos deprecated como parte da superficie principal

### Gap 3: fechar ou esconder stubs operacionais

Prioridade alta:

- `websearch`
- `codesearch`
- `lsp`
- `task` com spawn real
- dynamic tool loading

Se nao estiver pronto, o certo e:

- tirar da narrativa principal
- ou marcar visivelmente como experimental/incomplete

### Gap 4: endurecer contratos de persistencia

Acao recomendada:

- corrigir comentario de locking em `session_bootstrap`
- padronizar melhor a historia de sessao, snapshot, progresso e artefatos laterais

### Gap 5: reduzir warnings do workspace central

Os testes passam, mas o ruido de warnings ainda e alto em alguns crates.

Acao recomendada:

- tratar warnings do runtime e retrieval como backlog de qualidade real

## 11. Onde Theo ja supera Claw

Theo ja supera Claw em:

- profundidade de entendimento de codebase
- desenho arquitetural de plataforma
- stack de graph/retrieval/wiki
- ambicao e extensibilidade do produto
- volume de testes em subsistemas centrais

## 12. Onde Claw ainda supera Theo

Claw ainda supera Theo em:

- coerencia do runtime principal
- fidelidade entre docs e implementacao
- honestidade de status via documento de paridade
- prontidao percebida da superficie principal do agente
- uniformidade da modelagem de sessao

## 13. Veredito final

Theo tem qualidade alta de engenharia.

Nao faz sentido classificar o projeto como inferior ou improvisado. A base e seria, modular e bem testada.

Mas hoje a qualidade do Theo e desigual por camada:

- muito forte em arquitetura, retrieval e teste
- mais irregular na superficie operacional do agente e no alinhamento doc-codigo

Conclusao final:

- Theo tem qualidade comparavel ao Claw em maturidade de engenharia geral
- Theo ainda nao tem a mesma qualidade de coesao operacional do Claw no nucleo do agente
- Theo tem potencial para ultrapassar o Claw com facilidade se fechar os stubs, reduzir legado exposto e alinhar a documentacao ao estado real

## 14. Proximo passo recomendado

Se a meta for elevar Theo ao mesmo nivel ou acima do Claw no runtime principal, a ordem de ataque deveria ser:

1. alinhar docs com o estado real
2. implementar ou esconder tools stub
3. concluir a migracao do runtime legada
4. consolidar a historia de sessao/persistencia
5. reduzir warnings do caminho critico
