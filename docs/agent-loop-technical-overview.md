# Visão Técnica Do Ciclo Do Agente

Este documento descreve, em nível técnico e operacional, como funciona o ciclo do agente no sistema: planejamento, decomposição de trabalho, execução, continuidade, avaliação e correção. O objetivo aqui é explicar o comportamento do sistema como arquitetura de runtime, sem depender de referências de implementação.

## 1. Visão Geral

O agente funciona como um orquestrador de trabalho iterativo. Ele recebe um objetivo, constrói um entendimento do contexto, decide uma estratégia, executa ações através de ferramentas, observa os resultados, reavalia o estado e repete esse ciclo até atingir um resultado satisfatório ou encontrar um bloqueio.

Esse modelo não é um único passo de inferência. Ele é um loop de execução com memória operacional, estado persistido, eventos, artefatos e mecanismos explícitos de retomada.

O ciclo completo pode ser dividido em seis grandes etapas:

1. Planejamento inicial e escolha de estratégia.
2. Quebra do trabalho em steps ou tasks.
3. Execução de ações por ferramentas e subagentes.
4. Continuidade de execução e retomada de contexto.
5. Avaliação do resultado produzido.
6. Correção iterativa até convergência.

## 2. Princípios Do Modelo

O agente parte de alguns princípios estruturais:

- O trabalho é incremental.
- O estado precisa ser observável.
- A execução precisa ser resumível.
- A decisão não depende apenas do prompt atual, mas do histórico operacional.
- Ferramentas são parte do raciocínio, não um efeito lateral externo.
- Planejamento, execução e avaliação são fases explícitas, mesmo quando ocorrem rapidamente.

Em termos práticos, isso significa que o agente não “responde apenas com texto”. Ele opera sobre um estado de sessão e produz ações, eventos, resultados e novos estados.

## 3. Planejamento Via Agents Como Plan E Explore

### 3.1 Objetivo Da Fase De Planejamento

A etapa de planejamento existe para transformar um objetivo amplo em uma estratégia operacional viável.

Ela serve para:

- entender o contexto disponível;
- decidir escopo;
- separar descoberta de execução;
- evitar ações prematuras sobre código ou sistema;
- reduzir o custo de iteração errada.

### 3.2 Agent De Exploração

O papel do agente de exploração é maximizar entendimento com risco mínimo.

Esse tipo de agente é voltado para:

- leitura de contexto;
- descoberta de arquivos, entidades, fluxos e dependências;
- levantamento de restrições;
- identificação de superfícies relevantes;
- mapeamento preliminar do problema.

Ele é útil quando ainda não está claro:

- onde o problema vive;
- quais arquivos importam;
- quais ferramentas serão necessárias;
- se a hipótese inicial do usuário está correta.

Em um ciclo maduro, o agente de exploração trabalha com viés de leitura e inspeção, e evita introduzir mudanças antes de existir contexto suficiente.

### 3.3 Agent De Planejamento

O papel do agente de planejamento é transformar entendimento em estrutura de execução.

Esse tipo de agente é voltado para:

- definir abordagem;
- decompor o trabalho em fases;
- escolher sequência de execução;
- apontar dependências críticas;
- identificar riscos e validações necessárias.

O resultado esperado de um planejamento bom não é apenas “uma lista de ideias”, mas uma estrutura de trabalho que o runtime consegue executar ou acompanhar.

### 3.4 Saída Do Planejamento

Ao fim da fase de planejamento, o sistema idealmente passa a conhecer:

- objetivo operacional do ciclo;
- contexto suficiente para agir;
- estratégia principal;
- steps intermediários;
- critérios de sucesso;
- possíveis pontos de revisão.

Essa saída alimenta a próxima fase: a decomposição em tasks e steps.

## 4. Quebra Do Trabalho Em Steps E Tasks No Nível De Sessão E Runtime

### 4.1 Duas Camadas De Decomposição

O sistema opera com duas camadas complementares:

- camada de sessão;
- camada de runtime.

A sessão organiza o histórico de trabalho de forma persistida e navegável.

O runtime organiza a execução viva das ações e dos agentes ativos.

### 4.2 Steps De Sessão

Um step de sessão representa uma unidade rastreável de trabalho dentro de uma conversa ou execução contínua.

Ele pode representar, por exemplo:

- uma preparação de agente;
- uma execução de script;
- uma tool call isolada;
- uma retomada de agente;
- uma continuação de execução.

O valor do step de sessão é dar materialidade ao histórico. Em vez de existir apenas texto livre, o sistema passa a ter marcos explícitos do que foi feito.

### 4.3 Tasks De Runtime

Uma task de runtime representa uma unidade operacional associada a trabalho em andamento ou concluído.

Ela normalmente contém:

