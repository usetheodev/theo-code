# Scorecard de Pesquisa SOTA — Cobertura por Domínio

**Data:** 2026-04-30
**Escala:** 0 (nenhum conhecimento) → 5 (cobertura SOTA completa, pronto para implementar)

---

## Resumo Visual

```
Domínio                  Nota   ████████████████████ Barra
───────────────────────────────────────────────────────────
Agent Loop               4.5   ████████████████████▌
Memory                   4.0   ████████████████████
Self-Evolution           4.0   ████████████████████
Evals / Benchmarks       3.5   █████████████████▌
Model Routing            3.5   █████████████████▌
Context Engineering      3.0   ███████████████
Sub-agents               3.0   ███████████████
CLI                      2.0   ██████████
Tools                    1.5   ███████▌
Prompt Engineering       1.0   █████
Providers                1.0   █████
Security / Governance    1.0   █████
Observability            0.5   ██▌
Languages                0.5   ██▌
Debug (DAP)              0.5   ██▌
Wiki                     0.5   ██▌
───────────────────────────────────────────────────────────
MÉDIA GERAL              2.0
```

---

## Detalhes por Domínio

### 1. Agent Loop — 4.5/5 ⭐⭐⭐⭐½

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 5/5 | 6 research files + Tsinghua ablation replicado por NLAH |
| Referências mapeadas | 5/5 | opendev, hermes, Archon, GSD, superpowers |
| Thresholds definidos | 4/5 | Self-evolution +4.8, doom-loop, session continuity |
| Gaps claros | 4/5 | System reminders (8 detectors), ACC 5-stage alignment |
| Pronto para implementar | 4/5 | Falta detalhar implementação de system reminders em Rust |

**Arquivos:** `harness-engineering-guide.md`, `harness-engineering.md`, `harness-engineering-openai.md`, `effective-harnesses-for-long-running-agents.md`, `opendev-building-ai-coding-agents.md`, `nlah-natural-language-agent-harnesses.md`

**O que sabemos:** ReAct extended (6 fases), ACC 5-stage compaction (70/80/85/90/99%), doom-loop MD5 fingerprinting, system reminders com guardrail counters, dual-memory (episodic+working), self-evolution como único módulo benéfico (+4.8), verifiers prejudicam (-0.8 a -8.4), multi-candidate prejudica (-2.4).

**O que falta:** Benchmark quantitativo do Theo Code no TerminalBench-2 ou SWE-bench. Implementação concreta dos 8 event detectors de system reminders.

---

### 2. Memory — 4.0/5 ⭐⭐⭐⭐

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 5/5 | CoALA, MemGPT, Mem0, Zep, MemoryBank, Karpathy Wiki |
| Referências mapeadas | 5/5 | hermes MemoryProvider/Manager, llm-wiki-compiler |
| Thresholds definidos | 4/5 | 10 thresholds (recall p50, wiki-to-recall, forgetting curve) |
| Gaps claros | 4/5 | RM0-RM5b roadmap com acceptance criteria |
| Pronto para implementar | 3/5 | Roadmap detalhado mas nenhum RM implementado ainda |

**Arquivos:** `agent-memory-sota.md`, `agent-memory-plan.md`, `karpathy-llm-wiki-tutorial.md`, `How AI Agents Remember Things.md`

**O que sabemos:** 8-type taxonomy (STM/WM/LTM-semantic/episodic/procedural/Retrieval/Reflection/Meta), MemoryEngine coordinator pattern, security scan patterns, frozen-snapshot injection, forgetting curve, wiki compilation hash-based.

**O que falta:** Implementação real (0 de 8 types wired). Benchmark de recall latency. A-MEM Zettelkasten evaluation.

---

### 3. Self-Evolution / Autodream — 4.0/5 ⭐⭐⭐⭐

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 4/5 | VeRO, Meta-Harness, Tsinghua (via NLAH) |
| Referências mapeadas | 3/5 | Karpathy autoresearch, Stanford Meta-Harness |
| Thresholds definidos | 5/5 | +4.8 replicated, +7.7 Meta-Harness, cross-model +4.7 |
| Gaps claros | 4/5 | Acceptance gate, narrow-then-expand, ablation framework |
| Pronto para implementar | 3/5 | Conceito claro, implementação Rust não detalhada |

