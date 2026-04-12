---
id: 20260411-103954
date: 2026-04-11
topic: "Aprovacao do plano de profissionalizacao do theo-cli"
verdict: REVISED
participants: 16
---

# Reuniao: Aprovacao do plano de profissionalizacao do theo-cli

## Pauta

**Contexto**: Apos pesquisa profunda (crossterm, syntect, pulldown-cmark, dialoguer, indicatif) e analise do estado atual do `apps/theo-cli` (35+ ANSI hardcoded, 0 integration tests, 7 slash commands), foi produzido o plano `docs/roadmap/cli-professionalization.md` com 27 microtasks em 5 fases. A reuniao decide se o plano avanca para implementacao.

**Questoes a decidir**:
1. Aprovar o plano em bloco ou faseadamente?
2. O escopo (27 tasks, 7 deps novas) e realista?
3. Ha violacoes arquiteturais ou duplicacao de conceitos (extend not duplicate)?
4. O plano TDD e viavel?
5. Quais gates automatizados sao mandatorios?

**Restricoes conhecidas**: Meeting gate obrigatorio entre fases; vast.ai para builds; baseline a preservar (TTFT, testes verdes).

---

## Posicoes por Agente

### Estrategia
| Agente | Posicao | Resumo |
|--------|---------|--------|
| chief-architect | CONCERN | Escopo monolitico; aprovar apenas F0+F1 agora; faltam baseline TTFT, audit de licencas, ADR rejeitando ratatui; T4.5 (XDG) deve ir para F0 |
| evolution-agent | APPROVE | Elimina debito real; sinergia com theo-desktop; sugere extrair `render/` e `input/` para crate `theo-tui` reusavel; adicionar metricas de binary size |

### Conhecimento
| Agente | Posicao | Resumo |
|--------|---------|--------|
| knowledge-compiler | APPROVE | Requer ADRs dedicados (Streaming Markdown, Reject Ratatui, XDG, Permission Persistence); adicionar T4.7 para gerar proposals/ de cli-rendering e cli-commands-registry |
| ontology-manager | APPROVE com CONCERN menor | `session.acl` (T3.1) DEVE usar `theo-domain::PermissionRule`, nao tipo novo; `TheoConfig.permission_mode` deve referenciar `PermissionAction` do dominio; verificar se ja existe abstracao de comando em theo-api-contracts |
| data-ingestor | CONCERN | `/export`, `/bug`, `@file` sem frontmatter/checksum — quebra source traceability; `/import` sem validacao de schema; exigir `schema_version`, `ingested_at`, `checksum` nos artefatos |
| wiki-expert | APPROVE com CONCERN | Sem duplicacao de logica (HTML vs ANSI sao targets diferentes), mas risco de drift de opcoes pulldown-cmark entre CLI e marklive; extrair `MarkdownOptions::default_wiki()` para theo-domain |

### Qualidade
| Agente | Posicao | Resumo |
|--------|---------|--------|
| validator | CONCERN | Snapshot tests fragil a upgrade de crossterm/syntect; "comparacao manual" em T1.1 nao e DoD aceitavel; latencia < 1ms deve ir para benchmark dedicado (criterion), nao teste unitario; T3.2 sem testes automatizados |
| linter | CONCERN | Sequenciamento parallel em F0 cria estado mestico; serializar T0.1→T0.3→T0.2→T0.4; criar `render/style.rs` como stub antes de editar `renderer.rs`; snapshot baseline antes da migracao |
| retrieval-engineer | APPROVE | Impacto minimo no retrieval; exigir `max_bytes_per_mention` (64KB) em T2.6; ADR sobre uso de filesystem vs graph no completion |
| memory-synthesizer | APPROVE com CONCERN | `/compact` em T2.2 bifurca semantica de compaction aprovada na meeting 20260409 (EpisodeSummary); deve ser thin wrapper sobre `agent-runtime::compact_session()`; alinhar `session_max_messages` com constante existente |

