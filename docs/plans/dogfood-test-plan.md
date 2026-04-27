# Dogfood Test Plan — Validação End-to-End do Sistema Theo Code

**Data:** 2026-04-27
**Owner execução:** Claude Code (autônomo onde possível)
**Owner observação:** paulohenriquevn (handoff em pontos com input humano)
**Sessão LLM:** OAuth Codex (`gpt-5.4`) — token válido até 2026-05-02 (`~/.config/theo/auth.json`)
**Custo orçado:** R$ 0 (OAuth Codex já provisionado)
**Relatório de saída:** `docs/audit/dogfood-2026-04-27.md`

---

## 1. Objetivo

Validar empiricamente que o sistema Theo Code está **totalmente funcional** — cada feature documentada conecta, responde, e produz resultado de qualidade. Não confiar em CLAUDE.md: verificar.

A "Honest System State" do CLAUDE.md (verified 2026-04-27) declara:
- 5238 testes passando
- 59 tools, 17 CLI subcommands, 26 LLM providers, 16 linguagens parser
- Smoke bench: 18/20 (90 %, CI [82.4 %, 100 %])
- 4 famílias sidecar (LSP/DAP/Browser/Computer Use) wired mas **não validadas E2E**
- Débito: 105 unwrap, 66 unsafe sem `// SAFETY:`, 17 god-files

Cada uma dessas afirmações vira uma assertion testável neste plano.

## 2. Princípios

- **Verificar, não confiar.** Cada gate roda na execução; números antigos são apenas referência.
- **Erros falham alto.** Qualquer regressão (ex: 5237 testes passando, 58 tools registrados) é finding obrigatório, não nota de rodapé.
- **Sem custo extra.** Reaproveitar OAuth Codex e binário já compilado quando possível.
- **Honestidade extrema.** Sidecars que não puderem ser validados são marcados SKIPPED com motivo, nunca PASS por omissão.

## 3. Estrutura — 3 Tiers

```
Tier 1  ── Gates automáticos     (~15 min, $0)
Tier 2  ── Dogfood real com LLM  (~30 min, $0 via OAuth Codex)
Tier 3  ── Sidecars              (~60-120 min, depende de instalação)
```

Cada tier produz um bloco no relatório final. Falha em Tier N **não bloqueia** Tier N+1; documenta-se e continua.

---

## 4. Tier 1 — Gates Automáticos

Objetivo: assegurar que os números do "Honest System State" continuam verdadeiros.

### T1.1 — Build do workspace
- Comando: `cargo build --workspace --all-targets`
- Aceitação: exit 0, zero warnings novos
- Coleta: tempo total, tamanho de `target/debug`

### T1.2 — Test suite completa
- Comando: `cargo test --workspace --exclude theo-code-desktop --lib --tests`
- Aceitação: **5238 testes passando** (delta ±5 aceitável só com justificativa)
- Coleta: contagem real, tempo wall-clock

### T1.3 — Architecture contract
- Comando: `bash scripts/check-arch-contract.sh`
- Aceitação: exit 0, zero violações do contrato em CLAUDE.md §Dependency Contract

