# Natural-Language Agent Harnesses (NLAHs)

**Paper:** arXiv:2603.25723v1
**Autores:** Linyue Pan, Lexiao Zou, Shuo Guo, Jingchen Ni, Hai-Tao Zheng
**Instituições:** Tsinghua University (Shenzhen International Graduate School), Harbin Institute of Technology (Shenzhen)
**Data:** March 26, 2026

---

## 1. O que é

O paper formaliza o conceito de **Natural-Language Agent Harnesses (NLAHs)** — representações em linguagem natural da lógica de controle de um harness (roles, contracts, stages, adapters, state, failure taxonomy) que são **executáveis** por um **Intelligent Harness Runtime (IHR)** compartilhado.

**Insight central:** A lógica de design-pattern do harness (o QUE fazer — stages, contracts, verification gates) pode ser separada da infraestrutura de runtime (o COMO — tool dispatch, sandbox, child lifecycle) e expressa em texto natural editável, portável, comparável e ablatável.

**Por que importa:** Harnesses hoje estão enterrados em código de controller, defaults de framework, e convenções runtime-specific. Isso torna impossível comparar, migrar, ou fazer ablation de módulos de harness. NLAHs resolvem isso externalizando o pattern layer.

## 2. Formalização

### Harness = 3 responsabilidades

| Aspecto | O que especifica |
|---------|-----------------|
| **Control** | Como trabalho é decomposto e agendado |
| **Contracts** | Que artefatos devem ser produzidos, que gates devem ser satisfeitos, quando parar |
| **State** | O que persiste entre steps, branches, e delegated workers |

### NLAH = 6 componentes explícitos

| Componente | O que é | Exemplo |
|-----------|---------|---------|
| **Contracts** | I/O requeridos, format constraints, validation gates, permission boundaries, retry/stop rules | "Contract: must output valid Python file `solution.py`" |
| **Roles** | Role prompts (solver, verifier, researcher, orchestrator) com responsabilidades não-overlapping | "Role: Solver — generate code; Role: Repairer — fix code based on error" |
| **Stage structure** | Topologia explícita de workload (plan → execute → verify → repair) | "Stages: 1. PLAN 2. EXECUTE 3. VERIFY 4. REPAIR (if failure)" |
| **Adapters & scripts** | Named hooks para ações determinísticas (testes, verifiers, retrieval, parsing) | "Adapter: run_tests — execute `python -m pytest solution.py`" |
| **State semantics** | O que persiste entre steps (artifacts, ledgers, child workspaces), como é reopened | "State: task_state.json (path-addressable, compaction-stable)" |
| **Failure taxonomy** | Named failure modes que drive recovery | "Failure: format_error → regenerate code; test_failure → go to REPAIR; tool_error → retry once" |

### IHR (Intelligent Harness Runtime) = 3 componentes

```
┌─────────────────────┐
│   In-loop LLM        │ ← Interpreta NLAH a cada step
│   (interpreter)      │
├─────────────────────┤
│   Runtime Charter     │ ← Policy + semantics + orchestration + child lifecycle
│   (shared policy)     │
├─────────────────────┤
│   Backend             │ ← Tool interface + agent calls + artifact ingestion
│   (Codex CLI)         │
└─────────────────────┘
```

**Separação-chave:** Runtime charter (policy compartilhada) vs Harness skill (lógica task-family). Na implementação: runtime charter = runtime skill (Codex), harness logic = harness skill (Codex).

## 3. File-Backed State Module

Estado durable externalizado em artefatos path-addressable no filesystem:

```
workspace/
├── artifacts/          # Outputs do agente
├── ledgers/            # Histórico de decisões
├── task_state.json     # Estado corrente (path-addressable)
├── template.txt        # Templates para reprodução
└── response.txt        # Último resultado
```

**3 propriedades enforced:**
1. **Externalized** — estado escrito em artefatos no disco, não em contexto transiente
2. **Path-addressable** — stages posteriores reopenam o objeto exato por path
3. **Compaction-stable** — estado sobrevive truncation, restart, e delegation

**Por que importa:** Long-horizon autonomy falha quando estado crítico é implícito/efêmero. Context-folding (compaction) e delegation (child agents) perdem estado que existe apenas na conversation history.

## 4. Resultados Experimentais

### 4.1 RQ1: Behavioral Effect (Full IHR vs ablations)

| Benchmark | Harness | Setting | Perf (%) | Prompt Tokens | Tool Calls | LLM Calls | Runtime (min) |
|-----------|---------|---------|----------|---------------|------------|-----------|---------------|
| SWE-bench Verified | TRAE | Full IHR | **74.4** | 16.3M | 642.6 | 414.3 | 32.5 |
| | | w/o RTS | 76.0 | 11.1M | 451.9 | 260.5 | 16.6 |
| | | w/o HS | 75.2 | 1.2M | 51.1 | 34.0 | 6.7 |
| | Live-SWE | Full IHR | **72.8** | 1.4M | 58.4 | 41.4 | 7.6 |
| | | w/o RTS | 76.0 | 1.1M | 41.0 | 28.2 | 5.5 |
| | | w/o HS | 75.2 | 1.2M | 51.1 | 34.0 | 6.7 |

