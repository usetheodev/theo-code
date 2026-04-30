# ProjDevBench: Benchmarking AI Coding Agents on End-to-End Project Development

**Paper:** arXiv:2602.01655v2
**Autores:** Pengrui Lu, Shiqi Zhang, Yunzhong Hou, Lyumanshan Ye, Chaoyi Huang, Zixi Chen, Ji Zeng, Hantao Jiang, Pengfei Liu, Yiwei Wang, Ming-Hsuan Yang
**Instituições:** UC Merced, Shanghai Jiao Tong University, Shanghai Innovation Institute, Beijing Institute of Technology
**Data:** February 10, 2026
**Repo:** https://github.com/zsworld6/projdevbench

---

## 1. O que é

ProjDevBench é um benchmark end-to-end para avaliar agentes de codificação em **construção completa de projetos** — não patches, não funções isoladas, mas repositórios inteiros a partir de especificações em linguagem natural.

Diferença fundamental vs benchmarks existentes:

| Benchmark | From Scratch | Output | E2E Exec. | Diag. Feedback | Code Review |
|-----------|-------------|--------|-----------|----------------|-------------|
| HumanEval / MBPP | ✗ | Function | ✗ | ✗ | ✗ |
| SWE-bench | ✗ | Patch | ✗ | ✗ | ✗ |
| DevEval | ✓ | Repository | ✗ | ✗ | ✗ |
| E2EDevBench | ✓ | Repository | ✓ | ✗ | ✓ |
| **ProjDevBench** | **✓** | **Repository** | **✓** | **✓** | **✓** |

ProjDevBench é o único que combina: construção from-scratch, execução E2E, feedback diagnóstico granular, E code review.

## 2. Design do Benchmark

### 2.1 Task Definition

- **Input:** Especificação em linguagem natural (funcionalidade, I/O, constraints)
- **Output:** Repositório Git completo (múltiplos source files, build config como CMakeLists.txt, executável compilável)
- **Dois settings:** project-completion (codebase parcial fornecido) e project-creation (do zero)
- **20 problemas** curados em 8 categorias: Algorithm, Data Structure, Assembly, Management, Game, Interpreter, Storage, Optimization
- **Média:** 138 interaction turns, 4.81M tokens por problema, até 2 horas por task

### 2.2 Pipeline de Curação (3 estágios)

1. **Initial Collection:** ~2,800 problemas de plataforma Online Judge educacional
2. **Scope Filter:** Reter apenas project-level (multi-file, build system, CMake) → ~100
3. **Quality Filter:** Specs claras, test suites compreensivas, sem shortcuts → 20 finais

### 2.3 Protocolo de Avaliação Dual

**Execution-based (80% do score final):**
- Online Judge compila, executa, testa contra test suites
- Feedback granular: Accepted, Wrong Answer, Compile Error, Time Limit Exceeded, Runtime Error, Memory Leak, Memory Limit Exceeded
- Execution Score = soma ponderada de test cases passados

**Code Review (20% do score final):**
- Rule-based: artefatos de build presentes? bibliotecas proibidas? executável com nome correto?
- LLM-based: padrão FS-as-DB? dados em memória? compliance com spec?
- Code Review Score = compliance com especificação

**Final Score = 0.80 × Execution + 0.20 × Code Review**

## 3. Resultados Principais

### 3.1 Performance por Agent Framework + Model

| Agent | Model | Easy (Final) | Hard (Final) | Overall (Final) |
|-------|-------|-------------|-------------|----------------|
| **Codex** | **GPT-5** | **79.81** | **71.95** | **77.85** |
| Augment | GPT-5 | 76.88 | 58.78 | 72.35 |
| Cursor | GPT-5 | 71.90 | 71.69 | 71.85 |
| Claude Code | Sonnet-4.5 | 72.06 | 68.87 | **68.87** |
| Gemini CLI | Gemini-3-Pro | 75.72 | 47.26 | 68.61 |
| GitHub Copilot | Sonnet-4.5 | 74.46 | 42.15 | 67.18 |

**Key finding:** Codex+GPT-5 lidera com 77.85. Claude Code+Sonnet-4.5 alcança o maior code review score (89.31) mas fica atrás em execução.

### 3.2 Modelos Open Source no Claude Code

| Model | Exec. | CR | Final |
|-------|-------|-----|-------|
| GPT-5 | 50.69 | 84.33 | 57.34 |
| Sonnet-4.5 | 66.85 | 92.89 | 68.87 |
| GLM-4.6 | 56.25 | 80.89 | 57.95 |
| Kimi-k2-0905-Preview | 53.49 | 65.89 | 52.77 |
| DeepSeek-V3.2-Exp | 50.05 | 60.78 | 50.33 |

### 3.3 Distribuição de Falhas

| Status | Count | Percentage |
|--------|-------|------------|
| **Accepted** | **484** | **27.38%** |
| Wrong Answer | 740 | 41.86% |
| Time Limit Exceeded | 246 | 13.91% |
| Runtime Error | 124 | 7.01% |
| Compile Error | 80 | 4.52% |
| Memory Leak | 62 | 3.51% |
| Memory Limit Exceeded | 24 | 1.36% |

**Apenas 27.38% das submissões são aceitas.** Maioria falha por wrong answers (42%) ou time limits (14%).

## 4. Análise de Falhas (Section 5 — o mais valioso)

