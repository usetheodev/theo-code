# VeRO: An Evaluation Harness for Agents to Optimize Agents

**Paper:** arXiv:2602.22480v1
**Autores:** Varun Ursekar, Apaar Shanker, Veronica Chatrath, Yuan (Emily) Xue, Sam Denton
**Instituição:** Scale AI
**Data:** February 25, 2026
**Contato:** varun.ursekar@scale.com

---

## 1. O que é

VeRO (Versioning, Rewards, Observations) é um harness de avaliação para **agent optimization** — o processo de usar um coding agent (o "optimizer") para melhorar iterativamente outro agent (o "target agent") via ciclos edit→execute→evaluate.

**Insight central:** Agent optimization difere fundamentalmente de engenharia de software convencional. O target agent entrelaça código determinístico com completions estocásticas de LLM, exigindo captura estruturada de raciocínio intermediário E resultados de execução downstream.

**Problema que resolve:** Sem infraestrutura como VeRO, otimização é não-reprodutível — optimizers falham em completar runs ou reportam resultados inconsistentes.

## 2. Formalização

### Target Agent Task T = (I, O, E)
- **I** = input space (instâncias de tarefas)
- **O** = output space (resposta final + execution trace com tool calls e raciocínio)
- **E: O → [0,1]** = evaluation function (scores via trace + resposta)

### Optimization Task P
- **Objetivo:** Encontrar A* = argmax E[E(A(x))] sobre held-out test data
- **Sujeito a:** n_E ≤ B (budget de evaluation calls)
- **Search space A_r:** subconjunto restrito (model checkpoints fixos, APIs permitidas, file permissions)
- **Métrica prática:** lift = E(A+) - E(A^base) (melhoria sobre baseline)

### O Optimizer S
A cada passo t, produz uma versão atualizada do target agent:
```
A_{t+1} = S(f({A_i, τ_i}_{i=0}^t), C)
```
Onde f é a observation function (o que o optimizer vê) e C é o contexto adicional.

## 3. Protocolo de Avaliação (6 requisitos)

| # | Requisito | O que garante |
|---|-----------|---------------|
| 1 | **Versioning** | Toda modificação do target = discrete snapshot (Git commits). Rollback, diff, trajectory analysis |
| 2 | **Budget enforcement** | n_E ≤ B. Tracking de evaluation calls, blocking quando excede |
| 3 | **Permission control** | Search space A_r enforced programaticamente — sem acesso a test data, sem trocar model checkpoint |
| 4 | **Reproducible execution** | Dependency locking (uv lockfiles), environment isolation |
| 5 | **Structured tracing** | Traces τ_t = {(x, o, e)} com inputs, outputs, scores — signal direcional para o optimizer |
| 6 | **Standardized observation interface** | f consistente entre todos optimizers comparados |

## 4. Arquitetura VeRO

```
┌─────────────────────┐     ┌─────────────────────┐     ┌─────────────────────┐
│   Optimizer Space    │     │    VeRO Harness      │     │   Target Space       │
│   (The Builder)      │     │  (Infrastructure)    │     │   (The Subject)      │
│                      │     │                      │     │                      │
│  Coding Agent ───────┼──→  │  Git Worktree ────→  │     │  Target Agent Code   │
│  (Edit, Run, Inspect)│     │  (Version Control)   │     │  (Python Package)    │
│                      │     │                      │     │                      │
│  Read/Edit Target ───┼──→  │  Auto-Commit ────→   │     │  A_T                 │
│  Inspect Traces ─────┼──→  │  Experiment DB ──→   │     │  Task Runner         │
│  Submit New Target ──┼──→  │  Evaluation Engine   │     │  (Reward Function)   │
│                      │     │  (Sandboxed UV)      │     │                      │
└─────────────────────┘     └─────────────────────┘     └─────────────────────┘
```

### 5 Core Abstractions