### Engineering
| Agente | Posicao | Resumo |
|--------|---------|--------|
| code-reviewer | APPROVE (com condicoes) | SRP respeitado; adicionar T0.0 para eliminar 87+ unwrap/expect em renderer.rs/init.rs antes dos snapshots; dividir T1.4 em T1.4a (stateful buffer) + T1.4b (proptest+benchmark); F2 precisa 3 sprints ou split T2.2 |
| graphctx-expert | APPROVE com CONCERNS | Spinner em T1.8 deve emitir progresso estruturado (n files, langs, nodes, edges); completer deveria usar symbol table do graph em T2.3 para `@Symbol`; `/doctor` DEVE checar graph health |
| arch-validator | APPROVE | Boundary rules respeitadas; importacao direta de theo-infra-auth/llm no CLI e excecao documentavel (adapters de fronteira); criar ADR para essa excecao; grep enforcement no CI desde T0.3 |
| test-runner | APPROVE | TDD_PLAN_VIABLE: YES; mockar `crossterm::terminal::size()`; CI com `NO_COLOR=1` + `FORCE_TTY=0`; proptest no T1.4 desde day 1; THEO_AUTO_ACCEPT=1 para CI |
| frontend-dev | APPROVE | Sinergia real com theo-desktop; extrair color tokens de `render/style.rs` como fonte de verdade para Tailwind; StreamingMarkdownRenderer sera referencia para `<StreamingMessage>` React; Permission prompts mapeam 1:1 para `<PermissionDialog>` Radix |

### Pesquisa
| Agente | Posicao | Resumo |
|--------|---------|--------|
| research-agent | APPROVE (concerns menores) | Decisoes defendaveis pela literatura (crossterm sem ratatui, syntect, streaming custom); gap vs Claude Code: reasoning blocks visuais distintos e `/resume` persistente; metrica TTFT "X ms" e inverificavel — medir antes de T0.1; `default-onig` em debug, `default-fancy` em release |

---

## Conflitos e Resolucoes

### Conflito 1: Aprovacao em bloco vs faseada
- **chief-architect**: aprovar apenas F0+F1 agora
- **evolution-agent + outros**: aprovar plano completo
- **Resolucao**: APROVAR o plano como documento vivo, mas **commit de execucao apenas para F0+F1**. Fases 2-4 reavaliadas apos dogfood da F1.

### Conflito 2: Escopo de F2 (9 comandos novos)
- **code-reviewer**: 3 sprints ou split T2.2
- **research-agent**: cortar /bug e /export do MVP
- **chief-architect**: subset `/model /cost /clear /compact /doctor`
- **Resolucao**: Dividir T2.2 em T2.2a (core: `/model /cost /clear /compact /doctor`) e T2.2b (extras: `/init /review /bug /export`). T2.2b condicional a reavaliacao.

### Conflito 3: session.acl vs PermissionRule
- **ontology-manager**: DEVE usar `theo-domain::PermissionRule`
- **arch-validator**: permission gate integra com `theo-governance::PolicyEngine`
- **Resolucao**: Nao ha conflito real — T3.1 deve consumir ambos. `session.acl` = `Vec<PermissionRule>` de dominio, fluxo passa por PolicyEngine.

### Conflito 4: `/compact` local vs EpisodeSummary
- **memory-synthesizer**: bifurca semantica ja aprovada
- **Resolucao**: `/compact` sera thin wrapper sobre `agent-runtime::compact_session()` (a criar/existente). Bloqueia T2.2a ate confirmar API disponivel.

### Conflito 5: Testes de latencia
- **validator**: fora de testes unitarios
- **test-runner**: viavel com mock
- **Resolucao**: Criterios de latencia (< 1ms chunk, < 50ms load) vao para `criterion` benchmarks, nao `cargo test`. Unit tests validam apenas correctness.

### Consenso Unanime
- Crossterm sozinho (NAO Ratatui) — decisao correta
- TDD obrigatorio com snapshot + property tests
- Zero raw ANSI fora de `render/style.rs` — gate automatizado
- ADRs dedicados sao mandatorios antes da F1

---

## Decisoes

1. **VEREDITO REVISED**: Plano aprovado com modificacoes obrigatorias (ver action items abaixo).
2. **Escopo inicial**: executar apenas Fase 0 + Fase 1. Fases 2-4 reavaliadas em nova `/meeting` apos F1.
3. **T0.0 (NOVO) obrigatorio**: antes de qualquer codigo, medir baseline (TTFT, binary size, cold-start, unwrap count) e commitar.
4. **Sequenciamento F0 serializado**: T0.1 → T0.3 → T0.2 → T0.4 (nao paralelo).
5. **Quatro ADRs obrigatorios antes de T1.1**:
   - ADR: Streaming Markdown State Machine
   - ADR: Reject Ratatui
   - ADR: XDG Paths (promovido de T4.5 para F0)
   - ADR: Apps importando infra-auth/infra-llm como excecao documentada