### 4.1 Specification Misalignment
- Agentes geram código sintaticamente correto mas omitem lógica de negócio crítica
- Exemplo: Train Ticket Management — todos os agentes omitiram seat management e user management
- Agentes acessaram apenas 3,789 de 3,825 safe blocks no Minesweeper
- Confundem contexto de dev vs submission (incluem test code com `main()` nos arquivos de submissão)

### 4.2 Edge Case Handling
- Fraquezas sistemáticas em boundary conditions
- Bookstore Hidden Test Points: todos falham em empty strings, file I/O exceptions, nested scenarios
- Runtime safety: Segmentation Faults, null pointer dereferences, array bounds violations
- Red-Black Tree: faltam null checks em rotation functions
- Bookstore: substring matching em vez de exact keyword matching

### 4.3 Time Complexity Deficiencies
- Agentes favorecem patterns familiares mas subótimos
- ICPC Management System: re-sort após cada unfreeze → O(K × N log N) vs O(K log N) com ordered data structures
- Recomputam orderings globais em vez de updates incrementais
- Usam `map` onde `unordered_map` bastaria (O(log N) vs O(1))
- I/O: unbuffered reads/excessive small writes

### 4.4 Resource Management Limitations
- Memory leaks: `std::stoi()` throwando exceções após alocar lhs/rhs sem liberar
- Preferem `new/delete` manual vs RAII
- Mini-Aidiv-N: 21 submissões com invalid matrix pointer access apesar de assertion checks

### 4.5 Code Engineering Gaps
- Template programming: assumem default constructors que não existem
- Namespace/header management: falham em merge de arquivos correto
- Incluem test code em submission files (confundem dev vs deployment)

### 4.6 Specification Compliance Failures (Code Review)
- Build systems mal configurados
- Executáveis com nomes incorretos
- Artefatos de build incluídos na submissão
- Usam `using namespace std` apesar de ser proibido
- Arquivos required omitidos
- Templates protegidos modificados apesar de restrição explícita

## 5. Correlações Estatísticas

| Variable Pair | Spearman ρ | p-value |
|---------------|-----------|---------|
| **Tokens vs. Score** | **-0.734** | **0.0002** |
| **Turns vs. Score** | **-0.668** | **0.0013** |
| **Turns vs. Tokens** | **0.898** | **< 0.0001** |
| Net Lines vs. Turns | 0.309 | 0.1848 |
| File Count vs. Score | -0.322 | 0.1665 |

**Key insight:** Mais interação = pior performance (ρ = -0.668). Problemas difíceis forçam interação prolongada, mas interação prolongada não produz progresso. Tokens altos vêm de turns repetidos, não de raciocínio longo.

Complexidade estática (file count, net lines) tem correlação fraca com performance — dificuldade se manifesta na interação, não no tamanho do código.

## 6. Validação do Code Review LLM

- LLM code review vs human judgment: Pearson r = 0.709 para readability (contínuo)
- Binary rule verification: accuracy = 85.2%, Cohen's κ = 0.710
- LLM code review é uma aproximação confiável de julgamento humano

## 7. Relevância para Theo Code

### Thresholds para o SOTA validation loop

| Métrica | Valor | Uso |
|---------|-------|-----|
| Overall acceptance rate | 27.38% | Baseline de dificuldade do benchmark |
| Best agent (Codex+GPT-5) | 77.85 | Target de referência para E2E project construction |
| Claude Code+Sonnet-4.5 | 68.87 | Posição atual do ecossistema Claude |
| Code Review accuracy (LLM vs human) | 85.2% | Confiança em avaliação automática |
| Tokens vs Score correlation | -0.734 | Mais tokens ≠ melhor resultado |

### Anti-patterns identificados (aplicáveis ao Theo Code)

1. **Specification misalignment** → O agente deve verificar TODOS os requisitos, não apenas os óbvios
2. **Edge case blindness** → Testes devem cobrir boundary conditions (empty input, null, overflow)
3. **Time complexity ignorance** → Preferir O(1) lookups, incremental updates, buffered I/O
4. **Resource management** → RAII em Rust (ownership system resolve isso), mas atenção com unsafe blocks
5. **Dev/deploy confusion** → Nunca incluir test code em artifacts de produção
6. **Extended interaction ≠ progress** → O loop SOTA deve detectar stall (já implementado: 2 ciclos sem progresso)

### Design decisions para incorporar

1. **Dual evaluation** (execution + code review) → Nosso quality evaluator já faz algo similar com rubrics por fase
2. **Diagnostic feedback** (não apenas pass/fail) → Probes devem retornar tipo de falha específico
3. **Code review rules** → Adicionar verificação de compliance com spec (não apenas "testes passam")
4. **Interaction budget** → Limitar tokens por fix, não apenas iterações

## 8. Limitações do Paper

- Apenas 20 tasks, todas em C++ (generalização para outras linguagens incerta)
- Avaliação fully autonomous (sem HITL) — não reflete workflows com human gates
- Scaling do benchmark é difícil (curar project-level problems com bons test suites é caro)

---

**Citação:**
```
Lu, P., Zhang, S., Hou, Y., Ye, L., Huang, C., Chen, Z., Zeng, J., Jiang, H., Liu, P., Wang, Y., & Yang, M.-H. (2026). ProjDevBench: Benchmarking AI Coding Agents on End-to-End Project Development. arXiv:2602.01655v2.
```