| Abstração | Papel | Tool vs Hook |
|-----------|-------|-------------|
| **Git Worktree** | Isola modificações, auto-commit hook grava diffs imutáveis | Hook |
| **Dataset** | Gerencia splits (train/val/test), enforce access control | Tool (DatasetViewer) |
| **Filesystem** | Pattern-based access control sobre file operations | Hook |
| **Experiment Database** | Armazena traces τ_t (scores, errors, rollouts, statistics) | Tool (ExperimentViewer) |
| **Evaluator** | Executa target agents, computa métricas, enforce budget | Tool (ExperimentRunner) |

### Optimization Loop (Algorithm 1)

```
Require: Base agent A_0, budget B, context C
t ← 0, n_E ← 0
while n_E < B do
    // Inspect
    D ← DatasetViewer.GetSamples()
    τ_{<t} ← ExperimentViewer.GetTraces()
    code_t ← FileTools.Read(A_t)
    
    // Hypothesize & Implement
    Δ ← S(D, τ_{<t}, code_t, C)        // LLM proposes edit
    FileTools.Write(Δ)                   // auto-commit hook fires
    A_{t+1} ← GitControl.GetHead()
    
    // Evaluate
    τ_{t+1} ← ExperimentRunner.Run(A_{t+1})
    n_E ← n_E + 1
    
    // Iterate
    if score(τ_{t+1}) < score(τ_t) then
        GitControl.Rollback(A_t)         // optional
    end if
    t ← t + 1
end while
return argmax_i score(τ_i)
```

## 5. Benchmark Suite

### Tasks (5 domínios)

| Task | Domain | Train/Val/Test | Initial Tools |
|------|--------|----------------|---------------|
| GAIA | Multi-step reasoning | 50/87/— | — |
| GPQA | Science QA | 98/—/100 | — |
| MATH | Math | 59/60/486 | — |
| TAU-Bench Retail | Tool use | 100/20/115 | Original |
| SimpleQA | Factual QA | 46/45/80 | Wikipedia |

### Scaffold Variants testados

| Scaffold | Variant | Descrição |
|----------|---------|-----------|
| VeRO | Default | Full tools + sub-agent delegation + Cookbook (design pattern library) |
| VeRO | Orchestrator | Sub-agent delegation bias |
| VeRO | Resources Only | Restrito a prompts, tool descriptions, parameters |
| Claude Code | Pure | Sem VeRO tools |
| Claude Code | VeRO Tools | Com VeRO tools + traces + dataset access |

### Optimizer Models testados
- Claude Sonnet 4.5 (75 runs, default)
- Claude Opus 4.5 (15 runs)
- GPT-5.2-Codex (15 runs)

## 6. Resultados Principais

### 6.1 Benchmark Suite (Table 2)

| Scaffold | Variant | Model | GAIA | GPQA | MATH | Retail | SimpleQA | **Avg.** |
|----------|---------|-------|------|------|------|--------|----------|---------|
| *Baseline* | — | — | 0.07 | 0.60 | 0.87 | 0.38 | 0.61 | 0.50 |
| Claude Code | Pure | Sonnet | 0.13 (0.29) | 0.58 (0.63) | **0.88** (0.90) | 0.43 (0.46) | 0.65 (0.68) | 0.53 (0.59) |
| Claude Code | VeRO Tools | Sonnet | 0.14 (0.21) | 0.64 (0.71) | **0.88** (0.90) | 0.39 (0.43) | 0.67 (0.71) | 0.55 (0.59) |
| **VeRO** | **Default** | **Sonnet** | **0.26** (0.30) | 0.64 (0.65) | 0.86 (0.87) | **0.55** (0.66) | **0.73** (0.76) | **0.61** (0.65) |
| VeRO | Orchestrator | Opus | 0.18 (0.18) | 0.62 (0.66) | **0.88** (0.88) | 0.55 (0.57) | **0.74** (0.86) | 0.59 (0.63) |
| VeRO | Orchestrator | Sonnet | 0.16 (0.20) | 0.62 (0.65) | 0.87 (0.88) | 0.51 (0.72) | 0.71 (0.72) | 0.57 (0.63) |
| VeRO | Orchestrator | GPT-5.2 | 0.07 (0.09) | **0.65** (0.70) | **0.88** (0.90) | 0.40 (0.42) | 0.69 (0.72) | 0.54 (0.56) |
| VeRO | Resources Only | Sonnet | 0.11 (0.13) | 0.60 (0.64) | **0.88** (0.88) | 0.42 (0.43) | 0.69 (0.72) | 0.54 (0.56) |

