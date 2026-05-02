# Meta-Harness: End-to-End Optimization of Model Harnesses

**Paper:** arXiv:2603.28052v1
**Autores:** Yoonho Lee, Roshen Nair, Qizheng Zhang, Kangwook Lee, Omar Khattab, Chelsea Finn
**Instituições:** Stanford, MIT, KRAFTON
**Data:** March 30, 2026
**Project page:** https://yoonholee.com/meta-harness/
**Repo:** https://github.com/stanford-iris-lab/meta-harness-tbench2-artifact
**Proposer:** Claude Code com Opus 4.6

---

## 1. O que é

Meta-Harness é um **outer-loop system** que usa um coding agent (o "proposer") para buscar automaticamente sobre o espaço de harnesses, otimizando o código de orquestração que envolve um LLM. A diferença fundamental dos text optimizers anteriores: **acesso total ao filesystem com todo o histórico** (source code + scores + execution traces de todas as tentativas anteriores), em vez de feedback comprimido em summaries ou scalar scores.

**Resultado headline:** Harnesses descobertos automaticamente **superam harnesses hand-engineered** em 3 domínios diferentes, e transferem para modelos não vistos durante a busca.

## 2. Por que Text Optimizers Falham em Harnesses

| Método | Histórico | Conteúdo do log | MTok/iter |
|--------|-----------|-----------------|-----------|
| OPRO | Window | (solution, score) pairs | 0.002 |
| TextGrad | Last | Textual feedback on current | 0.015 |
| AlphaEvolve | Window | Program database + eval scores | 0.022 |
| GEPA | Summary | Reflective feedback from rollout traces | 0.008 |
| Feedback Descent | Summary | Comparison + textual feedback | 0.012 |
| TTT-Discover | Window | Prev. solution fragment | 0.026 |
| **Meta-Harness** | **Full** | ***all* logs and scores** | **10.0** |

**Gap:** ~1000× mais contexto disponível por iteração. Text optimizers comprimem demais — perdem informação diagnóstica causal. Meta-Harness dá ao proposer filesystem access ilimitado para grep, cat, e inspecionar seletivamente.

## 3. Arquitetura

### Search Loop (Algorithm 1)

```
Input: tasks X, LLM M, proposer P, iterations N
Initialize: population H (seed harnesses)
Initialize: filesystem D ← ∅

// Evaluate seeds
for H in H:
    E_H ← Evaluate(H, M, X)
    D ← D ∪ {(H, E_H)}

// Search
for t = 1...N:
    P queries filesystem D          // inspect prior harnesses + scores + traces
    P proposes k new harnesses {H_1,...,H_k}
    for H in {H_1,...,H_k}:
        if H passes interface validation:
            D ← D ∪ {(H, Evaluate(H, M, X))}

return Pareto frontier of harnesses stored in D
```

### Filesystem como Canal de Feedback

Para cada harness avaliado, o filesystem armazena:
- **Source code** completo do harness (Python)
- **Evaluation scores** por task instance
- **Execution traces** completas (reasoning traces, tool calls, model outputs, state updates)

O proposer acessa via `grep`, `cat`, `find` — decide por conta própria o que inspecionar. **Sem estrutura de feedback imposta** — o proposer é um agente autônomo.

### O que o Proposer Faz

1. Lê amplamente entre prior candidates e seus logs
2. Identifica prováveis causas de failure
3. Isola causal changes vs confounded edits
4. Propõe novo harness (single-file Python program) com mudanças targeted
5. **Cada harness é um programa executável completo** — prompts, retrieval, memory, state management, orchestration

**Prática:** Proposer lê mediana de **82 arquivos por iteração**, referenciando >20 prior candidates. ~60 harnesses avaliados em ~20 iterações por run.

## 4. Resultados

### 4.1 Online Text Classification

| Harness | USPTO | S2D | Law | Avg Acc | Context (K tokens) |
|---------|-------|-----|-----|---------|-------------------|
| Zero-Shot | 12.0 | 63.2 | 7.0 | 27.4 | 0 |
| Few-Shot (all) | 15.0 | 78.3 | 29.0 | 40.8 | 12.3 |
| ACE (hand-designed) | 16.0 | 77.8 | 29.0 | 40.9 | 50.8 |
| **Meta-Harness** | **14.0** | **86.8** | **45.0** | **48.6** | **11.4** |

**+7.7 pontos sobre ACE (SOTA hand-designed) com 4× menos context tokens.**