- identificador estável;
- descrição;
- tipo de agente;
- estado atual;
- artefatos associados;
- output persistido;
- relação com transcript e eventos.

Tasks são úteis porque permitem:

- inspeção posterior;
- acompanhamento de progresso;
- continuidade dirigida;
- separação entre múltiplos agentes e múltiplos trabalhos.

### 4.4 Por Que Separar Steps E Tasks

Essa separação existe porque:

- nem todo step vira uma task longa;
- nem toda task é apenas um step isolado;
- a sessão precisa contar a história do processo;
- o runtime precisa sustentar a execução operacional.

Em termos práticos:

- o step é um evento de workflow;
- a task é um objeto de trabalho durável.

## 5. Execução De Tools E Agent-Query

### 5.1 Tools Como Unidade De Ação

As tools são o mecanismo primário de ação.

Elas permitem ao agente:

- ler;
- buscar;
- editar;
- escrever;
- executar shell;
- consultar web;
- operar sobre sessão e tasks;
- interagir com outras superfícies de integração.

Sem tools, o agente só descreveria intenções. Com tools, ele altera o estado do sistema.

### 5.2 Tool Calls Como Contratos

Cada execução de ferramenta é um contrato explícito:

- entrada bem definida;
- resultado serializável;
- sucesso ou falha;
- output utilizável pela próxima etapa do loop.

Isso torna o agente mais confiável, porque ele não depende de inferir efeitos implícitos.

### 5.3 Agent-Query

`agent-query` é a forma de rodar um loop de modelo associado a um agente preparado, com contexto, tools resolvidas e identidade operacional.

Ele combina:

- prompt do objetivo;
- configuração do agente;
- ferramentas disponíveis;
- histórico parcial da execução;
- backend de modelo;
- loop de tool use e retorno.

Enquanto uma tool call simples executa uma ação única, `agent-query` executa um mini-ciclo agentivo controlado.

### 5.4 Estrutura Da Execução

Uma execução de `agent-query` normalmente segue esta ordem:

1. O agente é preparado.
2. O contexto inicial é montado.
3. O backend do modelo recebe o estado atual.
4. O modelo responde com texto final, tool calls ou ambos.
5. Tool calls são executadas.
6. Resultados dessas tools retornam ao loop.
7. O modelo produz novo turno ou encerra.
8. O sistema registra eventos, transcript e resultado final.

### 5.5 Por Que Isso É Diferente De Um Prompt Simples

Um prompt simples responde uma vez.

Um loop agentivo:

- age;
- observa;
- itera;
- produz estado persistente;
- pode ser retomado depois.

Isso é o que transforma o modelo em um agente operacional.

## 6. Continuidade De Execução Com SendMessage, Resume E Continue

### 6.1 Necessidade De Continuidade

O agente nem sempre termina em um único ciclo.

Ele pode:

- ser interrompido;
- precisar de novo contexto;
- receber uma nova instrução;
- depender de um output posterior;
- ser retomado após persistência.

Por isso, o sistema precisa de continuidade explícita.

### 6.2 SendMessage

`SendMessage` serve para enviar uma nova mensagem a um agente já existente.

Seu papel é:

- continuar um trabalho sem recriar o agente;
- anexar uma nova instrução;
- manter a identidade operacional;
- preservar a relação com transcript e task.

Ele é útil quando já existe um agente ativo ou persistido e se quer avançar o mesmo fluxo.

### 6.3 Resume

`resume` serve para reidratar um agente persistido e recolocá-lo em contexto operacional.

Essa operação normalmente usa artefatos persistidos, como:

- metadata;
- transcript;
- output anterior;
- identidade do agente.

O objetivo não é “recomeçar”, mas voltar ao mesmo fluxo com o contexto histórico relevante.

### 6.4 Continue

`continue` normalmente representa uma continuação mais estruturada de um agente já retomado, com novas tool calls, novo prompt ou novo fechamento.

Ele é a operação que permite:

- avançar o estado após um `resume`;
- adicionar mais ações;
- fechar o loop com novos resultados.

### 6.5 Benefício Arquitetural

Esses mecanismos evitam dois problemas:

- repetir trabalho já feito;
- perder contexto operacional entre execuções.

Eles transformam a execução do agente em algo durável e incremental.

## 7. Avaliação Via Transcript, Runtime Events, Task Inspection, Dashboard E Reports

### 7.1 A Avaliação Não Depende Só Do Texto Final

Em um sistema agentivo maduro, o resultado não é avaliado apenas pela mensagem final do modelo.

A avaliação acontece através de múltiplas superfícies:

- transcript;
- runtime events;
- inspeção de task;
- dashboard;
- relatórios resumidos.

Cada uma resolve um problema diferente.

### 7.2 Transcript

O transcript é o histórico serializado da execução.

Ele registra:

