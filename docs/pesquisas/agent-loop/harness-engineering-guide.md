# Harness Engineering: O Que Realmente Importa ao Construir Agentes de IA

> Baseado em pesquisas de Stanford, Tsinghua, DeepMind, LangChain e Anthropic (Março 2026)

---

## O Problema

Mesmo modelo. Mesmo benchmark. **6x de diferença de performance.**

Pesquisadores de Stanford descobriram que o **codigo de orquestração** ao redor de um modelo de linguagem gera mais variação de performance do que o próprio modelo. A LangChain confirmou: modificando apenas a infraestrutura do harness, seu agente de codificação saltou de fora do top 30 para o **5º lugar** no Terminal Bench 2.

A conclusão é direta: se você constrói agentes, você é um **engenheiro de harness** — reconhecendo isso ou não.

---

## O Que É Harness

**Agente = Modelo + Harness**

A analogia com sistema operacional esclarece:

| Componente de SO | Equivalente no Agente |
|---|---|
| CPU | LLM (poderoso, mas inerte sozinho) |
| RAM | Context window (rápida, limitada) |
| Disco | Bancos de dados externos |
| Device drivers | Integrações com ferramentas |
| **Sistema operacional** | **Harness** |

Concretamente, harness é **tudo que não são os pesos do modelo**:

- System prompts
- Definições de ferramentas
- Lógica de orquestração
- Gerenciamento de memória
- Loops de verificação
- Guardrails de segurança

---

## Os 5 Padrões Canônicos de Orquestração

Identificados pela Anthropic, todo agente de produção combina esses padrões:

### 1. Prompt Chaining
Encadeamento sequencial de chamadas ao modelo, onde a saída de uma etapa alimenta a próxima.

### 2. Routing
O modelo decide qual caminho seguir baseado no input — como um switch/case semântico.

### 3. Parallelization
Múltiplas chamadas simultâneas ao modelo para tarefas independentes, consolidando resultados depois.

### 4. Orchestrator-Workers
Um agente orquestrador delega subtarefas para agentes trabalhadores especializados.

### 5. Evaluator-Optimizer Loops
Um agente gera, outro avalia. Ciclo repete até atingir critério de qualidade.

**As escolhas arquiteturais entre esses padrões — não o modelo por baixo — são o que gera os gaps de 6x de performance.**

---

## Os Dois Modos de Falha

Harnesses mal projetados falham de duas formas previsíveis:

### One-shotting
O agente tenta resolver tudo de uma vez e **esgota o contexto**. Sem decomposição, sem delegação — joga tudo na context window e torce pelo melhor.

### Premature Completion
Uma sessão posterior vê progresso parcial e **declara vitória**. O trabalho está meio feito, mas o agente não tem como distinguir "parcialmente completo" de "completo".

### A Solução da Anthropic
Evoluíram para uma arquitetura de 3 agentes inspirada em GANs:

```
Planner → Generator → Evaluator
                         ↑
              (clica no app como um usuário real)
```

Resultado: 20x mais caro ($200 vs $9), mas **a coisa central funciona em vez de estar quebrada**.

---

## O Que Funciona e O Que Não Funciona

A pesquisa de Tsinghua fez ablação módulo por módulo e descobriu algo contraintuitivo:

| Módulo | SWE-Bench | OS World | Veredicto |
|---|---|---|---|
| Self-evolution (loop de tentativas com gate de aceitação) | **+4.8** | **+2.7** | Consistentemente ajuda |
| Verificadores | -0.8 | **-8.4** | Ativamente prejudica |
| Busca multi-candidato | -2.4 | -5.6 | Ativamente prejudica |

**Mais estrutura nem sempre é melhor.**

O único módulo que ajuda consistentemente é o que **estreita o loop de tentativas do agente** — um ciclo que começa restrito e só amplia quando sinais de falha justificam. Disciplina bate força bruta.

### O Dado Mais Importante

~90% de todo compute flui pelos **agentes filhos delegados**, não pelo agente pai. O harness é um **padrão de orquestração**, não um padrão de raciocínio. Ele decompõe, delega e verifica.

---

## Representação Importa (Muito)

A mesma estratégia de agente, reescrita de código nativo para linguagem natural estruturada:

| Métrica | Código Nativo | Linguagem Natural |
|---|---|---|
| Taxa de resolução | 30.4% | **47.2%** |
| Tempo de execução | 361 min | **141 min** |
| Chamadas ao LLM | 1,200 | **34** |

**+16.8 pontos** apenas mudando como o harness é expresso. Mesma lógica, mesmo modelo.

---

## Meta-Harness: Otimização Automática

Stanford (Omar Khattab, criador do DSPy) criou o Meta-Harness — um sistema que **otimiza o harness automaticamente**.

### Como funciona

1. Um agente propositor (Claude Code + Opus 4.6) lê traces de execuções que falharam
2. Diagnostica o que quebrou
3. Escreve um harness completamente novo
4. Um avaliador testa a proposta
5. Repete

**Escala:** 10 milhões de tokens por iteração, 82 arquivos lidos por rodada.

### Resultados

- **Rank 2** com Opus no Terminal Bench 2
- **Rank 1** com Haiku — um modelo menor superando maiores apenas via otimização de harness
- 76.4% no Terminal Bench 2 — único sistema automaticamente otimizado num campo de entradas feitas à mão
- 48.6% em classificação de texto — 7.7 pontos acima do estado da arte com 4x menos tokens

### O Achado Que Muda Tudo

> Um harness otimizado em um modelo **transferiu para outros cinco modelos**, melhorando todos eles. O ativo reutilizável não é o modelo — é o harness.

---

## As Três Eras (em 4 anos)

```
Prompt Engineering → Context Engineering → Harness Engineering
```

Cada era absorve a anterior. Harness engineering incorpora prompt e context engineering e adiciona o que o modelo não consegue fazer sozinho: orquestração, memória, verificação, segurança.

---

## O Harness É Vivo: Poda > Construção

A Anthropic nomeou a dinâmica: **cada componente do harness codifica uma suposição sobre o que o modelo não consegue fazer sozinho — e essas suposições expiram.**

| Quem | O que fez |
|---|---|
| Anthropic | Quando Opus 4.6 parou de precisar de resets de contexto, **removeram completamente** |
| Manus | Reescreveu o harness **5 vezes em 6 meses** |
| Vercel | Removeu **80% das ferramentas** de um agente e obteve resultados melhores |

O espaço do harness não encolhe conforme modelos melhoram — **ele se move**. Trabalho maduro de harness se parece menos com construir estrutura e mais com **podar**.

**Um ofício tanto de subtração quanto de adição.**

---

## Riscos e Problemas Abertos

- **Segurança:** Lógica portável de harness facilita a disseminação de workflows arriscados
- **Prompt injection:** Injeções enterradas em texto de harness ou ferramentas maliciosas enxertadas em artefatos compartilhados
- **Vulnerabilidades em skills compartilhadas:** Pesquisas já encontraram que **1 em cada 4** skills contribuídas por comunidade contém uma vulnerabilidade
- **Co-evolução:** A questão mais consequente em aberto — harness e pesos do modelo podem ser co-evoluídos? Estratégia moldando o que o modelo aprende, e o modelo remodelando a estratégia que o envolve?

---

## Takeaways Para a Equipe

1. **Investir no harness gera ganhos maiores, mais rápidos e mais confiáveis do que esperar o próximo upgrade de modelo**

2. **Comece estreito, amplie sob evidência** — o único módulo consistentemente benéfico é o que restringe tentativas até que falhas justifiquem expansão

3. **Meça antes de adicionar estrutura** — verificadores e busca multi-candidato prejudicaram performance nos benchmarks. Mais complexidade ≠ melhor resultado

4. **Revise e pode regularmente** — suposições sobre limitações do modelo expiram. O que era necessário ontem pode ser peso morto hoje

5. **Trate o harness como código de primeira classe** — com testes, versionamento e ablação. Se você não consegue desligar um componente e medir o impacto, você não sabe se ele ajuda

6. **A pergunta não é mais "qual modelo escolher"** — é **"qual estrutura remover"**

---

*Fonte: [The Rise of Harness Engineering](https://youtube.com/watch?v=Xxuxg8PcBvc) — compilação de pesquisas de Stanford, Tsinghua, DeepMind, LangChain e Anthropic (Março 2026)*