**Key findings:**

1. **Process moves more than resolved rate.** Full IHR aumenta dramaticamente tokens (16.3M vs 1.2M), tool calls (642 vs 51), e runtime (32 vs 6.7 min), mas perf muda modestamente. O harness muda o COMO o agente trabalha, não apenas o resultado.

2. **~90% de compute vai para child agents delegados.** Na configuração TRAE Full IHR, apenas ~9% dos tokens/calls são do parent thread — 91% é delegated. Confirma o achado do Tsinghua ablation.

3. **Diferenças se concentram na fronteira difícil.** Em 125 samples SWE-bench, >110 concordam entre Full e ablations. As diferenças reais são poucos cases component-sensitive na fronteira.

4. **Full IHR age como solved-set replacer, não frontier expander.** Ganha alguns cases que ablations perdem, mas também perde direct-path repairs que settings mais leves mantêm.

### 4.2 RQ2: Module Composition & Ablation

Adição incremental de módulos a partir de Basic (bare Codex):

| Módulo adicionado | SWE-bench (%) | OSWorld (%) |
|-------------------|--------------|-------------|
| **Basic** (baseline) | 75.2 | 41.7 |
| + File-Backed State | 76.8 (+1.6) | 47.2 (+5.5) |
| + Evidence-Backed Answering | 76.8 (+1.6) | 41.7 (0.0) |
| + Verifier | 74.4 (-0.8) | 33.3 (-8.4) |
| + **Self-Evolution** | **80.0 (+4.8)** | **44.4 (+2.7)** |
| + Multi-Candidate Search | 72.8 (-2.4) | 36.1 (-5.6) |
| + Dynamic Orchestration | 75.2 (0.0) | 44.4 (+2.7) |

**Achados-chave (os mais importantes do paper):**

1. **Self-Evolution é o ÚNICO módulo consistentemente benéfico** (+4.8 SWE, +2.7 OSWorld). Funciona como acceptance-gated attempt loop — mantém search narrow até falhas justificarem expansão.

2. **Verifier PREJUDICA** (-0.8 SWE, **-8.4 OSWorld**). Adiciona checking layer independente, mas acceptance do verifier pode divergir da acceptance do benchmark.

3. **Multi-Candidate Search PREJUDICA** (-2.4 SWE, -5.6 OSWorld). Mais visible search behavior, mas overhead-heavy demais sob budget limitado.

4. **File-Backed State é o módulo lightweight mais útil** (+1.6 SWE, +5.5 OSWorld). Melhora process structure (auditability, handoff discipline, trace quality) sem impacto semântico direto.

5. **"More structure ≠ always better."** Módulos ajudam quando tightening o path da evidência intermediária ao acceptance condition. Não ajudam quando adicionam layers cujo "sucesso" diverge do benchmark acceptance.

### 4.3 RQ3: Code-to-Text Harness Migration

Migração de harness nativo (código-fonte) para NLAH reconstruído:

| Benchmark | Realization | Perf (%) | Prompt Tokens | Agent Calls | Tool Calls | Runtime (min) |
|-----------|------------ |----------|---------------|-------------|------------|---------------|
| OSWorld (OS-Symphony) | Code (native) | 30.4 | 11.4M | 99 | 651 | 361.5 |
| | **NLAH** | **47.2** | 15.7M | 72 | 683 | 140.8 |

**NLAH supera o harness nativo em +16.8 pontos no OSWorld** (47.2 vs 30.4), com runtime 2.5× menor.

**Por que a NLAH é melhor:**
- Nativo: desktop control loop (screenshot → GUI repair → retry). Brittle, preso em GUI-level recovery
- NLAH: contract-first runtime flow. Estado em task files + ledgers. Switches mais facilmente de GUI para file/shell/package operations quando GUI falha
- NLAH materializa verificação em artefatos (escrito → reopened → verificado), em vez de confiar em screen plausibility

**O efeito da migração não é perda de orquestração, mas relocação de mecanismos de reliability** — de screen repair local para durable state + artifact-backed closure.

## 5. Design Lessons (Section 5)

### 5.1 Code vs Natural Language

> "Natural language carries editable high-level harness logic, while code remains responsible for deterministic operations, tool interfaces, and sandbox enforcement."

NLAHs não substituem código. Elas externalizam o **pattern layer** (roles, stages, contracts, failure taxonomy) enquanto o código cuida das **operações determinísticas** (test execution, tool dispatch, sandboxing).

### 5.2 Why Natural Language Still Matters