6. **T1.4 dividido**: T1.4a (buffer stateful) e T1.4b (property tests + benchmark criterion).
7. **T2.2 dividido**: T2.2a (core 5 comandos) e T2.2b (extras 4 comandos, condicional).
8. **CI gate `grep ANSI`**: habilitado em T0.3, bloqueia merge.
9. **session.acl = `Vec<PermissionRule>` de theo-domain** (extend, not duplicate).
10. **`/compact` = wrapper sobre agent-runtime**, nao logica LLM inline.
11. **MarkdownOptions compartilhado** entre CLI (`render/markdown.rs`) e marklive via theo-domain ou theo-api-contracts.
12. **Data traceability**: `/export`, `/bug`, `@file` produzem artefatos com frontmatter `{schema_version, source, ingested_at, checksum}`.
13. **Config versioning**: `TheoConfig.config_version: u32` desde T0.4.
14. **Benchmarks via criterion**: latencia removida de unit tests.
15. **TheoCapability shared color tokens**: extrair tabela de cores de `render/style.rs` para `docs/current/color-tokens.md` (fonte de verdade para Tailwind do desktop).
16. **GRAPHCTX integration**: T1.8 spinner com progresso estruturado; T2.3 symbol completion via graph; `/doctor` checa graph health.

---

## Action Items

- [ ] **chief-architect** — criar 4 ADRs (Streaming Markdown, Reject Ratatui, XDG, Infra Exception) em `docs/adr/` — **antes de T0.1**
- [ ] **code-reviewer** — executar T0.0: medir baseline TTFT, binary size, contar unwrap/expect, commitar em `docs/current/cli-baseline.md` — **antes de T0.1**
- [ ] **arch-validator** — validar que T0.1 `cargo tree` nao introduz conflitos e todas as 7 deps tem licenca MIT/Apache-2.0 — **gate de T0.1**
- [ ] **ontology-manager** — auditar `theo-domain::permission` e `theo-api-contracts` para garantir que T3.1 usa tipos existentes — **antes de F3**
- [ ] **memory-synthesizer** — verificar se `agent-runtime::compact_session()` existe ou precisa ser criada em paralelo a T2.2a — **antes de T2.2a**
- [ ] **data-ingestor** — definir schema JSON do `/bug` e frontmatter do `/export` em ADR separado — **antes de T2.2b**
- [ ] **wiki-expert** — extrair `MarkdownOptions::default_wiki()` para crate compartilhada — **T1.2 gate**
- [ ] **validator** — setup `insta` com review workflow documentado; definir quais mudancas de snapshot sao regressoes vs atualizacoes — **T1.1 gate**
- [ ] **test-runner** — setup criterion benchmarks para T1.4b e `syntect` load; CI runner com `NO_COLOR=1 FORCE_TTY=0` — **T1.3 gate**
- [ ] **linter** — criar `render/style.rs` como modulo stub antes de editar `renderer.rs`; snapshot dos 23 testes atuais como baseline — **inicio de T1.1**
- [ ] **graphctx-expert** — expor canal de progresso em `theo-engine-graph` (callback ou channel) para T1.8 — **antes de T1.8**
- [ ] **retrieval-engineer** — especificar `max_bytes_per_mention=64KB` e estrategia de truncation em T2.6 — **antes de T2.6**
- [ ] **frontend-dev** — criar `docs/current/color-tokens.md` apos T0.3 com tabela de tokens compartilhaveis com Tailwind — **apos T0.3**
- [ ] **knowledge-compiler** — gerar `proposals/new_pages/cli-rendering.md` apos F1 completar — **apos F1**
- [ ] **research-agent** — documentar gap vs Claude Code (reasoning deltas visuais distintos, /resume persistente) em `outputs/reports/cli-sota-gaps.md` — **antes de F1**
- [ ] **evolution-agent** — avaliar extracao futura de `render/` + `input/` para crate `theo-tui` apos F1 concluir — **post-F1**

---

## Plano TDD (por task da F0 e F1)

### T0.0 — Baseline measurement
1. **RED**: Criar script `scripts/measure-baseline.sh` que falha se arquivo `docs/current/cli-baseline.md` nao existe
2. **GREEN**: Script mede TTFT via `hyperfine`, binary size via `ls -la`, unwrap count via grep, commita em cli-baseline.md
3. **REFACTOR**: Extrair em Makefile target `make baseline`
4. **VERIFY**: `make baseline && test -f docs/current/cli-baseline.md`