**Arquivos:** `vero-agent-optimization.md`, `meta-harness-end-to-end-optimization.md`

**O que sabemos:** Keep/discard é o approach correto (VeRO confirma), filesystem como canal de feedback (Meta-Harness confirma), raw traces >> summaries, cross-model transfer funciona, harness auto-otimizado supera hand-crafted, 10× speedup sobre text optimizers.

**O que falta:** Framework concreto de ablation para Theo Code (desligar componentes e medir impacto). Narrow-then-expand scope tracking.

---

### 4. Evals / Benchmarks — 3.5/5 ⭐⭐⭐½

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 4/5 | ProjDevBench, llvm-autofix, meeting-sota-research + 3 artigos |
| Referências mapeadas | 3/5 | SWE-bench, TerminalBench-2, llvm-bench, ProjDevBench |
| Thresholds definidos | 3/5 | Acceptance rates, cross-domain drops |
| Gaps claros | 4/5 | Expert review "genuine" rate, ChangeAssert anti-pattern |
| Pronto para implementar | 3/5 | Probes existem, mas falta benchmark suite unificada |

**Arquivos:** `projdevbench.md`, `llvm-autofix-compiler-harness.md`, `meeting-sota-research.md`, `The Quality Loop...md`, `AI Agent Evals...md`, `The Observability Layer...md`

**O que sabemos:** 27% acceptance rate (ProjDevBench), 62% drop domain-specific (llvm), >60% patches "aceitos" são incorretos após expert review, 3 anti-patterns (ChangeAssert, WrongLocalization, WrongFix), mais tokens = pior score (ρ=-0.734).

**O que falta:** Benchmark do Theo Code no SWE-bench/TerminalBench-2. Suite de evals domain-specific para Rust agents.

---

### 5. Model Routing — 3.5/5 ⭐⭐⭐½

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 3/5 | smart-model-routing.md cobre FrugalGPT, RouteLLM, Anthropic |
| Referências mapeadas | 5/5 | hermes smart_model_routing.py, opendev slots, Archon per-node |
| Thresholds definidos | 4/5 | +90.2% orchestrator-worker, -11% cost Advisor Strategy |
| Gaps claros | 3/5 | Implementation plan exists |
| Pronto para implementar | 3/5 | Trait design claro, falta implementação |

**Arquivos:** `smart-model-routing.md`, `smart-model-routing-plan.md`

**O que sabemos:** 5 model roles (Action/Thinking/Critique/Vision/Compact), rule-based routing (hermes pattern), cascading defaults (opendev slot pattern), per-subagent model override, prompt caching economics.

**O que falta:** Implementation real (zero routing no Theo Code). Latency benchmarks. Optimal rule set para Rust codebase.

---

### 6. Context Engineering — 3.0/5 ⭐⭐⭐

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 2/5 | context-engine.md (spec), OpenDev ACC (indireto) |
| Referências mapeadas | 4/5 | qmd RRF, fff.nvim frecency, opendev ACC |
| Thresholds definidos | 3/5 | MRR 0.90, depcov 0.96, recall@5 0.92 |
| Gaps claros | 3/5 | NDCG@5 unmeasured, per-lang recall unmeasured |
| Pronto para implementar | 3/5 | RRF exists, mas benchmarks incompletos |

**Arquivos:** `context-engine.md`

**O que sabemos:** GRAPHCTX assembly, RRF 3-ranker, +16.8 SWE-bench com representation change, ACC 5-stage compaction, tool result summarization (30K→100 tokens), prompt caching 88% savings.

**O que falta:** Paper dedicado sobre context engineering formal (Mei et al. CE survey citado no OpenDev paper). NDCG@5 benchmark. Per-language recall@5 measurement. Representation format optimization paper.

---

### 7. Sub-agents — 3.0/5 ⭐⭐⭐

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 2/5 | sota-subagent-architectures.md |
| Referências mapeadas | 5/5 | Claude Code, Codex, opendev, Archon, GSD, superpowers |
| Thresholds definidos | 2/5 | max_depth, max_concurrent |
| Gaps claros | 3/5 | File locking, peer messaging, shared task list |
| Pronto para implementar | 3/5 | SubAgentRole exists, falta coordination primitives |