*(Valores em parênteses = máximo; sem parênteses = média sobre N=3 iterations)*

### 6.2 Key Findings

**1. VeRO é necessário para ganhos reais:**
- Claude Code Pure: +2% avg (0%), +0% best
- Claude Code + VeRO Tools: +8% (6%)
- **VeRO Default: +22% (30%)**
- Harness completo >> tools isoladas. Sem infraestrutura, otimização é irrelevante.

**2. Sucesso é altamente task-dependent:**
- Tool-use tasks (GAIA, Retail, SimpleQA): ganhos consistentes com scaffolding rico
- Reasoning tasks (GPQA, MATH): pouco ou nenhum ganho com qualquer configuração
- **Resources Only underperforma consistentemente** — mudanças além de prompts são necessárias

**3. Nenhum modelo optimizer domina:**
- Sonnet > GPT-5.2 em GAIA, Retail, SimpleQA
- GPT-5.2 > Sonnet em GPQA
- Opus atinge melhor max em SimpleQA (0.86) mas não consistentemente
- **Performance é task × model dependent**

### 6.3 Robustness Study (Table 3)

Otimizações feitas com GPT-4.1 mini como target transferem para outros models?

| Task | Optimizer | Target Model | Init → Final | Δ |
|------|-----------|-------------|-------------|---|
| GAIA | Sonnet | GPT-4.1 | 0.15 → 0.22 | **+0.07** |
| GAIA | Sonnet | Qwen3-30B | 0.11 → 0.16 | **+0.05** |
| GAIA | Codex | Qwen3-30B | 0.11 → 0.03 | **-0.08** |
| TAU-Bench | Sonnet | Claude-Sonnet | 0.59 → 0.81 | **+0.22** |
| TAU-Bench | Sonnet | GPT-4.1 | 0.55 → 0.79 | **+0.24** |
| TAU-Bench | Codex | Claude-Sonnet | 0.57 → 0.56 | **-0.02** |

**Transferência funciona dentro da mesma família de modelos** mas falha para out-of-family (Gemini-Flash, Qwen3-4B regressam frequentemente).

### 6.4 Case Study: Pawn vs Knight (GAIA)

Dois target agents com complexidade diferente:

| Agent | Tools | Prompt | Max turns | Complexity |
|-------|-------|--------|-----------|------------|
| **Pawn** | 4 | 25 lines | 20 | Minimal |
| **Knight** | 6 | 140 lines | 40 | Sophisticated (ReAct, Wikipedia, reflection) |

**Achado crítico: Headroom inversamente correlaciona com complexidade do agent.**
- Pawn lifts: +11.5% GAIA, +10.5% FACTS, +13.3% SimpleQA
- Knight lifts: +6.9% GAIA, +5.6% FACTS, +4.5% SimpleQA
- **Agentes simples têm mais room para melhorar que agentes sofisticados**

**Achado contra-intuitivo: Instruções ótimas dependem do agent.**
- Pawn: **Cookbook+Reasoning** (guidance detalhado) funciona melhor
- Knight: **Minimal** (menos guidance, mais liberdade) funciona melhor
- Guidance prescritiva constrange agentes já capazes — **template selection deve refletir baseline capabilities**

## 7. Interpretability Analysis

### O que optimizers realmente mudam (Figure 4)