- prompts;
- turns do assistant;
- tool calls;
- tool results;
- eventos derivados;
- marcos de continuidade.

O transcript é importante porque permite reconstruir o raciocínio operacional do agente, não só sua conclusão.

### 7.3 Runtime Events

Runtime events são sinais compactos de transição e atividade.

Exemplos típicos:

- agente preparado;
- tool executada;
- resultado de tool;
- agente retomado;
- agente concluído;
- agente continuado.

O valor deles está em permitir observabilidade com granularidade menor do que um transcript completo.

### 7.4 Task Inspection

A inspeção de task foca no objeto de trabalho.

Ela permite responder perguntas como:

- qual agente executou isso;
- qual é o estado atual;
- onde está o output;
- que descrição esse trabalho tinha;
- que artefatos foram gerados.

É a visão mais útil quando o foco é “o que aconteceu com esse trabalho específico”.

### 7.5 Dashboard

O dashboard consolida a sessão em uma visão resumida.

Ele tende a agregar:

- estado da sessão;
- agentes envolvidos;
- tasks relevantes;
- eventos importantes;
- artefatos auxiliares.

É a superfície mais útil para orientação humana rápida.

### 7.6 Reports

Os relatórios servem para transformar o estado técnico em leitura operacional.

Eles sintetizam:

- quantidade de ações;
- sequência principal;
- status final;
- outputs relevantes;
- possíveis erros ou desvios.

Eles são particularmente úteis em fluxos longos ou quando há mais de um agente.

## 8. Correção Iterando Novas Tool Calls Ou Novos Turns

### 8.1 Correção Como Parte Nativa Do Loop

O sistema não assume que a primeira execução será correta.

A correção é parte estrutural do modelo de agente.

Depois de avaliar o estado, o agente pode:

- fazer uma nova leitura;
- executar uma nova busca;
- alterar arquivos;
- reexecutar validações;
- retomar um agente com nova instrução;
- rodar novo turno de modelo.

### 8.2 Tipos De Correção

Correções podem ocorrer por diferentes motivos:

- resultado incompleto;
- ferramenta falhou;
- hipótese inicial estava errada;
- output mostrou novo contexto;
- houve regressão;
- faltou validação;
- a tarefa estava mal decomposta.

### 8.3 Estratégias De Correção

O agente pode corrigir de formas diferentes:

- corrigir localmente com nova tool call;
- replanejar;
- abrir uma nova subtask;
- enviar nova mensagem ao mesmo agente;
- retomar de um estado persistido;
- trocar de estratégia mantendo o mesmo objetivo.

### 8.4 Convergência

O ciclo ideal de correção busca convergência.

Isso significa que a cada iteração o sistema deve reduzir uma ou mais destas incertezas:

- escopo;
- erro de execução;
- erro de entendimento;
- falta de evidência;
- falta de validação.

Se a iteração não reduz incerteza, o sistema tende a oscilar. Por isso, o bom uso do loop agentivo exige avaliação explícita antes de continuar.

## 9. Ciclo Operacional Completo

Em forma compacta, o ciclo completo do agente é:

1. Receber objetivo.
2. Explorar contexto.
3. Planejar abordagem.
4. Decompor em steps e tasks.
5. Preparar agente e contexto operacional.
6. Executar tools ou `agent-query`.
7. Persistir transcript, outputs e eventos.
8. Avaliar o estado produzido.
9. Decidir se encerra, continua, retoma ou corrige.
10. Iterar até convergência.

## 10. O Que Torna Esse Modelo Robusto

O agente ganha robustez porque combina:

- planejamento explícito;
- execução instrumentada;
- persistência;
- continuidade;
- observabilidade;
- correção iterativa.

Sem isso, o sistema seria apenas um modelo com tools.

Com isso, ele passa a operar como um runtime agentivo, em que a execução é:

- recuperável;
- auditável;
- iterativa;
- mensurável;
- composta por objetos e eventos claros.

## 11. Limites Naturais Do Modelo

Mesmo com essa estrutura, há limites naturais:

- planejamento pode estar errado;
- decomposição pode ser ruim;
- tools podem falhar;
- a avaliação pode ser insuficiente;
- o loop pode continuar sem realmente convergir;
- parte da qualidade do sistema depende das políticas de execução e isolamento;
- parte da eficácia depende da qualidade das surfaces de observabilidade.

Por isso, a maturidade real do agente não depende só de “ter tools” ou “ter prompts”, mas de como essas camadas se conectam.

## 12. Conclusão

O agente é melhor entendido como um sistema de workflow iterativo orientado por modelo.

Ele não apenas responde, mas:

- formula estratégia;
- organiza trabalho;
- executa ações;
- persiste estado;
- retoma contexto;
- observa resultados;
- corrige o próprio curso.

Esse é o núcleo técnico do comportamento agentivo do sistema.
