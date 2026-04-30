# Prompt Engineering — Pesquisa SOTA

## Escopo
System prompt structure, tool schema design, structured NL representation, progressive disclosure, few-shot examples, error message quality, role-specific prompts, anti-hallucination.

## Crates alvo
- `theo-agent-runtime` — system prompts, role definitions
- `theo-tooling` — tool schemas, descriptions, error surfaces

## Referências-chave
| Fonte | O que extrair |
|-------|---------------|
| Tsinghua representation | +16.8 SWE-Bench com structured NL |
| opendev BaseTool | Validation/normalization/sanitization pipeline |
| hermes-agent tools | 58+ tools com registry central, parallel-safe classification |
| pi-mono | TypeBox schema validation, 7 tools built-in |
| rippletide | Hallucination detection in agent evaluation |
| Anthropic Claude 4 prompting guide | Multi-context window best practices |
| OpenAI harness | Specialized prompts per role (review vs implement) |
| superpowers | Mandatory workflows, auto-triggering skills |
| GSD | XML-structured plans optimized for Claude |

## Arquivos nesta pasta
- (pesquisas sobre prompt engineering vão aqui)

## Gaps para pesquisar
- Optimal tool count per session (Anthropic recommends ≤ 20, Theo has 72)
- Progressive disclosure: which tools to show when?
- Representation format: XML vs Markdown vs JSON for agent plans?
- Anti-hallucination: measurable reduction techniques
- Tool schema quality: validation at registration vs runtime