| Tipo de mudança | Frequência |
|-----------------|-----------|
| **Prompt modifications** | **>50%** de todas as fases, consistentemente |
| Tool changes | Aumentam quando a task demanda |
| Workflow changes | Minority |
| Config changes | Rare |

**Current optimizers default to prompt edits over structural changes.** Diversidade de mudanças diminui ao longo das fases — optimizers "revert to prompts" quando mudanças mais ambiciosas falham.

### Variance-performance tradeoff
- High-variance templates (Cookbook+Reasoning para Pawn) → higher peak, higher risk
- Low-variance templates (Evidence-Based) → stable but capped
- **Orchestrator mantém mais diversidade ao longo das fases** que os outros variants

### Cross-task generalization é frágil
- Pawn iteration 3 com Cookbook+Reasoning: +5.75% GAIA, **-17.8% SimpleQA**
- O commit adicionou um multi-step verification tool que melhorou multi-hop reasoning mas adicionou overhead desnecessário para factual queries simples
- **Otimizações task-specific podem prejudicar cross-task**

### Runtime efficiency varia por template
- Evidence-Based: **2× mais rápido** que Tool-Centric (26.2s vs 56.6s para Knight)
- Lightweight modifications + discouragement of complex tool additions = competitive accuracy + lower cost

## 8. Relevância para Theo Code

### Thresholds para o SOTA validation loop

| Métrica | Valor | Uso |
|---------|-------|-----|
| VeRO Default avg lift | +0.11 (22% relativo) | Target para self-evolution loop |
| Best single-task lift | +0.19 (GAIA, Pawn, Cookbook+Reasoning) | Upper bound de melhoria por iteração |
| Prompt-edit dominance | >50% das mudanças | Nosso loop deve ir além de prompts |
| Cross-task regression rate | Observada (GAIA +5.75% → SimpleQA -17.8%) | Validar em MÚLTIPLAS tasks após cada fix |
| Transfer same-family | +0.07 a +0.24 | Optimizações transferem dentro da família |
| Transfer out-of-family | -0.08 a +0.00 | NÃO transferem para modelos diferentes |

### Design decisions para o SOTA loop

1. **Git worktree + auto-commit = nosso baseline snapshot + DISCARD marker** — VeRO valida que essa é a abordagem correta
2. **Budget enforcement via evaluation calls** — nosso hook conta iterações, mas deveria contar evaluation calls também
3. **Structured tracing (Experiment Database)** — nosso `history.jsonl` é uma versão simplificada; VeRO mostra que traces completos (inputs, outputs, scores) são mais valiosos
4. **Observation interface padronizada** — nosso probe-runner.sh faz isso, mas VeRO é mais rigoroso (DatasetViewer, ExperimentViewer)
5. **Template selection deve refletir agent complexity** — se Theo Code já é sofisticado (72 tools, 16 crates), guidance mínima pode ser melhor que prescritiva
6. **Diversificar além de prompts** — >50% das mudanças de optimizers são prompts; nosso loop deve forçar mudanças estruturais (tools, workflow, config) quando prompts estagnarem
7. **Cross-task validation obrigatória** — nunca validar fix em uma feature sem verificar regressão nas outras

### Anti-patterns identificados

1. **Prompt-only optimization** → Estagna após poucas iterações. Nosso loop deve detectar "3 iterações consecutivas mudando apenas prompts" e forçar structural change
2. **Task-specific overfitting** → Fix que melhora GAIA pode piorar SimpleQA. Sempre rodar probe-runner.sh completo, não apenas o probe da feature fixada
3. **Ignorar o tradeoff variance-performance** → High-variance approaches (ambiciosas) podem bater caps. Nosso quality evaluator deve permitir high-variance no início e tighten ao longo dos ciclos

---

**Citação:**
```
Ursekar, V., Shanker, A., Chatrath, V., Xue, Y., & Denton, S. (2026). VeRO: An Evaluation Harness for Agents to Optimize Agents. arXiv:2602.22480v1.
```
