# Model Routing — Pesquisa SOTA

## Escopo
Smart model routing per role (Normal/Compact/Vision/Subagent/Compaction/Reviewer), orchestrator-worker pattern, cost optimization, cascade fallback, cross-provider routing.

## Crates alvo
- `theo-domain` — ModelRouter trait, RoutingContext, ModelChoice
- `theo-infra-llm` — rule-based router, provider catalog (26 specs), model_limits.rs
- `theo-application` — routing integration with use-cases

## Referências-chave
| Fonte | O que extrair |
|-------|---------------|
| Anthropic orchestrator-worker | +90.2% improvement, Opus plans + Sonnet executes |
| Anthropic Advisor Strategy | -11% cost, +2% quality |
| FrugalGPT (arXiv:2305.05176) | LLM cascade, up to 98% cost reduction |
| RouteLLM (arXiv:2406.18665) | Preference classifier, -85% cost on MT-Bench |
| hermes smart_model_routing.py | Rule-based: char/word/newline/backtick/keyword heuristics |
| hermes auxiliary_client.py | Fallback chain: OpenRouter→Nous→custom→Codex→Anthropic |
| opendev | Named slots (Normal/Compact/Vision), cascading defaults |
| Archon | Per-node model overrides, model/provider validation at load |
| pi-mono | Model resolver, fuzzy matching, scope concept |

## Arquivos nesta pasta
- `smart-model-routing.md` — Full research report
- `smart-model-routing-plan.md` — Implementation plan

## Gaps para pesquisar
- Latency impact of cascade routing (FrugalGPT sequential problem)
- Optimal rule set for code agent routing (hermes keywords may not apply to Rust)
- Cost tracking accuracy requirements for routing decisions
- Cross-provider auth coordination