**Arquivos:** `sota-subagent-architectures.md`

**O que sabemos:** 8 subagent types (OpenDev), orchestrator-worker dominante, schema gating > runtime blocking, ~90% compute em child agents, markdown-defined agent specs, auto-parallelization.

**O que falta:** Paper dedicado sobre multi-agent coordination (shared blackboard, peer-to-peer). File locking implementation. Benchmark de subagent delegation efficiency.

---

### 8. CLI — 2.0/5 ⭐⭐

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 1/5 | cli-agent-ux-research.md |
| Referências mapeadas | 3/5 | opendev TUI, opencode modes, pi-mono |
| Thresholds definidos | 1/5 | Apenas 17 subcommands listados |
| Gaps claros | 2/5 | Output modes, startup performance, completions |
| Pronto para implementar | 3/5 | CLI exists, mas sem research de UX profunda |

**Arquivos:** `cli-agent-ux-research.md`

**O que falta:** LongCLI-Bench results (<20% pass rates para long-horizon CLI tasks). Shell integration research. Comparative UX analysis (theo vs claude code vs codex vs cursor).

---

### 9. Tools — 1.5/5 ⭐½

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 0/5 | Nenhum paper dedicado a tool design |
| Referências mapeadas | 3/5 | opendev 35 tools, hermes 58 tools, pi-mono 7 tools |
| Thresholds definidos | 1/5 | Apenas tool count (72) |
| Gaps claros | 2/5 | Fuzzy edit matching, tool result summarization |
| Pronto para implementar | 1/5 | Sem research específica sobre tool schema quality |

**Arquivos:** Nenhum (pasta vazia)

**O que falta:** Paper sobre tool design patterns para coding agents. Fuzzy edit matching chain-of-responsibility. Tool result optimization benchmarks. MCP lazy discovery cost analysis. τ-Bench results (tool-use benchmark).

---

### 10. Prompt Engineering — 1.0/5 ⭐

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 0/5 | Nenhum paper dedicado |
| Referências mapeadas | 2/5 | OpenDev PromptComposer (indireto) |
| Thresholds definidos | 0/5 | Nenhum threshold quantitativo |
| Gaps claros | 1/5 | Mencionados no INDEX mas não pesquisados |
| Pronto para implementar | 1/5 | Conceitos conhecidos, sem evidência quantitativa |

**Arquivos:** Apenas INDEX.md

**O que falta:** Tudo. Paper sobre conditional prompt composition. Representation format experiments (+16.8 precisa source paper). Anti-hallucination techniques quantificadas. Progressive tool disclosure research. Provider-specific prompt sections.

---

### 11. Providers — 1.0/5 ⭐

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 0/5 | Nenhum paper dedicado |
| Referências mapeadas | 2/5 | opendev provider cache, hermes credential pool |
| Thresholds definidos | 0/5 | Apenas count (26 specs) |
| Gaps claros | 1/5 | Streaming, retry, token counting |
| Pronto para implementar | 2/5 | 26 specs exist, mas sem research de quality |

**Arquivos:** Apenas INDEX.md

**O que falta:** Paper sobre provider abstraction patterns. Streaming unification research. Retry strategy comparison (exponential backoff vs circuit breaker). Token counting accuracy benchmarks. OAuth refresh flow for long sessions.

---

### 12. Security / Governance — 1.0/5 ⭐

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 0/5 | Nenhum paper dedicado |
| Referências mapeadas | 3/5 | opendev 5-layer safety, hermes injection scan, rippletide |
| Thresholds definidos | 1/5 | Apenas secrets=0, sandbox_escape=0 |
| Gaps claros | 2/5 | Injection patterns, landlock benchmarks |
| Pronto para implementar | 1/5 | Crates existem mas sem research profunda |

**Arquivos:** Apenas INDEX.md

**O que falta:** Paper sobre defense-in-depth para coding agents. arXiv:2604.14228 (Dive into Claude Code — 7 safety layers). Injection scan pattern completeness analysis. Sandbox performance benchmarks. Supply chain security for MCP/skills.

---

