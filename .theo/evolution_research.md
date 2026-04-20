# Evolution Research — SOTA Agent Memory

**Prompt source:** `outputs/agent-memory-plan.md`
**Deep research:** `outputs/agent-memory-sota.md` (3800 palavras, 9 secoes)
**Team review:** `.claude/meetings/20260420-134446-agent-memory-sota.md` (16 agentes, veredito REVISED, 20 decisoes)
**Date:** 2026-04-20
**Baseline:** 73.300 (L1=96.1, L2=50.5)

## 1. Starting context

theo-code ja tem skeleton em `theo-domain`: `memory.rs` (MemoryProvider trait), `session_summary.rs`, `working_set.rs`, `episode.rs`. Gaps criticos: (a) coordinator `MemoryEngine` sem casa (decision engine nao existe); (b) Reflection e Meta-Memory = zero tipos; (c) Karpathy LLM Wiki nao existe; (d) wiring do MemoryProvider no `agent_loop.rs` pendente; (e) `theo-domain::evolution::Reflection` ja existe e colide com o novo tipo proposto → renomear para `MemoryLesson`.

## 2. Reference patterns (3 referencias primarias)

| Padrao | Origem | Uso no plano |
|---|---|---|
| MemoryProvider lifecycle (prefetch/sync_turn/on_pre_compress) | `referencias/hermes-agent/agent/memory_provider.py:42-120` | Trait ja em theo-domain; RM0 wire-up |
| MemoryManager fan-out com error isolation | `referencias/hermes-agent/agent/memory_manager.py:97-206` | RM1 MemoryEngine |
| Hash-based incremental wiki compiler | `referencias/llm-wiki-compiler/README.md:82-116` | RM5a (puro) + RM5b (MockLLM) |

Bibliografia externa (verificada): CoALA (arXiv:2309.02427, TMLR 2024), MemGPT (arXiv:2310.08560), Zep/Graphiti (arXiv:2501.13956), Mem0 (arXiv:2504.19413), MemoryBank (arXiv:2305.10250). Omissao corrigida: A-MEM (arXiv:2502.12110) deferida para RM7.

## 3. Execution order (locked pela ata, C3)

```
RM-pre (5 items, paralelo) →
  RM0 (wire hooks) →
    RM1 (MemoryEngine) →
      RM3a (Builtin + security) →
        RM2 (retrieval) + UI inicio →
          RM4 (MemoryLesson + 7 gates) →
            RM5a (hash + lint puro) →
              RM5b (compiler MockLLM) + Lint tool + test-fixtures crate
```

Reordenacao vs proposta original: RM3a antes de RM2 (Builtin tem menos deps; security scan e pre-req de qualquer write path). UI pode comecar apos RM3a (nao esperar RM5).

## 4. Completion gate

`outputs/agent-memory-plan.md` §7 "Ready-to-execute checklist" e §8 traceability matrix definem o gate. Promise `TODAS TASKS, E DODS CONCLUIDOS E VALIDADOS` exige:

- Os 5 pre-reqs mergeados
- RM0 → RM5b com 61 ACs totais como named tests passando
- `cargo test --workspace` exits 0
- `cargo check --workspace --tests` 0 warnings
- Harness score ≥ 73.300 (nao regredir)
- Pre-commit hook sem `--no-verify`
- 3 rotas UI + lint tool + test-fixtures crate presentes

## 5. Adaptacoes ao escopo do evolution-loop

- ≤ 200 LOC/fase respeitado; RM3a, RM4, RM5b no limite.
- Scope rules: todos os touches em `crates/` ou `apps/theo-cli`/`apps/theo-desktop` (NAO `apps/theo-benchmark`). UI Tauri em `apps/theo-desktop`.
- Nenhum novo workspace member exceto `theo-infra-memory` e `theo-test-memory-fixtures` (ambos justificados em ADR 008).
