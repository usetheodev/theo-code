# Memory — Pesquisa SOTA

## Escopo
Sistema de memória multi-camada: STM, WM, LTM-semantic (builtin + wiki), LTM-episodic, LTM-procedural, Reflection/MemoryLesson, Meta-Memory, Retrieval-backed.

## Crates alvo
- `theo-domain` — traits (MemoryProvider, WikiBackend, Reflection, MetaMemory)
- `theo-infra-memory` — implementações (Builtin, RetrievalBacked, Wiki, LessonStore)
- `theo-application` — MemoryEngine coordinator

## Referências-chave
| Fonte | O que extrair |
|-------|---------------|
| CoALA (arXiv:2309.02427) | Taxonomia 6 tipos de memória |
| MemGPT (arXiv:2310.08560) | Virtual context, paging tool calls |
| Mem0 (arXiv:2504.19413) | ADD-only extraction, 91.6 LoCoMo |
| Zep/Graphiti (arXiv:2501.13956) | Temporal KG, 94.8 DMR, edge invalidation |
| MemoryBank (arXiv:2305.10250) | Ebbinghaus forgetting curve |
| Karpathy LLM Wiki | raw → compile → query, hash-based incremental |
| hermes-agent `memory_provider.py` | MemoryProvider lifecycle (prefetch/sync_turn/on_pre_compress) |
| hermes-agent `memory_manager.py` | Fan-out, error isolation, one-external rule |
| hermes-agent `memory_tool.py` | Security scan, frozen snapshot, MEMORY.md/USER.md |
| llm-wiki-compiler | Two-phase pipeline, SHA-256 incremental |

## Arquivos nesta pasta
- `agent-memory-sota.md` — Report completo da arquitetura SOTA
- `agent-memory-plan.md` — Roadmap RM0-RM5b com acceptance criteria

## Gaps para pesquisar
- A-MEM Zettelkasten (arXiv:2502.12110) — alternativa pós-MVP
- Graphiti temporal KG integration detalhada
- Benchmark de recall latency em Rust (p50 < 500ms target)