**10× mais rápido que text optimizers:** Meta-Harness atinge em 4 evaluations o que OpenEvolve e TTT-Discover alcançam em 40+.

### Ablation: O que importa no feedback?

| Interface | Median Acc | Best Acc |
|-----------|-----------|----------|
| Scores only | 34.6 | 41.3 |
| Scores + Summary | 34.9 | 38.7 |
| **Meta-Harness (full traces)** | **50.0** | **56.7** |

**Acesso a raw execution traces é o ingrediente-chave.** Summaries não substituem — podem até prejudicar (38.7 best vs 56.7).

### 4.2 Retrieval-Augmented Math (200 IMO-level problems)

| Method | GPT-5.4n | GPT-5.4m | Gem-3.1FL | Gem-3F | GPT-20B | Avg |
|--------|----------|----------|-----------|--------|---------|-----|
| No Retriever | 23.0 | 28.8 | 28.6 | 42.6 | 47.6 | 34.1 |
| BM25 Retrieval | 30.2 | 29.2 | 32.8 | 46.6 | 48.9 | 37.5 |
| **Meta-Harness** | **31.7** | **30.4** | **34.9** | **46.3** | **50.6** | **38.8** |

**+4.7 pontos em média sobre no-retriever, transferindo para 5 modelos held-out.** Um único harness descoberto melhora todos os modelos — o ativo reutilizável é o harness, não o modelo.

### 4.3 TerminalBench-2 (Agentic Coding)

| Harness | Auto | Pass (%) — Opus 4.6 |
|---------|------|---------------------|
| Claude Code | ✗ | 58.0 |
| Terminus 2 | ✗ | 62.9 |
| Terminus-KIRA | ✗ | 74.7 |
| ForgeCode | ✗ | 81.8 |
| **Meta-Harness** | **✓** | **76.4** |

| Harness | Auto | Pass (%) — Haiku 4.5 |
|---------|------|---------------------|
| Claude Code | ✗ | 27.5 |
| Goose | ✗ | 35.5 |
| **Meta-Harness** | **✓** | **37.6** |

**Rank #2 entre todos os Opus 4.6 agents** (atrás apenas do ForgeCode que não é reproduzível). **Rank #1 entre todos os Haiku 4.5 agents.** Único sistema automaticamente otimizado num campo de harnesses hand-crafted.

### OOD Generalization (Text Classification)

| Harness | Avg Acc across 9 unseen datasets |
|---------|--------------------------------|
| Few-Shot (all) | 69.6 |
| ACE | 70.2 |
| **Meta-Harness** | **73.1** |

**+2.9 pontos sobre o melhor hand-designed em 9 datasets nunca vistos durante search.** O harness captura estratégias gerais, não overfitting ao search set.

## 5. Comportamento Qualitativo do Proposer

Da trajetória de search (Appendix A):

1. **Iterações iniciais:** Proposer combina structural fixes + prompt-template edits, observa regressões em ambos
2. **Diagnóstico causal:** Propor hipótese de que regressions são causadas por confounded prompt intervention (shared prompt edit afetou ambas mudanças)
3. **Isolamento:** Separa structural change de prompt change, testa cada isoladamente
4. **Convergência:** Pivota para safer additive modification que se torna o melhor candidato

**Filesystem access permite ao proposer formar hipóteses causais** — não apenas reagir a scores, mas entender POR QUE algo regrediu.

## 6. Design Insights

### 6.1 Harness = Programa Executável Completo
> "A harness is a stateful program that wraps a language model and determines what context the model sees at each step."

Harness não é um prompt — é **prompts + retrieval + memory + state management + orchestration** empacotados como código executável.

### 6.2 Filesystem > Compressed Feedback
> "Access to raw execution traces is the key ingredient for enabling harness search."

Summaries descartam informação diagnóstica causal. O proposer precisa ver traces raw para entender relações entre harness choices e downstream failures.

### 6.3 Harnesses como Search Space
> "Once a search space is accessible, stronger general-purpose agents can outperform hand-engineered solutions."

O achado é um padrão recorrente em ML: make the search space accessible → automated search beats manual design.

### 6.4 Cross-Model Transfer
> "A harness optimized on one model transferred to five others, improving all of them."

O ativo reutilizável é o harness, não o modelo. Implicação: investir em harness engineering gera retorno multiplicado por cada modelo que o usa.

