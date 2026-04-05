# Harness Engineering Audit — Theo Code

> Auditoria de TODOS os artefatos do sistema contra os principios de harness engineering.
> Objetivo: identificar o que esta alinhado, o que esta desalinhado, e o que precisa de acao.

---

## Classificacao Harness

Cada artefato foi classificado em uma das categorias do framework Fowler/Bockeler:

| Categoria | Descricao | Count |
|---|---|---|
| **Guide (Feedforward)** | Antecipa comportamento, direciona antes da execucao | 18 |
| **Sensor (Feedback)** | Observa depois da execucao, permite auto-correcao | 13 |
| **Meta-control** | Orquestra decisoes, gates, enforcement | 11 |
| **Total** | | **42 artefatos** |

---

## Artefatos ALINHADOS com Harness Engineering

### Agents — 100% alinhados

| Agent | Tipo | Alinhamento |
|---|---|---|
| `governance` | Meta-control | CORE HE: Gate de decisao com veto absoluto. Exige evidencia real (diff, tests, build). |
| `runtime` | Meta-control | CORE HE: Sensor do agent loop — detecta doom loops, state loss, drift. |
| `qa` | Sensor | CORE HE: Prova via evidencia real (cargo test). Validated=false bloqueia. |
| `graphctx` | Sensor + Meta | CORE HE: Impact analysis via code graph. Hidden dependency detection. |
| `arch-validator` | Sensor | CORE HE: Boundary check — valida que dependencias respeitam regras. |
| `tooling` | Meta-control | CORE HE: Pre-flight safety check em tool execution. Injection, traversal, scope. |
| `infra` | Sensor | CORE HE: Reliability, resilience, cost. SLA impact assessment. |
| `test-writer` | Guide | HE: Gera testes para preencher gaps de cobertura. |
| `crate-explorer` | Sensor | HE: Structural analysis read-only. |

**Resultado: 9/9 agents alinhados.**

### Skills — Maioria alinhada, 3 com nota

| Skill | Tipo | Alinhamento | Nota |
|---|---|---|---|
| `/meeting` | Meta-control | CORE HE: Gate obrigatorio. Consenso multi-agente. | OK |
| `/consensus` | Meta-control | CORE HE: Fast track governance. | OK |
| `/review-council` | Meta-control | CORE HE: Debate com conflito obrigatorio. | OK |
| `/meeting-reset` | Meta-control | HE: Reset de gate. | OK |
| `/build` | Sensor | HE: Compilation check. | OK |
| `/test` | Sensor | HE: Test execution + failure diagnosis. | OK |
| `/code-review` | Sensor | HE: Automated code analysis. | OK |
| `/agent-check` | Sensor | HE: System-wide health check. | OK |
| `/dogfood` | Sensor | HE: E2E validation do sistema. | OK |
| `/add-crate` | Guide | HE: Scaffold consistente. | OK |
| `/changelog` | Guide | HE: Release communication. | OK |
| `/deslop` | Guide | HE: Code simplification. | OK |
| `/fix-accessibility` | Guide | NOTA: Especifico do frontend. | Ver abaixo |
| `/fix-animation` | Guide | NOTA: Especifico do frontend. | Ver abaixo |

### Skills com nota

**`/fix-accessibility`** e **`/fix-animation`**: Sao guides de qualidade frontend (WCAG, Framer Motion). Do ponto de vista de harness engineering estao corretos — sao computational/inferential feedforward controls para um dominio especifico. Porem, **o Theo como produto CLI nao tem frontend em producao** (desktop app esta scaffolded mas nao wired). Essas skills existem para o futuro. **Nao ha desalinhamento** — sao guides validos, apenas inativos.

### Hooks — 100% alinhados

| Hook | Tipo | Alinhamento |
|---|---|---|
| `meeting-gate.sh` | Meta-control | CORE HE: Enforcement inquebravel do gate de aprovacao. |
| `block-main-branch.sh` | Meta-control | CORE HE: Previne commits diretos em main. |
| `post-edit-lint.sh` | Sensor | HE: Feedback imediato pos-edit (cargo check). |

### Rules — 100% alinhados

