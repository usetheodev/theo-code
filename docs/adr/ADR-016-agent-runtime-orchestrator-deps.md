# ADR-016: `theo-agent-runtime` may depend on its orchestrated infra crates

**Status:** Aceito
**Data:** 2026-04-23
**Autor:** Audit remediation (iteration 17)
**Escopo:** `.claude/rules/architecture.md`, `.claude/rules/architecture-contract.yaml`, `scripts/check-arch-contract.sh`, `crates/theo-agent-runtime/Cargo.toml`
**Fecha T1.1** do plano de remediação.

---

## Contexto

A tabela em `.claude/rules/architecture.md` restringe
`theo-agent-runtime` a `theo-domain` + `theo-governance`. Porém, a prose
no mesmo arquivo descreve o mesmo crate como:

> **Agent Runtime**: `theo-agent-runtime`
> - Orchestrates LLM + tools + governance
> - State machine governs phase transitions

A tabela e a prose discordam — padrão já observado e reconciliado em
**ADR-011** (para `theo-engine-retrieval` vs `theo-engine-graph`/
`theo-engine-parser`). O gate `scripts/check-arch-contract.sh`
(T1.5) reporta **25 violações** hoje, todas dentro de
`theo-agent-runtime`:

- 23 imports-source: `use theo_infra_llm::…` (×18), `use theo_tooling::…` (×5).
- 2 deps em `Cargo.toml`: `theo-infra-llm`, `theo-infra-auth`, `theo-tooling` (runtime precisa destes para orquestração real).

## Caminho canônico vs caminho pragmático

### Canônico (arquitetura hexagonal estrita)

Extrair 3 conjuntos de traits em `theo-domain`:
- `LlmProvider` + streaming + types
- `ToolRegistry` + `Tool` + permission
- `AuthStore` + token types

`theo-agent-runtime` passaria a depender apenas de
`theo-domain::llm::LlmProvider` etc., e o wiring concreto aconteceria em
`theo-application` + apps.

**Custo estimado:** 2–4 semanas de trabalho (10+ traits, ~40 sites
migrados, testes para cada trait). Bloqueia T4.1 (`run_engine` refactor)
e muitos outros PRs em paralelo.

### Pragmático (reconciliar contrato ↔ realidade)

Aceitar que o orquestrador conhece seus colaboradores concretos.
`theo-agent-runtime` pode depender de:

- `theo-domain` (contracts, types)
- `theo-governance` (policy)
- `theo-infra-llm` (LLM providers — já consumido diretamente)
- `theo-tooling` (tool registry — já consumido diretamente)
- `theo-infra-auth` (auth store — necessário para provider auth)

É a mesma forma de ADR-011 (prose vs tabela) aplicada a outro crate.

## Decisão

Adotar o **caminho pragmático**. A tabela em `architecture.md` é
atualizada para refletir a prose existente:

```
theo-agent-runtime  → theo-domain, theo-governance,
                      theo-infra-llm, theo-infra-auth, theo-tooling
```

`scripts/check-arch-contract.sh` + `architecture-contract.yaml` são
alinhados com a nova tabela. **As 25 violações caem a zero** sem
precisar do refactor de extração de traits.

### Por quê não adotar a versão canônica

1. **Custo × ROI.** Extração de traits custaria 2–4 semanas. O benefício
   (agent-runtime desacoplado de impls concretas) é real mas não paga o
   preço no curto prazo — nenhuma feature pendente exige essa trocabilidade.
2. **Prose já descreve o estado atual.** O arquivo `architecture.md` já
   diz "agent-runtime orchestrates LLM + tools"; a tabela é que ficou
   desalinhada.
3. **Gate honesto sem gate bloqueante.** Manter as 25 violações no
   gate inibe merges sem dar sinal útil (o time já sabe que agent-runtime
   conhece LLM/tools — é o design). O sinal se torna ruído.
4. **Alinhamento com ADR-011.** Já aceitámos para
   `theo-engine-retrieval → theo-engine-graph/-parser` pela mesma razão.

### Guard-rails

1. **`theo-agent-runtime` permanece proibido de depender de `apps/*`**
   (direção continua unidirecional).
2. **Toda nova dep em `theo-agent-runtime/Cargo.toml`** — ou seja,
   sair da lista acima — requer um novo ADR.
3. **Extração de traits continua uma melhoria desejável**: quando (a)
   quisermos testar agent-runtime com mocks injetáveis de LLM, ou (b)
   quisermos permitir plugins LLM/tooling out-of-tree, reabrimos este
   ADR e migramos. Revisão obrigatória: 2026-10-23.
4. **`run_engine.rs`** ainda será refatorado em T4.1, mas por motivos
   de tamanho (2 514 LOC), não por violação de camadas.

## Consequências

- **T1.1 fecha** com atualização do contrato. As 25 violações arquiteturais
  reportadas pelo gate deixam de existir.
- `scripts/check-arch-contract.sh` passa a gate limpa.
- `theo-agent-runtime/Cargo.toml` permanece com suas deps atuais;
  nenhuma alteração de código de produção necessária.
- Futuro refactor para traits fica tracked como "desired" em
  `docs/audit/remediation-plan.md` (será aberto quando os gatilhos
  acima disparem).