### 6.5 Regularization Bias
> "Coding models tend to propose coherent algorithms rather than brittle, hard-coded solutions."

Representar harnesses como programas (não templates) impõe um bias natural para soluções algorítmicas generalizáveis.

## 7. Números Consolidados

| Métrica | Valor | Contexto |
|---------|-------|---------|
| Text classification lift over SOTA | **+7.7 points** | vs ACE (hand-designed), 4× less context |
| Search speedup over text optimizers | **10×** | 4 evaluations vs 40+ |
| Math reasoning lift (IMO-level) | **+4.7 points avg** | Across 5 held-out models |
| TerminalBench-2 (Opus 4.6) | **76.4%** (Rank #2) | Only auto-optimized system |
| TerminalBench-2 (Haiku 4.5) | **37.6%** (Rank #1) | Beats all hand-crafted Haiku agents |
| OOD generalization | **73.1%** | 9 unseen datasets, +2.9 over hand-designed |
| Cross-model transfer | **5 models improved** | Single harness optimization |
| Files read per iteration | **82 median** | Full filesystem access |
| Prior candidates referenced | **20+** | Per proposer step |
| MTok per iteration | **10.0** | 1000× more than text optimizers |

## 8. Relevância para Theo Code

### Validação direta do SOTA loop

| Aspecto | Meta-Harness | Nosso SOTA Loop | Status |
|---------|-------------|-----------------|--------|
| Search over harness code | ✓ (Python programs) | ✓ (Rust crates) | **Aligned** |
| Full history via filesystem | ✓ (grep/cat) | ✓ (probe JSONs, history.jsonl, baselines) | **Aligned** |
| Proposer = coding agent | ✓ (Claude Code Opus 4.6) | ✓ (implementation-coder agent) | **Aligned** |
| Evaluate → log → iterate | ✓ (Algorithm 1) | ✓ (Phase 1→2→3→4→5→loop) | **Aligned** |
| Cross-model transfer | ✓ (+4.7 over 5 models) | Não testado ainda | **Gap** |

### Patterns a adotar

| Pattern | Aplicação | Prioridade |
|---------|-----------|-----------|
| **Raw traces > summaries** | `{output_dir}/probes/*.json` deve manter traces completos, não resumos | HIGH |
| **Filesystem como canal de feedback** | O SOTA loop já usa arquivos (baselines, progress, probes) — validado | CONFIRMED |
| **10M tokens de contexto diagnóstico por iteração** | Nosso loop deve dar acesso COMPLETO ao histórico, não compactar | HIGH |
| **Causal hypothesis from trace inspection** | gap-analyzer deve inspecionar traces de iterações anteriores para entender WHY | MEDIUM |
| **Pareto frontier over harnesses** | Manter múltiplas versões de quality (accuracy vs cost) ao longo dos ciclos | LOW |
| **Cross-model validation** | Após otimizar com um modelo, testar com outro para verificar transferência | MEDIUM |

### Thresholds para o SOTA registry

| Métrica | Valor | Source |
|---------|-------|-------|
| Auto-optimized harness vs hand-crafted (text class.) | **+7.7 points** | Meta-Harness vs ACE |
| Auto-optimized harness vs hand-crafted (coding) | **76.4% vs 74.7%** (Terminus-KIRA) | TerminalBench-2 |
| Cross-model transfer lift | **+4.7 points avg** | 5 held-out models, IMO math |
| OOD generalization | **+2.9 points** over hand-designed | 9 unseen datasets |
| Search efficiency (evals to converge) | **4 evaluations** vs 40+ for text optimizers | 10× speedup |
| Diagnostic context per iteration | **10M tokens** | vs 0.002-0.026M for text optimizers |

### Anti-patterns confirmados

1. **Compressed feedback (summaries, scalar scores)** → Loses causal diagnostic information. Raw traces are strictly better
2. **Template-filling optimization** → Harnesses need full program rewrites, not just slot-filling
3. **Evaluation without full trace logging** → If you don't log everything, the next iteration can't diagnose failures
4. **Single operating point** → Pareto frontier (accuracy vs cost tradeoff) reveals multiple useful harnesses
5. **Assuming hand-crafted > automated** → False. Automated search beats manual design once the search space is accessible

---

**Citação:**
```
Lee, Y., Nair, R., Zhang, Q., Lee, K., Khattab, O., & Finn, C. (2026). Meta-Harness: End-to-End Optimization of Model Harnesses. arXiv:2603.28052v1.
```