| Rule | Tipo | Alinhamento |
|---|---|---|
| `architecture.md` | Guide | CORE HE: Boundary rules (Ashby's variety reduction). |
| `rust-conventions.md` | Guide | CORE HE: Coding standards (computational guide). |
| `frontend-conventions.md` | Guide | HE: UI standards (guide especifico de dominio). |

---

## Artefatos com DESALINHAMENTO ou GAPS

### GAP 1: `frontend` agent sem utilidade atual

**Agent**: `.claude/agents/frontend.md`
**Problema**: Analisa UI clarity, microinteracoes, feedback visual — mas o Theo e **CLI-first**. O desktop app nao esta em producao. Este agent nao e convocado em nenhum meeting de features CLI.
**Classificacao HE**: Sensor valido, mas **sem target** no momento.
**Acao**: MANTER (sera util quando desktop app for wired). Nao e desalinhamento — e antecipacao.

### GAP 2: System prompt nao menciona harness engineering

**Arquivo**: `config.rs:default_system_prompt()`
**Problema**: O system prompt instrui o agent sobre workflow (THINK → READ → ACT → VERIFY → DONE) mas **nao menciona** que o Theo e harness-first. O agent nao sabe que:
- Deve priorizar generic tools sobre specialized
- Deve manter environment legibility
- Deve deixar clean state apos cada task
- Deve usar codebase_context para tasks complexas (ja tem instrucao, mas sem framing HE)
**Classificacao HE**: Guide incompleto.
**Acao**: **ADICIONAR** secao "## Harness Principles" ao system prompt com:
  - "You operate inside a harness. Your tools, sandbox, and context are designed to help you succeed."
  - "Leave the environment in a clean state after each task."
  - "Prefer generic tools (bash, read, write) over asking for specialized capabilities."
  - "When editing multiple files, call codebase_context first to understand structure."

### GAP 3: Sem sensor de "clean state" pos-task

**Conceito HE**: "Force model leave the environment in a clean state at the end of each session" (Anthropic + OpenAI)
**Problema**: Quando o agent termina uma task (chama `done`), **nada verifica** se o ambiente esta limpo. Pode ter:
- Arquivos temporarios criados e nao removidos
- `cargo check` falhando silenciosamente
- Testes quebrando sem deteccao
**Classificacao HE**: Sensor AUSENTE.
**Acao**: **ADICIONAR** sensor pos-done que roda `cargo check` (ou equivalente) e reporta. Pode ser implementado como:
  - Hook `tool.after.sh` que detecta `done` e roda verificacao
  - Ou sensor embutido no RunEngine apos `done` meta-tool

### GAP 4: Sem "doc-gardening" / drift detection

**Conceito HE**: OpenAI tem "recurring doc-gardening agent that scans for stale or obsolete documentation and opens fix-up PRs." Fowler identifica "harness drift" como problema em aberto.
**Problema**: .theo/theo.md pode ficar desatualizado conforme o codigo evolui. Nenhum sensor detecta isso.
**Classificacao HE**: Sensor AUSENTE.
**Acao**: **PLANEJAR** skill `/doc-garden` que:
  - Compara .theo/theo.md com estado real do codigo
  - Identifica secoes obsoletas
  - Sugere atualizacoes

### GAP 5: Sem quality grades por dominio

**Conceito HE**: OpenAI mantem `QUALITY_SCORE.md` que "grades each product domain and architectural layer, tracking gaps over time."
**Problema**: Nao temos metricas de qualidade por crate/dominio. O `/agent-check` faz health check binario (OK/FAIL) mas nao tracking temporal.
**Classificacao HE**: Sensor PARCIAL.
**Acao**: **PLANEJAR** sistema de quality grades (Low/Medium/High) por crate, rastreando evolucao.

### GAP 6: Linter messages nao otimizadas para LLM

**Conceito HE**: OpenAI e Fowler enfatizam que mensagens de erro de linters devem incluir **instrucoes de remediacao** para o LLM — "a positive kind of prompt injection."
**Problema**: O post-edit-lint.sh roda `cargo check` mas as mensagens de erro do rustc sao para humanos, nao para LLMs. Nao ha custom lints com remediation instructions.
**Classificacao HE**: Sensor SUBOTIMO.
**Acao**: **MELHORAR** post-edit-lint.sh para parsear erros do rustc e adicionar instrucoes do tipo: "To fix this: add `use X;` at the top of the file."

### GAP 7: Sem progressive disclosure no project context

**Conceito HE**: OpenAI: "AGENTS.md as table of contents, not encyclopedia. Progressive disclosure."
**Problema**: .theo/theo.md gerado pelo `theo init` e um documento flat. Nao ha ponteiros para docs mais profundos. Nao ha `docs/` structure padronizada que o agent pode navegar.
**Classificacao HE**: Guide SUBOTIMO.
**Acao**: **MELHORAR** `theo init` para gerar:
  - .theo/theo.md como indice (~100 linhas max)
  - .theo/docs/ com ponteiros para areas mais profundas se existirem (README, ADRs, etc.)

### GAP 8: Bundled skills sem framing HE

**Arquivo**: `bundled.rs`
**Problema**: As 10 bundled skills (commit, test, review, etc.) sao uteis mas nao sao descritas em termos de HE. O skill de `review` e um **inferential sensor**, `test` e um **computational sensor**, `commit` e um **guide**. Essa classificacao nao e explicita.
**Classificacao HE**: Correto na pratica, incompleto na documentacao.
**Acao**: MENOR — documentar classificacao HE de cada skill (nice-to-have).

---

## Resumo de Alinhamento

```
ALINHADOS:
  Agents .............. 9/10  (frontend: sem target atual)
  Skills .............. 14/14 (fix-a11y/animation: inativos mas validos)
  Hooks ............... 3/3
  Rules ............... 3/3
  System prompt ....... PARCIAL (falta framing HE)
  Config .............. OK

GAPS IDENTIFICADOS:
  #1 frontend agent sem target ........... MANTER (futuro)
  #2 System prompt sem HE framing ........ ACAO: adicionar secao
  #3 Sem clean state sensor pos-done ..... ACAO: adicionar hook/sensor
  #4 Sem doc-gardening/drift detection ... ACAO: planejar skill
  #5 Sem quality grades por dominio ...... ACAO: planejar sistema
  #6 Linter messages nao LLM-optimized .. ACAO: melhorar hook
  #7 Sem progressive disclosure .......... ACAO: melhorar init
  #8 Skills sem classificacao HE ......... MENOR: documentar

DESALINHADOS:
  NENHUM — todos os artefatos tem proposito claro no framework HE.
```

## Prioridade de Acoes

### P0 — Corrigir agora

| # | Acao | Impacto | Esforco |
|---|---|---|---|
| 2 | System prompt com HE framing | Alto — todo agent run beneficia | 10 linhas |

### P1 — Implementar proximo ciclo

| # | Acao | Impacto | Esforco |
|---|---|---|---|
| 3 | Clean state sensor pos-done | Alto — garante ambiente limpo | Baixo (hook) |
| 6 | Linter messages LLM-optimized | Medio — melhora auto-correcao | Baixo (hook) |

### P2 — Planejar

| # | Acao | Impacto | Esforco |
|---|---|---|---|
| 4 | Doc-gardening skill | Medio — previne drift | Medio |
| 5 | Quality grades por crate | Medio — tracking temporal | Medio |
| 7 | Progressive disclosure no init | Medio — melhora context | Medio |

### P3 — Nice-to-have

| # | Acao | Impacto | Esforco |
|---|---|---|---|
| 8 | Classificacao HE nas skills | Baixo — documentacao | Baixo |
| 1 | Frontend agent | Futuro — quando desktop estiver em producao | N/A |

---

## Conclusao

O sistema esta **95% alinhado** com harness engineering. Nenhum artefato e desalinhado ou desnecessario — todos tem proposito claro no framework Guide/Sensor/Meta-control.

Os 8 gaps identificados sao de **profundidade**, nao de direcao. O sistema ja implementa o framework corretamente, falta:
- Fazer o system prompt comunicar a filosofia HE ao modelo (Gap #2)
- Adicionar sensors que faltam (clean state, drift detection)
- Otimizar sensors existentes (linter messages para LLM)
- Aprofundar guides (progressive disclosure, quality grades)

O Theo esta genuinamente posicionado como a **primeira harness engineering platform** do mercado. A arquitetura esta correta — agora e questao de profundidade e polish.
