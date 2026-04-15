Segue uma **versão LLM-friendly estruturada parágrafo por parágrafo**, com foco em:

* clareza semântica
* chunking para RAG
* consistência de schema
* redução de ambiguidade
* otimização para agentes (planner/executor/judge)

Baseado no documento: 

---

# Code-Craft (HCGS)

---

## 🔹 1. PROBLEMA

### 📌 Contexto

* Entender codebases grandes é difícil.
* Ferramentas atuais:

  * tratam código como texto plano
  * ou usam estrutura local (limitada)
* LLMs:

  * funcionam bem em snippets
  * falham em contexto de repositório completo

### ⚠️ Limitações principais

* janela de contexto limitada
* dependências complexas entre funções
* dificuldade em encontrar código relevante

---

## 🔹 2. HIPÓTESE

### 💡 Ideia central

Representar código como:

```
grafo + sumarização hierárquica + embeddings
```

---

## 🔹 3. SOLUÇÃO: HCGS (Hierarchical Code Graph Summarization)

### 🧩 Definição

Sistema que:

1. constrói um grafo de código
2. gera resumos estruturados bottom-up
3. cria embeddings contextuais
4. permite busca semântica avançada

---

## 🔹 4. PRINCÍPIO FUNDAMENTAL

### 🚨 Problema dos embeddings tradicionais

* embedding de função isolada = perde contexto

### ✅ Solução

* propagar contexto via grafo de chamadas
* gerar embeddings com contexto hierárquico

---

## 🔹 5. ARQUITETURA

### 🏗️ Componentes

```yaml
components:
  - CodeGraphGenerator
  - SummaryGenerator
  - Storage
  - QueryEngine
```

### 🔁 Fluxo

```yaml
flow:
  - input: source_code
  - step1: gerar_code_graph
  - step2: gerar_summaries_hierarquicos
  - step3: gerar_embeddings
  - step4: armazenar_vector_db
  - step5: query + retrieval
```

---

## 🔹 6. CODE GRAPH GENERATOR

### 🎯 Objetivo

Transformar código em grafo dirigido

### 📊 Estrutura

```yaml
node_types:
  - file
  - class
  - function
  - method

edge_types:
  - calls
  - imports
  - inheritance
```

### ⚙️ Tecnologia

* LSP (Language Server Protocol)
* multilspy (cliente LSP)

### 💡 Benefícios

* multi-linguagem
* parsing robusto
* sem necessidade de parsers custom

---

## 🔹 7. STRUCTURED SUMMARY SCHEMA

### 🧱 Schema padrão

```yaml
ImplementationDetails:
  summary: string
  primary_algorithm: optional string
  key_operations: list[string]
  side_effects: list[string]
  performance_characteristics: optional string

Dependencies:
  child_functions:
    - function_name: summary
  external_interactions:
    - string

StructuredSummary:
  implementation_details: ImplementationDetails
  dependencies: Dependencies
```

### 🎯 Objetivo

* padronizar saída do LLM
* permitir query estruturada
* reduzir ambiguidade

---

## 🔹 8. SUMMARY GENERATOR

### 🤖 Função

Gerar summaries usando LLM

### 🧠 Input do LLM

```yaml
input:
  - codigo_da_funcao
  - child_context (summaries das dependências)
```

### 📤 Output

```yaml
output:
  - structured_summary (JSON estrito)
```

---

## 🔹 9. ALGORITMO PRINCIPAL

---

### 🔹 9.1 Construção de níveis (topologia)

```pseudo
while nodes_restantes:
  pegar_nodes_sem_dependencias
  adicionar_ao_nivel
  remover_do_grafo
```

### ⚠️ Trata ciclos:

* break_cycle()

---

### 🔹 9.2 Processamento paralelo

```yaml
for level in levels:
  processar_todos_os_nodes_em_parallel
```

### 💡 Propriedades

* determinístico
* escalável
* eficiente

---

### 🔹 9.3 Geração de summary por nó

```pseudo
child_summaries = pegar_summaries_dependencias
summary = LLM(code, child_summaries)
```

---

### 🔹 9.4 Geração de summary por módulo

```pseudo
module_summary = LLM(lista_de_funcoes)
```

---

## 🔹 10. STORAGE & RETRIEVAL

### 🧠 Pipeline

```yaml
pipeline:
  - structured_summary
  - embedding_model (MiniLM)
  - vector_db (ChromaDB)
```

### 🔍 Query

```yaml
query_flow:
  - gerar_embedding(query)
  - buscar_top_k
  - opcional: expandir_subgrafo
```

---

## 🔹 11. INOVAÇÕES

```yaml
innovations:
  - linguagem_agnostica (LSP)
  - propagacao_contexto (bottom-up)
  - embeddings_contextuais
  - schema_estruturado
  - context_graph_engine
```

---

## 🔹 12. METODOLOGIA

### ⚠️ Problemas benchmarks atuais

* funções isoladas
* labels incorretos (~51%)
* sem contexto de repositório

---

### 🧪 Avaliação

```yaml
comparacao:
  - code_only
  - summary_based
```

### 📏 Métricas

```yaml
metrics:
  - Pass@k
  - Coverage
  - NDCG
```

---

## 🔹 13. RESULTADOS

### 📈 Principais ganhos

```yaml
libsignal:
  Pass@1: +82%

ingress-nginx:
  Pass@1: +76%

geral:
  melhorias consistentes
  maiores ganhos em codebases grandes
```

---

## 🔹 14. LIMITAÇÕES

```yaml
limitations:
  - não captura comportamento dinâmico
  - depende da qualidade do LLM
  - LSP nem sempre completo
  - reprocessamento completo (sem incremental)
```

---

## 🔹 15. CONCLUSÃO

```yaml
summary:
  - abordagem hierárquica melhora retrieval
  - embeddings com contexto são superiores
  - ganhos maiores em sistemas complexos
```

---

# 🧠 ANÁLISE CRÍTICA (nível Staff/FAANG)

### ✔️ Pontos fortes

* arquitetura extremamente alinhada com RAG moderno
* resolve problema real de contexto
* abordagem escalável (level-based)

### ⚠️ Pontos críticos (importante para seu caso)

1. **Ainda não é "agent-ready"**

   * falta loop de validação (judge)
   * não garante consistência semântica global

2. **Sem memória incremental**

   * inviável em ambientes com deploy contínuo

3. **Sem runtime awareness**

   * ignora traces / execução real

4. **LLM = single point of failure**

   * risco de hallucination propagada

---