> "Natural language remains important when used to specify harness-level control — roles, contracts, verification gates, durable state semantics, and delegation boundaries — rather than only one-shot prompt phrasing."

Mesmo com modelos melhores, harness-level control em NL continua relevante porque especifica CONTRATOS e GATES, não apenas frasing de prompts.

### 5.3 Harnesses as Searchable Objects

> "Once harnesses are explicit objects, they become a search space. Explicit harness modules can be manually designed, retrieved, migrated, recombined, and systematically ablated."

**Implicação futura:** Otimização automática sobre representações de harness (vs opaque bundle engineering) — convergência com VeRO/Meta-Harness.

## 6. Números Consolidados

| Métrica | Valor | Contexto |
|---------|-------|---------|
| Self-Evolution lift (SWE) | **+4.8** | Único módulo consistentemente benéfico |
| Self-Evolution lift (OSWorld) | **+2.7** | Confirma cross-benchmark |
| Verifier harm (SWE) | **-0.8** | Prejudica performance |
| Verifier harm (OSWorld) | **-8.4** | Severo dano |
| Multi-Candidate harm (SWE) | **-2.4** | Overhead > benefício |
| Multi-Candidate harm (OSWorld) | **-5.6** | Overhead > benefício |
| File-Backed State lift (OSWorld) | **+5.5** | Melhor módulo lightweight |
| NLAH vs native (OSWorld) | **+16.8** | Harness em NL supera código nativo |
| Compute in child agents | **~91%** | Parent thread usa ~9% |
| TRAE Full IHR score | **74.4** | SWE-bench Verified (GPT-5.4) |

## 7. Relevância para Theo Code

### Confirmações diretas dos achados Tsinghua

Este paper **replica e confirma** os achados do Tsinghua ablation study (citado em `harness-engineering-guide.md`) com metodologia mais rigorosa (shared runtime, controlled ablation):

| Achado Tsinghua | Confirmado? | Valor neste paper |
|----------------|-------------|------------------|
| Self-evolution +4.8 SWE | ✅ **Exato** | +4.8 SWE, +2.7 OSWorld |
| Verifiers harm | ✅ | -0.8 SWE, -8.4 OSWorld |
| Multi-candidate harm | ✅ | -2.4 SWE, -5.6 OSWorld |

**Confiança nos thresholds do SOTA loop aumenta significativamente** — os números são reproduzidos em estudo independente.

### Patterns para adotar

| Pattern | Aplicação no Theo Code | Prioridade |
|---------|----------------------|-----------|
| **Self-Evolution como acceptance-gated loop** | `theo-agent-runtime` — confirma que nosso keep/discard pattern é o approach correto | CRITICAL |
| **NÃO adicionar verifier separado** | Nosso quality-evaluator NÃO deve ser um verifier independente — deve ser integrado no loop | CRITICAL |
| **NÃO usar multi-candidate search** | Uma hipótese por vez, keep/discard. Já fazemos isso no SOTA loop | CONFIRMED |
| **File-Backed State** | `theo-agent-runtime` — externalizar estado em artefatos (task_state.json, ledgers), não apenas in-memory | HIGH |
| **NLAH como representação** | Nosso `templates/sota-prompt.md` já é uma NLAH! Roles, stages, contracts, failure taxonomy — tudo em markdown | VALIDATED |
| **~90% compute em child agents** | `theo-agent-runtime` subagent system deve receber a maioria do compute budget | MEDIUM |
| **Compaction-stable state** | Estado do SOTA loop (`.claude/sota-loop.local.md`) já sobrevive compaction — bom | CONFIRMED |

### Anti-patterns reforçados

1. **Verifier como módulo separado** → -0.8 a -8.4 pontos. NUNCA adicionar verifier standalone no Theo Code
2. **Multi-candidate search** → -2.4 a -5.6 pontos. NUNCA gerar múltiplas soluções e escolher a melhor
3. **"More structure = always better"** → FALSO. Módulos ajudam apenas quando tightening path evidence → acceptance
4. **Estado implícito/efêmero** → Falha em long-horizon. Externalizar em artefatos path-addressable

### Thresholds para o SOTA registry

| Métrica | Valor | Confidence |
|---------|-------|-----------|
| Self-evolution minimum lift | +4.8 SWE-bench points | **0.95** (replicated) |
| Verifier impact (must be negative or zero) | -0.8 to -8.4 | **0.92** (replicated) |
| Multi-candidate impact (must be negative) | -2.4 to -5.6 | **0.90** (replicated) |
| File-backed state lift | +1.6 to +5.5 | **0.80** |
| Compute delegation ratio | ~91% in child agents | **0.85** |
| NLAH vs native code harness | +16.8 OSWorld points | **0.75** (single benchmark) |

---

**Citação:**
```
Pan, L., Zou, L., Guo, S., Ni, J., & Zheng, H.-T. (2026). Natural-Language Agent Harnesses. arXiv:2603.25723v1.
```