### 13. Observability — 0.5/5 ½⭐

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 0/5 | Nenhum (artigos em evals/ são tangenciais) |
| Referências mapeadas | 1/5 | opendev CostTracker mencionado |
| Thresholds definidos | 0/5 | Nenhum threshold quantitativo |
| Gaps claros | 1/5 | Superficiais |
| Pronto para implementar | 0/5 | Sem base para design |

**Arquivos:** Apenas INDEX.md

**O que falta:** Tudo. OpenTelemetry para agent loops. Cost tracking accuracy standards. Trajectory format specification (RL-compatible?). Dashboard architecture. Langfuse/Braintrust/Helicone comparison.

---

### 14. Languages — 0.5/5 ½⭐

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 0/5 | Nenhum |
| Referências mapeadas | 1/5 | qmd AST-aware chunking |
| Thresholds definidos | 0/5 | Nenhum |
| Gaps claros | 0/5 | Não investigados |
| Pronto para implementar | 1/5 | 14 grammars existem sem research |

**O que falta:** Per-language parsing quality benchmarks. Symbol extraction completeness. Cross-language import resolution. New language grammar evaluation (Zig, Elixir, Dart).

---

### 15. Debug (DAP) — 0.5/5 ½⭐

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 0/5 | Nenhum |
| Referências mapeadas | 1/5 | llvm-autofix usa gdb (tangencial) |
| Thresholds definidos | 0/5 | Nenhum |
| Gaps claros | 1/5 | Gap 6.1 CRITICAL identificado na maturity analysis |
| Pronto para implementar | 0/5 | 11 tools registered, zero E2E tested |

**O que falta:** Tudo. DAP spec compliance research. Debugger adapter benchmarks. Security model for debug access.

---

### 16. Wiki — 0.5/5 ½⭐

| Critério | Score | Justificativa |
|----------|-------|---------------|
| Papers lidos | 0/5 | Karpathy tutorial está em memory/, não wiki/ |
| Referências mapeadas | 2/5 | llm-wiki-compiler, qmd |
| Thresholds definidos | 0/5 | Nenhum |
| Gaps claros | 1/5 | Compilation cost, lint rules |
| Pronto para implementar | 1/5 | WikiBackend trait exists, sem impl |

**O que falta:** Compilation cost benchmark. Wiki lint rule coverage. Wiki vs memory boundary clarification. Integration with retrieval pipeline.

---

## Prioridades de Pesquisa (ordenadas por impacto)

| Prioridade | Domínio | Nota Atual | Ação |
|-----------|---------|-----------|------|
| 🔴 CRITICAL | **Prompt Engineering** | 1.0 | Pesquisar conditional prompt composition, representation format, anti-hallucination |
| 🔴 CRITICAL | **Tools** | 1.5 | Pesquisar fuzzy edit matching, tool result optimization, MCP lazy discovery |
| 🔴 CRITICAL | **Observability** | 0.5 | Pesquisar OpenTelemetry, cost tracking, trajectory formats |
| 🟡 HIGH | **Security/Governance** | 1.0 | Ler arXiv:2604.14228 (Claude Code safety), injection patterns |
| 🟡 HIGH | **Context Engineering** | 3.0 | Ler CE survey (Mei et al.), benchmark NDCG@5 e per-lang recall |
| 🟡 HIGH | **Debug (DAP)** | 0.5 | Pesquisar DAP spec, debugger benchmarks |
| 🟢 MEDIUM | **Languages** | 0.5 | Benchmark per-language parsing quality |
| 🟢 MEDIUM | **Wiki** | 0.5 | Benchmark compilation cost, lint coverage |
| 🟢 MEDIUM | **Providers** | 1.0 | Pesquisar streaming unification, retry strategies |
| 🟢 MEDIUM | **CLI** | 2.0 | Pesquisar LongCLI-Bench, comparative UX |
| 🔵 LOW | **Sub-agents** | 3.0 | Pesquisar file locking, peer messaging |
| ✅ DONE | **Agent Loop** | 4.5 | Implementar — pesquisa suficiente |
| ✅ DONE | **Memory** | 4.0 | Implementar RM0-RM5b — pesquisa suficiente |
| ✅ DONE | **Self-Evolution** | 4.0 | Implementar keep/discard — pesquisa validada |
| ✅ DONE | **Model Routing** | 3.5 | Implementar routing — pesquisa suficiente |
