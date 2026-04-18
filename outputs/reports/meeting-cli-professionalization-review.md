---
type: report
question: "O plano cli-professionalization.md esta alinhado com SOTA de AI coding CLIs (Claude Code, Aider, Cursor CLI)?"
generated_at: 2026-04-11T00:00:00Z
confidence: 0.88
sources_used: 2
meeting: cli-professionalization-approval
---

# Meeting Review: CLI Professionalization Plan

## POSICAO: APPROVE (com concerns menores)

## ANALISE

O plano se alinha fortemente com o estado da arte de AI coding CLIs. As tres escolhas arquiteturais sao defendaveis pela literatura: (1) crossterm sozinho sem ratatui e a decisao correta para streaming append-only — ratatui pressupoe immediate-mode full-screen redraw incompativel com token streaming (ver gitui, secao 6.3 do report); Claude Code e Aider tambem nao usam TUI full-screen. (2) syntect e o padrao de fato (bat, delta, xi) e a unica solucao madura que aceita grammars Sublime + 24-bit ANSI. (3) pulldown-cmark incremental e necessariamente custom — nenhum crate existente resolve streaming markdown (gap #1 do report). O plano cobre gaps reais do baseline atual (35+ raw ANSI, sem TTY detection, sem syntax highlighting, sem permission UI) que sao exatamente os eixos em que Claude Code se diferencia. Sequenciamento Foundation→Render→Commands→Permission→Polish e pragmatico e respeita TDD.

## RISCOS

- **Streaming markdown parser custom**: unica peca sem referencia pronta; alto risco de edge cases (fences aninhados, code interrompido). Property tests (T1.4) mitigam mas nao eliminam.
- **syntect debug-mode lentidao**: feature `default-fancy` ainda penaliza debug builds; pode degradar DX do time durante desenvolvimento iterativo.
- **16+ slash commands em 2 sprints** (Fase 2): escopo ambicioso; risco de comandos implementados sem polish real.
- **Gap vs Claude Code nao endereçado**: plano nao menciona ferramentas como `/resume` com session history persistente, nem thinking/reasoning blocks visuais distintos (Claude Code renderiza reasoning em cinza separado).
- **Ausencia de benchmark de TTFT**: metrica baseline listada como "X ms" — sem numero, impossivel provar "sem regressao".

## RECOMENDACOES

- **Medir TTFT baseline AGORA** (antes de T0.1) e gravar em `outputs/insights/` — sem isso o gate da Fase 1 e inverificavel.
- **Adicionar ADR explicito** "Streaming Markdown State Machine" detalhando estados (Idle, InBold, InCodeFence, InList) antes de T1.4 — o risco #1 merece design doc.
- **Split Fase 2 em 2a (core: /model, /cost, /clear, /compact, /doctor) e 2b (extras: /review, /bug, /export)** para evitar overcommit.
- **Adicionar T1.4.5**: renderizar `ReasoningDelta` em estilo distinto (dim italic) — paridade com Claude Code/o1.
- **Escolher `default-onig` como dev feature** de syntect via `[features]` do theo-cli para nao sofrer penalty em debug, mantendo `default-fancy` em release.
- **Reforcar T0.3 no CI**: adicionar job que falha se grep encontrar `\x1b[` fora de `style.rs` — e o unico criterio mensuravel de "renderizacao profissional" no plano.
- **Confirmar com /meeting que /bug e /export sao MVP ou cortar para Out of Scope** — nao sao diferenciadores vs SOTA.

## Sources

- /home/paulo/Projetos/usetheo/theo-code/docs/roadmap/cli-professionalization.md
- /home/paulo/Projetos/usetheo/theo-code/outputs/reports/rust-terminal-ecosystem.md