### T1.4 — SOTA DoD gates
- Comando: `make check-sota-dod`
- Aceitação: 9 gates automáticos verdes (DoD #10/#11 reconhecidos como bloqueados por terminal-bench externo)

### T1.5 — Audit scripts (6 surfaces CONTENT/STRUCTURAL)
Rodar em paralelo:
- `scripts/check-allowlist-paths.sh`
- `scripts/check-env-var-coverage.sh`
- `scripts/check-workspace-deps.sh`
- `scripts/check-bench-preflight.sh`
- `scripts/check-adr-coverage.sh`
- `scripts/check-changelog-phase-coverage.sh`
- Aceitação: todos exit 0

### T1.6 — Debt baseline (não-bloqueador)
Rodar em paralelo, registrar números:
- `scripts/check-unwrap.sh` — esperado 105
- `scripts/check-panic.sh` — esperado 1 (panic intencional em `theo-tooling/src/registry/mod.rs`)
- `scripts/check-unsafe.sh` — esperado 66
- `scripts/check-secrets.sh`
- `scripts/check-module-size.sh`
- `scripts/check-complexity.sh`
- Aceitação: número ≤ baseline. Se piorou → finding HIGH.

### T1.7 — Snapshot tests críticos (invariantes do CLAUDE.md)
- `cargo test -p theo-tooling default_registry_tool_id_snapshot_is_pinned`
- `cargo test -p theo-tooling every_tool_input_example_satisfies_declared_required_params`
- `cargo test -p theo-cli every_subcommand_responds_to_help_with_exit_zero`
- Aceitação: 3/3 PASS

### T1.8 — Inventário CLI
- Comando: `cargo run -q --bin theo -- --help`
- Para cada um dos 17 subcommands: `theo <sub> --help`
- Aceitação: 17 subcommands respondem com exit 0 (já coberto por T1.7 mas confirmamos count visual)

### T1.9 — Lint estrito
- Comando: `cargo clippy --workspace --all-targets -- -D warnings`
- Aceitação: exit 0

**DoD Tier 1:** todos os comandos rodaram, todos os números registrados, regressões viraram findings.

---

## 5. Tier 2 — Dogfood com LLM (OAuth Codex)

Objetivo: o agent loop funciona em prompts reais, não só smoke fixtures.

### T2.0 — Pré-requisitos
- Verificar `~/.config/theo/auth.json` `openai.expires_at` no futuro (já confirmado 2026-05-02)
- Smoke run mínimo: `theo --headless --mode ask "diga olá em uma palavra"` retorna JSON válido com `done=true`
- Aceitação: sessão produz resposta sem erro 401/403

### T2.1 — Re-rodar smoke bench
- Comando: `python3 apps/theo-benchmark/runner/smoke.py`
- Coleta: pass rate, total tasks, tempo médio, custo por task
- Comparar com baseline `smoke-1777306420.sota.md` (18/18 = 100% reportado, mas CLAUDE.md diz 18/20 = 90%; a discrepância **deve ser investigada** — 20 tasks com 2 timeouts vs report mostrando 18 tasks)
- Aceitação: pass rate ≥ 90%; **finding obrigatório** se a contagem total não bater (18 vs 20)

### T2.2 — Dogfood real nesta codebase (3 prompts)

**Prompt A (descoberta de código):**
```
"Explique o algoritmo RRF (Reciprocal Rank Fusion) usado em theo-engine-retrieval. Cite os 3 rankers, o caminho dos arquivos chave, e como eles compõem o score final."
```
- Modo: `theo --headless --mode ask`
- Aceitação: resposta menciona BM25 + Tantivy + embeddings, cita `crates/theo-engine-retrieval/src/`, sem alucinar nomes de arquivo

**Prompt B (busca dirigida):**
```
"Liste todos os crates do workspace que importam theo_domain::prompt_sanitizer::fence_untrusted, com o caminho do arquivo e a linha."
```
- Modo: `theo --headless --mode ask`
- Aceitação: lista coerente; cross-check com `Grep "fence_untrusted"`

**Prompt C (agente — write):**
```
"No arquivo CHANGELOG.md, na seção [Unreleased] / Added, adicione UMA entrada: '- Validação dogfood end-to-end executada (#dogfood-2026-04-27)'. Não toque em nenhuma outra linha."
```
- Modo: `theo --headless --mode agent`
- Aceitação: diff mínimo (uma linha adicionada), checkpoint criado, comando termina `done=true`
- **Reverter o commit** após validação (`git checkout -- CHANGELOG.md`) — esta linha é apenas teste

### T2.3 — TUI smoke (handoff humano)
- Iniciar `theo` (sem args) — abre TUI
- Cenário sugerido para paulohenriquevn:
  1. `/help` → renderiza menu
  2. `/mode plan` → muda modo
  3. Prompt curto: "list rust files in crates/theo-domain/src"
  4. `/exit`
- Aceitação: 0 panics; resposta coerente do prompt
- **Status:** SKIP automatizado se humano não disponível; documenta no relatório

**DoD Tier 2:** smoke bench re-rodado + 3 prompts respondidos + handoff TUI documentado (executado ou SKIP justificado).

---

## 6. Tier 3 — Sidecars (4 famílias declaradas, 0 validadas até agora)

Objetivo: fechar o gap explícito do "Honest System State" — provar que cada família de tool sidecar **executa** ou marcar como SKIPPED honesto.

### T3.0 — Inventário ambiente
Detectar binários disponíveis:
- LSP: `which rust-analyzer pyright pyright-langserver gopls`
- DAP: `which lldb-vscode debugpy dlv codelldb`
- Browser: `npx playwright --version` + `ls node_modules/.bin/playwright 2>/dev/null`
- Computer Use: `which xdotool` + `echo $DISPLAY`
- Aceitação: matriz completa do que está/não está disponível registrada no relatório

### T3.1 — LSP (rust-analyzer alvo principal)
- Pré-requisito: rust-analyzer instalado
- Prompt teste: `"Use lsp_definitions para encontrar a definição de AgentRunEngine"`
- Modo: `theo --headless --mode agent`
- Aceitação: tool retorna location válido em `crates/theo-agent-runtime/src/`; **NÃO** retorna o erro "sidecar absent"
- Se rust-analyzer ausente → `cargo install rust-analyzer` ou SKIPPED

### T3.2 — DAP
- Pré-requisito: `lldb-vscode` ou `debugpy` ou `dlv` instalado
- Se nenhum disponível → SKIPPED com nota clara
- Se disponível: prompt teste mínimo (`"abra um session DAP no binário /bin/echo, defina breakpoint na entrada"`)
- Aceitação: tool retorna confirmação ou SKIPPED documentado

### T3.3 — Browser (Playwright)
- Pré-requisito: `npx playwright install chromium` (~150 MB; **pedir confirmação ao usuário antes** se ainda não instalado)
- Prompt teste: `"Use browser_navigate para abrir https://example.com e me retorne o conteúdo do <title>"`
- Aceitação: retorna `Example Domain`
- Se download bloqueado → SKIPPED

### T3.4 — Computer Use
- Pré-requisito: display server (`$DISPLAY` setado) + `xdotool`
- Headless server (caso provável aqui) → **SKIPPED automático** com motivo
- Aceitação: SKIPPED documentado OU executado se display existir

**DoD Tier 3:** matriz de inventário publicada; cada família ou EXECUTADA ou SKIPPED com motivo objetivo (ausente / sem display / sem rede). Nenhuma marcada PASS por omissão.

---

## 7. Relatório Final

**Caminho:** `docs/audit/dogfood-2026-04-27.md`

**Estrutura obrigatória:**

1. **Sumário executivo** — semáforo por tier (verde / amarelo / vermelho) + uma linha de TL;DR
2. **Tabela master** — gate / expected / actual / status / link para log
3. **Findings** — bugs, regressões, surpresas; cada um com severidade (HIGH / MED / LOW) e recomendação
4. **Gaps confirmados** — sidecars não validados (com motivo objetivo)
5. **Atualização do "Honest System State"** — propor patches concretos ao CLAUDE.md se algum número mudou
6. **Anexos** — caminhos dos JSONs de bench, transcripts dos 3 prompts, output bruto dos check-scripts

## 8. Riscos e mitigações

| Risco | Probabilidade | Mitigação |
|---|---|---|
| OAuth Codex token expira durante a run | Baixa (5 dias margem) | Rerun de `theo login` antes de Tier 2 |
| Smoke bench regride abaixo de 18 | Média | Documenta como finding HIGH; não bloqueia Tier 3 |
| Sidecar Browser exige download grande | Alta | Pedir confirmação ao usuário antes do `playwright install` |
| Sem display server para Computer Use | Quase certa | SKIPPED documentado, sem tentativa |
| TUI requer humano | Certa | Handoff explícito; SKIP justificado se indisponível |
| Discrepância 18/18 vs 18/20 nos relatórios | Confirmada | Investigação obrigatória em T2.1 |

## 9. Definição global de "Done"

- Tier 1: 100 % executado, todos os comandos rodaram, todos os números no relatório
- Tier 2: smoke bench re-rodado + 3 prompts dogfood + handoff TUI (executado ou SKIP justificado)
- Tier 3: matriz de inventário + cada família ou EXECUTADA ou SKIPPED motivado
- `docs/audit/dogfood-2026-04-27.md` publicado e linkado no CHANGELOG `[Unreleased] / Added`

## 10. Como rodar este plano

```bash
# Tier 1 — automático, paralelizável
make check-sota-dod
cargo test --workspace --exclude theo-code-desktop --lib --tests
bash scripts/check-arch-contract.sh
# (demais scripts em paralelo)

# Tier 2 — usa OAuth Codex
python3 apps/theo-benchmark/runner/smoke.py
cargo run -q --bin theo -- --headless --mode ask "..."

# Tier 3 — depende de inventário
which rust-analyzer && theo --headless --mode agent "..."
```

---

**Última atualização:** 2026-04-27