### T0.1 — Dependencies
1. **RED**: Teste `cargo tree | grep syntect` retorna vazio
2. **GREEN**: Adicionar deps em workspace Cargo.toml, `cargo build`
3. **REFACTOR**: Validar licencas via `cargo deny check licenses`
4. **VERIFY**: `cargo build -p theo && cargo deny check` no vast.ai

### T0.2 — TtyCaps + resize
1. **RED**: Escrever `tests/tty_detection.rs` com 4 casos (tty, no-tty, NO_COLOR, resize)
2. **GREEN**: Implementar `TtyCaps::detect()` usando `console::Term` + `crossterm::terminal::size`
3. **REFACTOR**: Mover detection para OnceLock, resize listener em tokio task
4. **VERIFY**: `cargo test -p theo tty::`

### T0.3 — render/style.rs
1. **RED**: Snapshot test com `insta` para cada style (success, error, warn, dim, accent, tool_name, code_bg)
2. **GREEN**: Implementar funcoes usando `crossterm::style`
3. **REFACTOR**: Macros `theo_print!`, `theo_println!` com queue!+flush
4. **VERIFY**: `cargo test -p theo render::style::` + CI grep gate

### T0.4 — TheoConfig
1. **RED**: Testes para load default, load valido, corrupto, env override, config_version migration
2. **GREEN**: Implementar struct + serde + dirs::config_dir
3. **REFACTOR**: Extrair loader em trait testavel
4. **VERIFY**: `cargo test -p theo config::`

### T1.1 — Migrar renderer.rs
1. **RED**: Snapshot de output atual de cada tool render (baseline golden files)
2. **GREEN**: Substituir ANSI por calls em render/style.rs, arquivo-por-arquivo
3. **REFACTOR**: Mover logica per-tool para render/tool_result.rs
4. **VERIFY**: `cargo test -p theo` + `grep -rn "\\x1b\[" apps/theo-cli/src/ | grep -v style.rs` vazio

### T1.2 — render/markdown.rs
1. **RED**: Snapshot tests para headers, bold, italic, code, lists, blockquote, hr, links, tables
2. **GREEN**: Parser pulldown-cmark + emit via crossterm style
3. **REFACTOR**: Separar render_inline, render_block, render_table
4. **VERIFY**: 12+ snapshots verdes

### T1.3 — render/code_block.rs
1. **RED**: Teste para 12 linguagens (rust, python, js, ts, go, java, bash, json, yaml, toml, html, css, sql) retorna output ANSI diferente de plain
2. **GREEN**: Carregar SyntaxSet + ThemeSet em OnceLock, implementar highlight
3. **REFACTOR**: Box border + language label
4. **VERIFY**: 12+ tests + `criterion bench_syntect_load < 50ms`

### T1.4a — StreamingMarkdownRenderer (buffer)
1. **RED**: 10 unit tests cobrindo: inline bold complete, code block fence complete, interrupted state, newline flush, reset on idle
2. **GREEN**: Estado com buffer + detector de fences
3. **REFACTOR**: Extrair state machine explicita
4. **VERIFY**: `cargo test -p theo render::streaming::`

### T1.4b — Property tests + benchmark
1. **RED**: `proptest!` que qualquer input produz output valido (nao corrompido)
2. **GREEN**: Corrigir edge cases detectados
3. **REFACTOR**: Integrar ao CriterionGroup
4. **VERIFY**: `cargo bench --bench streaming_markdown` com threshold < 1ms/chunk

### T1.5 — Integracao ContentDelta
1. **RED**: Integration test em `apps/theo-cli/tests/` — mock agent stream → terminal assert
2. **GREEN**: CliRenderer delega ContentDelta para StreamingMarkdownRenderer
3. **REFACTOR**: Flush em RunStateChanged::Idle
4. **VERIFY**: `cargo test -p theo --test streaming_integration`

### T1.6, T1.7, T1.8 — Similar pattern (RED-GREEN-REFACTOR-VERIFY)

---

## Veredito Final

**REVISED**: O plano e aprovado em principio com 16 modificacoes obrigatorias listadas em "Decisoes". Escopo de execucao imediata reduzido para **Fase 0 + Fase 1**. Fases 2-4 aguardam nova `/meeting` apos dogfood da F1. Quatro ADRs e T0.0 (baseline measurement) sao pre-requisitos bloqueantes para T0.1.

**Tally final**: 11 APPROVE (varios com concerns menores), 4 CONCERN, 0 REJECT, 0 ABSTAIN.

**Proximo passo autorizado**: criacao dos 4 ADRs + T0.0 (baseline measurement). Inicio de T0.1 apenas apos ADRs commitados.
