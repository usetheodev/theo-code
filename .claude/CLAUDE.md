# Theo Code

Governance-first autonomous code agent. Rust workspace + Tauri v2 desktop app.

## Arquitetura

Monorepo com 4 bounded contexts:

```
crates/
  theo-domain          # Tipos puros, traits, erros (zero infra)
  theo-engine-graph    # Code graph via Tree-Sitter (14 linguagens)
  theo-engine-parser   # AST parser multi-linguagem, extração de símbolos
  theo-engine-retrieval # Semantic search, embeddings, TF-IDF, graph attention
  theo-governance      # Policy engine, impact analysis, métricas
  theo-agent-runtime   # Agent loop async, decision control plane
  theo-infra-llm       # LLM client (OpenAI-compatible, Anthropic, vLLM)
  theo-infra-auth      # OAuth PKCE, device flow, token management
  theo-tooling         # Tool registry: bash, edit, grep, glob, LSP, webfetch, etc.
  theo-api-contracts   # DTOs e eventos para surfaces
  theo-application     # Camada de casos de uso

apps/
  theo-cli             # CLI binary
  theo-desktop         # Tauri v2 (Rust backend + React frontend)
  theo-ui              # React 18 + TypeScript + Tailwind + Radix UI
  theo-benchmark       # Benchmark runner (isolado)
```

## Build & Test

```bash
# Build workspace
cargo build

# Rodar todos os testes
cargo test

# Rodar testes de um crate específico
cargo test -p theo-engine-graph

# Desktop app (dev)
cd apps/theo-desktop && cargo tauri dev

# Frontend isolado
cd apps/theo-ui && npm run dev
```

## Convenções

- **Linguagem do código**: Inglês (variáveis, funções, tipos, comentários técnicos)
- **Comunicação**: Português Brasil
- **Rust edition**: 2024
- **Testes**: Obrigatórios para lógica de negócio. Padrão Arrange-Act-Assert.
- **Erros**: Tipados com `thiserror`. Nunca engolir erros silenciosamente.
- **Dependências workspace**: Declarar em `[workspace.dependencies]` no root `Cargo.toml`
- **Imports**: Usar `theo-domain` como fonte de tipos compartilhados

## Regras Arquiteturais

- `theo-domain` NÃO depende de nenhum outro crate (tipos puros)
- Apps NUNCA importam engines diretamente — falam com `theo-application`
- Governance é obrigatória no caminho crítico, não pós-processo opcional
- Todo tool call passa pelo Decision Control Plane antes de execução
- State Machine governa transições de fase: LOCATE → EDIT → VERIFY → DONE

## REGRA #0 — Meeting Obrigatoria (Gate Inquebravel)

TODA alteracao no sistema DEVE ser precedida de `/meeting`. Sem excecoes.

- Feature nova → `/meeting` primeiro
- Bug fix → `/meeting` primeiro
- Refatoracao → `/meeting` primeiro
- Mudanca de dependencia → `/meeting` primeiro

**O hook `meeting-gate.sh` BLOQUEIA Edit/Write** ate que `/meeting` produza APPROVED.

Se o veredito for REJECTED → revise a proposta e rode `/meeting` novamente.
Se nao houver meeting → nenhum arquivo do projeto pode ser alterado.

"E so uma mudanca pequena" NAO e excecao. "Ja sei o que fazer" NAO e excecao.

## Agent Core: GRAPHCTX + State Machine + Context Loops

Três mecanismos não-negociáveis:
1. **GRAPHCTX** — dá ao modelo alvos de arquivo corretos (o contexto)
2. **State Machine** — bloqueia done() até git diff mostrar mudanças reais (promise gate)
3. **Context Loops** — a cada N iterações resume o que foi feito/falhou/próximos passos

## Sistema Multi-Agente

Time de agentes especializados disponíveis em `.claude/agents/`:

| Agente | Papel | Model | Quando usar |
|---|---|---|---|
| `governance` | Principal Engineer | opus | Toda mudança significativa — veto absoluto |
| `runtime` | Staff AI Engineer | sonnet | Mudanças no agent loop, state machine, async |
| `graphctx` | Compiler Engineer | sonnet | Mudanças em parsers, graph, dependências |
| `qa` | QA Staff Engineer | sonnet | Validação de testes e cobertura |
| `tooling` | Systems Engineer | haiku | Segurança de tool execution |
| `infra` | SRE | haiku | Performance, resiliência, custo |
| `frontend` | UX Engineer | sonnet | Interface, microinterações, feedback visual |

### Skills de Decisão

- `/review-council` — Reunião FAANG completa: convoca agentes, debate com conflito obrigatório, decisão final
- `/consensus` — Decisão rápida: Governance + QA + Runtime em paralelo, sem debate

### Regra de Consenso (Consensus Engine)

```
SE Governance = REJECT → REJECT (veto absoluto)
SE QA.validated = false → REJECT (sem prova, sem aprovação)
SE Runtime.risk_level = CRITICAL → REJECT
SENÃO → APPROVE
```

### Skills de Desenvolvimento

- `/build [crate|ui|desktop|check]` — Build com diagnóstico
- `/test [crate|nome|changed]` — Testes com análise de falhas
- `/add-crate theo-xxx "desc"` — Scaffolding de crate
- `/agent-check` — Health check completo
- `/changelog [N]` — Atualiza CHANGELOG.md

## Diretórios Importantes

- `docs/current/` — Documentação do que ESTÁ implementado
- `docs/target/` — Documentação do que é planejado (futuro)
- `docs/adr/` — Architecture Decision Records
- `docs/roadmap/` — Roadmap do produto
- `research/` — Papers, experimentos, referências (isolado do runtime)
