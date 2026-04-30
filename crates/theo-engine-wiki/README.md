# theo-engine-wiki

**Wiki compilada por LLM para humanos entenderem codebases.**

Ler código é moroso e complicado. Onboarding num projeto de 15 crates leva
semanas. O Theo Wiki resolve isso: o LLM compila o codebase inteiro numa
wiki navegável que um humano consegue ler em horas, não semanas.

## O Contrato

```
HUMANO     = LEITOR   → lê, navega, consulta. Nunca escreve.
WIKI AGENT = ESCRITOR → sub-agente background, ativado por triggers.
                        Único escritor. Wiki se mantém sozinho.
MANUAL     = OPCIONAL → `theo wiki generate` força rebuild. Raro.
```

## Arquitetura: Skeleton + Enrichment

```
Código fonte
    │
    ├─ Tree-Sitter Parse (grátis)
    │   └─ SKELETON: arquivos, símbolos, APIs, dependências
    │
    └─ LLM Enrichment (Wiki Agent, background)
        └─ WIKI PAGES: "o que faz", "por quê", "como funciona", "o que quebra"
```

- **Skeleton** = inventário estrutural extraído do código (sem LLM, sem custo)
- **Enrichment** = entendimento compilado pelo LLM (custo: Haiku, ~$0.15/update)
- O skeleton já existe via `theo-engine-graph`. O enrichment é o diferencial.

## Módulos

| Módulo | O que faz |
|--------|-----------|
| `page.rs` | WikiPage struct: skeleton + enrichment, staleness tracking |
| `skeleton.rs` | Extrai dados estruturais do code graph (free) |
| `store.rs` | Persistência em JSON, atomic write (temp+rename) |
| `hash.rs` | SHA-256 incremental — unchanged files = zero LLM calls |
| `lint.rs` | 6 regras: missing enrichment, stale, broken links, orphans, empty sections |
| `error.rs` | Typed errors (WikiError), nunca strings genéricas |

## Dependências

```
theo-engine-wiki
  → theo-domain        (WikiBackend trait, tipos puros)
  → theo-engine-graph  (code graph para skeleton)
  → theo-engine-parser (Tree-Sitter para symbol extraction)
```

Mesma camada que `theo-engine-retrieval`. Nunca depende de application ou runtime.

## O que este crate NÃO faz

- **NÃO chama LLM** — o crate fornece a engine. O Wiki Agent (em `theo-agent-runtime`)
  é quem chama o LLM para enrichment.
- **NÃO é para o agente** — o agente lê código diretamente. O wiki é para HUMANOS.
- **NÃO é documentação estática** — é mantido vivo pelo Wiki Agent via triggers.
