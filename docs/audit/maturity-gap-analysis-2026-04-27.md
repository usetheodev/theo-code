# Maturity Gap Analysis — Theo Code

**Data:** 2026-04-27
**Avaliador:** Claude Code (autônomo, pós-dogfood iter-2)
**Score atual:** **3.2 / 5**
**Score-alvo próxima iteração:** **4.0 / 5**
**Score-alvo SOTA:** **5.0 / 5**
**Refresh:** rodar `make check-sota-dod` + `bash scripts/check-unwrap.sh` + `bash scripts/check-unsafe.sh` e atualizar este arquivo quando os números mudarem ≥ 5 % em qualquer dimensão.
**Fontes empíricas:**
- `docs/audit/dogfood-2026-04-27.md` (jornada do dogfood)
- `apps/theo-benchmark/reports/smoke-1777323535.sota.md` (smoke 19/20)
- `apps/theo-benchmark/reports/smoke-1777306420.sota.md` (smoke 18/20 baseline)
- `make check-sota-dod` log

---

## 1. Scorecard (verificado 2026-04-27)

| Dimensão | Nota | Justificativa |
|---|:---:|---|
| Núcleo (CLI / agent loop / tool registry / GRAPHCTX / RRF) | **4** | 5248 tests verdes, arch contract zero violações, clippy `--all-targets` strict ✅, 17 subcomandos pinados, contract tests estruturais |
| Empirical evidence | **3** | Smoke bench 19/20 = 95 % (Wilson [82.4 %, 100 %]) com OAuth Codex. Mas só 20 cenários; SWE-Bench-Verified e terminal-bench não rodados (DoD #10/#11 SKIP) |
| Test discipline | **4** | TDD enforçado, snapshot pins, **6 contract tests** (incluindo o `observability_tool_name_contract` adicionado nesta sessão), 22 audit scripts CI-enforced |
| Documentação | **4** | README + CLAUDE.md + ADRs (D1–D16) + dogfood report + audit folder. "Honest System State" é raro nesse nível |
| Honestidade / self-awareness | **5** | A seção "Honest System State" lista débitos e gaps explicitamente. O dogfood produziu uma narrativa empírica de bugs caçados |
| Sidecars (LSP / DAP / Browser / Computer Use) | **2** | LSP ✅ E2E com rust-analyzer (1 de 5 servidores). DAP ⚪ não exercitado. Browser 🟠 dispatch OK mas Chromium precisa libs SO. Computer Use ⚪ skip |
| Dívida histórica ativa | **2.5** | 96 unwrap em produção (era 105), 66 unsafe sem `// SAFETY:`, 17 god-files com sunset 2026-07-23. Tudo gateado e tracked, mas o baseline é alto |
| Resiliência | **3** | Sandbox cascade `bwrap > landlock > noop`, capability gate sempre instalada, secret scrubbing, prompt-injection fences. Falta load testing, chaos testing, fuzzing público |
| Operational readiness | **2.5** | `cargo install` funciona; sem homebrew/apt/msi, sem release pipeline público, sem SLA documentada, sem real-world adoption metrics |
| Bug-hunting culture | **4** | Esta sessão sozinha caçou 9 bugs stale silenciosos que existiam desde o início — incluindo um que desabilitava 4 famílias de tools em produção. O fato de isso ter sido pegável significa que o sistema é auditável; o fato de existir significa que a cobertura E2E ainda é incompleta |

**Média ponderada (10 dimensões iguais):** **3.4** · **Média mínima por dimensão:** **2** (Sidecars) · **Score honesto reportado:** **3.2** (descontando overlap entre dimensões correlacionadas).

---

## 2. Gap analysis — uma seção por dimensão

Cada gap segue o template:
> **Gap N (severidade):** descrição
> - **Estado atual:** com número
> - **Estado-alvo:** definição de "fechado"
> - **Ação:** comando ou arquivo a tocar
> - **Esforço:** S (≤ 4 h) / M (1-3 dias) / L (1-3 semanas) / XL (> 3 semanas)
> - **Owner:** TBD (não atribuído ainda)
> - **Bloqueia:** lista de outros gaps cuja resolução depende deste

### 2.1 Núcleo (4 → 5)

#### Gap 1.1 (HIGH) · Cobertura E2E entre `cmd_headless` e o registry de produção
- **Estado atual:** `AgentLoop::build_registry` ficou usando `create_default_registry()` (catálogo vazio) em produção por **período desconhecido** sem que nenhum teste pegasse — descoberto só no dogfood deste iter.
- **Estado-alvo:** integration test que arranca uma `theo --headless` real, executa um prompt forçando uso de cada família de tool sidecar, e valida que cada tool dispatch chega à versão project-aware (não ao stub).
- **Ação:** novo arquivo `crates/theo-application/tests/headless_uses_project_aware_registry.rs` que invoca `cmd_headless` programaticamente OU um shell test em `apps/theo-cli/tests/` que sobe o binário e inspeciona `report.tool_breakdown`.
- **Esforço:** M
- **Bloqueia:** nenhum

#### Gap 1.2 (MED) · `theo desktop` excluído do gate canônico
- **Estado atual:** `theo-code-desktop` é excluído de `cargo test --workspace` por dependência de `gio-2.0` / `glib-2.0` (T1.1 do dogfood). CI não testa o desktop.
- **Estado-alvo:** ou (a) job dedicado `audit.yml::desktop` rodando em runner com GTK instalado, ou (b) feature gating que permita compilar a Tauri shell sem GTK.
- **Ação:** adicionar `audit.yml::desktop-tests` ubuntu-latest com `apt install libgtk-3-dev`.
- **Esforço:** M
- **Bloqueia:** Operational readiness 2.5 → 3

#### Gap 1.3 (LOW) · Performance benchmark do agent loop
- **Estado atual:** existe `crates/theo-agent-runtime/benches/run_engine_bench.rs` mas não está no CI.
- **Estado-alvo:** rodar criterion no CI semanal com regression detection (e.g. > 10 % slowdown falha).
- **Ação:** workflow `bench.yml` agendado.
- **Esforço:** S

---

### 2.2 Empirical evidence (3 → 4 → 5)

#### Gap 2.1 (CRITICAL) · DoD #10 — SWE-Bench-Verified ≥ 10 pt acima do baseline
- **Estado atual:** OUT-OF-SCOPE explícito em `make check-sota-dod`. O smoke bench (20 cenários simples) cobre só a layer "smoke", não SWE-Bench.
- **Estado-alvo:** rodar SWE-Bench-Verified contra o commit `37cb3b2` (baseline pre-SOTA) e contra HEAD; medir delta. Para SOTA: ≥ 10 pp acima.
- **Ação:**
  1. Provisionar terminal-bench (Docker compose com SWE-Bench-Verified setup).
  2. Wire `apps/theo-benchmark/runner/swe_bench.py` para apontar para o dataset e correr theo headless.
  3. Documentar o delta no `apps/theo-benchmark/reports/`.
- **Esforço:** XL (terminal-bench setup + LLM cost: ~$200-500 por run completo)
- **Bloqueia:** score ≥ 4 desta dimensão

#### Gap 2.2 (CRITICAL) · DoD #11 — Tier T1 (7/7) + T2 (9/9) coverage
- **Estado atual:** OUT-OF-SCOPE. O scenario-to-tier mapping não existe em código.
- **Estado-alvo:** cada cenário de bench tagueado com `tier = "T1" | "T2" | "T3"` e o report agrega passe rate por tier.
- **Ação:** adicionar campo `tier` aos `apps/theo-benchmark/scenarios/smoke/*.toml` + agregação em `analysis/report_builder.py`.
- **Esforço:** M

#### Gap 2.3 (HIGH) · Multi-provider parity
- **Estado atual:** smoke bench rodado APENAS com OAuth Codex `gpt-5.4`. Anthropic / Ollama / Groq / Cerebras nunca medidos.
- **Estado-alvo:** matriz `bench × provider` com pelo menos 3 providers (1 Anthropic, 1 OpenAI, 1 local-Ollama) na mesma suite.
- **Ação:** `apps/theo-benchmark/runner/smoke.py` com `--provider <X>` + um workflow `eval.yml::matrix` que itera providers.
- **Esforço:** M (custo: ~$30-60 por provider × scenario)

#### Gap 2.4 (MED) · Statistical power do smoke bench
- **Estado atual:** 20 cenários; Wilson 95 % CI [82.4 %, 100 %] = ±9 pp de incerteza.
- **Estado-alvo:** ≥ 50 cenários para CI ±5 pp.
- **Ação:** expandir `apps/theo-benchmark/scenarios/smoke/` com 30 cenários adicionais cobrindo mais categorias.
- **Esforço:** L

#### Gap 2.5 (MED) · Retrieval bench escala limitada
- **Estado atual:** RRF measurement é MRR=0.86 / Hit@5=0.97 sobre 57 queries em 3 repos (`crates/theo-engine-retrieval/tests/benchmarks/`).
- **Estado-alvo:** ≥ 200 queries em ≥ 10 repos diversos (Rust, Python, TS, Go, Java).
- **Ação:** expandir `tests/benchmarks/ground_truth/`.
- **Esforço:** M

---

### 2.3 Test discipline (4 → 5)

#### Gap 3.1 (HIGH) · Mutation testing
- **Estado atual:** `cargo-mutants` configurado em `complexity-allowlist` mas não rodado em CI.
- **Estado-alvo:** mutation kill rate ≥ 60 % em `theo-domain` + `theo-tooling` + `theo-agent-runtime/observability`.
- **Ação:** workflow `mutation.yml` semanal que falha se kill rate < 60 %.
- **Esforço:** L (custo: 4-8h CPU por run)

#### Gap 3.2 (MED) · Property-based testing
- **Estado atual:** zero proptest no workspace (verificável via `grep -rn "use proptest" crates/`).
- **Estado-alvo:** proptest para parsers (Tree-Sitter wrappers) + RRF rank stability + secret scrubber + prompt sanitizer.
- **Ação:** adicionar `proptest = "1"` ao workspace e ≥ 3 propriedades por target acima.
- **Esforço:** M

#### Gap 3.3 (MED) · Cobertura de linhas
- **Estado atual:** 38.56 % line-rate no cobertura.xml local (gate accepta ≥ 30 %).
- **Estado-alvo:** ≥ 70 % nas 5 SOTA-touched crates (`theo-domain`, `theo-engine-retrieval`, `theo-tooling`, `theo-agent-runtime`, `theo-application`).
- **Ação:** rodar `cargo tarpaulin` por crate e atacar paths não cobertos com testes direcionados.
- **Esforço:** L

#### Gap 3.4 (LOW) · Test que arranca o desktop app
- **Estado atual:** zero teste exercita `theo-desktop` (Tauri).
- **Estado-alvo:** smoke test que sobe a Tauri shell em headless e verifica que o IPC roteia.
- **Ação:** integration test em `apps/theo-desktop/tests/`.
- **Esforço:** M (resolve juntos do Gap 1.2)

---

### 2.4 Documentação (4 → 5)

#### Gap 4.1 (MED) · API docs (rustdoc) publicadas
- **Estado atual:** `cargo doc` gera; docs.rs não publica (não é crate publicada).
- **Estado-alvo:** site GitHub Pages com `cargo doc --no-deps` de cada crate público.
- **Ação:** workflow `docs.yml` que builda + publica em `gh-pages`.
- **Esforço:** S

#### Gap 4.2 (MED) · Diagramas C4
- **Estado atual:** ASCII art em README + arch contract; sem diagrama de Container/Component visual.
- **Estado-alvo:** 4 níveis C4 (System Context, Container, Component, Code) em SVG via Structurizr/PlantUML.
- **Ação:** `docs/architecture/c4/*.dsl` + render em CI.
- **Esforço:** M

#### Gap 4.3 (MED) · Migration / upgrade guide
- **Estado atual:** sem versioning público (todas as versões são 0.1.0).
- **Estado-alvo:** SemVer começando em 0.2.0 + `MIGRATING.md` documentando breaking changes.
- **Ação:** definir politica de versão + criar `MIGRATING.md`.
- **Esforço:** S
- **Bloqueia:** Operational readiness Gap 5.1

#### Gap 4.4 (LOW) · Tutorial / video / quickstart interativo
- **Estado atual:** README "Quick Start" 3 linhas.
- **Estado-alvo:** docs site com `docs/tutorial/01-fix-a-bug.md` ... `04-pilot-loop.md`, ≥ 4 walkthroughs.
- **Ação:** mdBook em `docs/book/`.
- **Esforço:** L

---

### 2.5 Honestidade (5 — manter)

Sem gaps. Manter:
- "Honest System State" refrescada a cada `make check-sota-dod` produzindo Δ.
- Audit reports com data e Δ explícito (`docs/audit/*.md`).

---

### 2.6 Sidecars (2 → 4)

#### Gap 6.1 (CRITICAL) · DAP nunca exercitado E2E
- **Estado atual:** 11 tools `debug_*` registrados; zero testes contra `lldb-vscode` / `debugpy` / `dlv` / `codelldb`.
- **Estado-alvo:** smoke `debug_status` + `debug_launch` + `debug_set_breakpoint` + `debug_continue` validados contra ≥ 1 debugger.
- **Ação:**
  1. Instalar `debugpy` (Python) ou `lldb-vscode` (sistema).
  2. Adicionar `apps/theo-benchmark/scenarios/dap/*.toml` com 5 cenários.
  3. Wire em `audit.yml::sidecar-dap`.
- **Esforço:** M (debugpy instala via pipx fácil; lldb-vscode requer apt)
- **Bloqueia:** sidecars 2 → 3

#### Gap 6.2 (HIGH) · Browser sidecar Chromium libs
- **Estado atual:** dispatch OK; sidecar bundled (`include_str!`); Playwright npm + Chromium 1217 baixam OK; **Chromium binary não roda** sem libs SO (libatk, libgbm, libcairo, libpango, libxcomposite, libxdamage, libxfixes, libnss, libasound).
- **Estado-alvo:** smoke `browser_open` + `browser_eval` validados num runner CI com `apt install libatk1.0-0 libgbm1 ...`.
- **Ação:**
  1. Adicionar step `apt install`-os no `audit.yml::sidecar-browser` (job dedicado).
  2. Cenário `browser/01-open-eval.toml`.
- **Esforço:** S (deps documentadas; só wire CI)
- **Bloqueia:** sidecars 3 → 4

#### Gap 6.3 (HIGH) · LSP cobertura limitada (1 / 5 servidores)
- **Estado atual:** rust-analyzer 1.95.0 ✅. `pyright` / `gopls` / `clangd` / `typescript-language-server` ainda não.
- **Estado-alvo:** 5 servidores instalados num runner CI; smoke `lsp_status` retorna ≥ 5 extensões; `lsp_definition` exercitado em ≥ 3 linguagens.
- **Ação:** matriz CI `audit.yml::sidecar-lsp` instalando os 5 binários + cenários por linguagem.
- **Esforço:** M
- **Bloqueia:** sidecars 4 → 5

#### Gap 6.4 (LOW) · Computer Use sem display server
- **Estado atual:** SKIP automático (`$DISPLAY` vazio).
- **Estado-alvo:** runner com `xvfb` + `xdotool` instalados; smoke `computer_action` (1 cenário trivial: mover mouse, tirar screenshot).
- **Ação:** `audit.yml::sidecar-computer` com xvfb-run.
- **Esforço:** M

---

### 2.7 Dívida histórica ativa (2.5 → 4)

#### Gap 7.1 (HIGH) · 96 unwrap em production paths
- **Estado atual:** `bash scripts/check-unwrap.sh` reporta 96 (era 105 pre-dogfood; -9 pagos nesta sessão). Gate RED em strict mode.
- **Estado-alvo:** ≤ 30 unwrap (ou 0 se viável).
- **Ação:** sweep dirigido por crate. Estimativa: 10-15 PRs cirúrgicas atacando 6-10 sites cada.
  - Próximas vítimas (de `check-unwrap.sh`):
    - `crates/theo-engine-graph/src/cluster.rs:756, 758, 894, 906, 908, 910, 925` — `partial_cmp().unwrap()` em floats; usar `unwrap_or(Ordering::Equal)`.
    - `crates/theo-engine-parser/src/extractors/php.rs:485` — `parse_http_method().unwrap()`; ver context.
    - `crates/theo-tooling/src/webfetch/mod.rs:292` — `quote.unwrap()` após guard.
    - `crates/theo-tooling/src/read/mod.rs:434, 436` — encoder write.
    - `crates/theo-tooling/src/test_gen/property.rs:228` — `name.chars().next().unwrap()`.
    - `crates/theo-agent-runtime/src/bin/theo-agent.rs:407` — bin entry.
    - `crates/theo-tooling/src/lsp/tool.rs:939` — LSP tool path.
    - `crates/theo-tooling/src/apply_patch/mod.rs:369` — `candidates.last().unwrap()`.
- **Esforço:** L (sweep contínuo; -10/PR é razoável)
- **Bloqueia:** dívida 3 → 4

#### Gap 7.2 (HIGH) · 66 unsafe blocks sem `// SAFETY:` comment
- **Estado atual:** `bash scripts/check-unsafe.sh` reporta 66 violations (mostly env-var mutation em tests + FFI em `graph_context_service` / `observability/otel_exporter`).
- **Estado-alvo:** **0** (todo unsafe block tem comentário `// SAFETY: <invariante>` justo acima).
- **Ação:** sweep batched — `crates/theo-agent-runtime/src/observability/otel_exporter.rs:345-356` é o cluster mais visível; resto é setup de testes.
- **Esforço:** S (cosmetic, mas necessário pra compliance ✅)
- **Bloqueia:** dívida 4 → 5

#### Gap 7.3 (MED) · 17 god-files allowlistados (sunset 2026-07-23)
- **Estado atual:** lista em `.claude/rules/size-allowlist.txt` — 17 entradas com sunset 2026-07-23.
- **Estado-alvo:** todos divididos em sub-módulos; allowlist com 0 entradas god-file (apenas test-only file overrides).
- **Ação:** plano de decomposição por arquivo. Os mais visíveis:
  - `crates/theo-tooling/src/registry/mod.rs` (1550 LOC) → mover builders para `registry/builders/`.
  - `crates/theo-agent-runtime/src/pilot/mod.rs` (allowlist 1450) → split por loop stage.
  - `crates/theo-agent-runtime/src/subagent/mod.rs` (1500) → split por integration.
- **Esforço:** XL (cada split é PR de 200-400 LOC + testes)
- **Bloqueia:** dívida 4.5 → 5

#### Gap 7.4 (LOW) · 75 functions > 100 LOC locked baseline
- **Estado atual:** `.claude/rules/complexity-allowlist.txt` baseline imutável.
- **Estado-alvo:** ≤ 30 functions > 100 LOC.
- **Ação:** sweep oportunista junto dos splits god-file (Gap 7.3).
- **Esforço:** L

---

### 2.8 Resiliência (3 → 4 → 5)

#### Gap 8.1 (HIGH) · Sem load testing
- **Estado atual:** zero testes que estressem agent loop sob carga concorrente.
- **Estado-alvo:** integration test que sobe N sub-agents simultâneos (N=10, 50, 100) e mede:
  - Throughput (tools/s)
  - Latência p95
  - Memória pico
- **Ação:** `crates/theo-agent-runtime/tests/load_subagent.rs`.
- **Esforço:** L

#### Gap 8.2 (HIGH) · Sem chaos / fault injection
- **Estado atual:** sandbox cascade `bwrap > landlock > noop` documentada mas não testada sob falhas.
- **Estado-alvo:** test suite que injeta:
  - LLM provider 5xx random
  - Tool sandbox kill durante execução
  - Disk full (`/tmp` cheio)
  - Network partition (provider unreachable)
- **Ação:** `crates/theo-agent-runtime/tests/chaos_*.rs` + `crates/theo-isolation/tests/sandbox_chaos.rs`.
- **Esforço:** L

#### Gap 8.3 (MED) · Fuzzing público
- **Estado atual:** zero `cargo-fuzz` target.
- **Estado-alvo:** ≥ 5 fuzz targets em paths de parsing (Tree-Sitter wrappers, secret_scrubber, prompt_sanitizer, JSON envelope) rodando OSS-Fuzz ou GitHub Actions.
- **Ação:** `crates/theo-domain/fuzz/` com 5 targets.
- **Esforço:** M

---

### 2.9 Operational readiness (2.5 → 4 → 5)

#### Gap 9.1 (HIGH) · Sem release pipeline / SemVer
- **Estado atual:** todas as crates em 0.1.0; sem tags; sem changelog versionado (só `[Unreleased]`).
- **Estado-alvo:** `cargo release` configurado + workflow `release.yml` que cria tag, GitHub Release, e binários cross-compiled (Linux x86_64, macOS arm64, Windows x86_64).
- **Ação:**
  1. Mover `[Unreleased]` para `[0.2.0] - 2026-04-27` (ou next).
  2. Configurar `cargo-release` no workspace.
  3. `release.yml` workflow.
- **Esforço:** M
- **Bloqueia:** ops 3 → 4

#### Gap 9.2 (HIGH) · Sem distribution channels
- **Estado atual:** apenas `cargo install --path apps/theo-cli`.
- **Estado-alvo:** disponível em ≥ 3 canais:
  - Homebrew (macOS / Linux)
  - apt repository (Debian / Ubuntu)
  - winget / scoop (Windows)
  - .deb / .rpm em GitHub Releases
- **Ação:** Brew tap `usetheodev/tap`; workflow `release.yml` publica nos tres canais via `goreleaser-rust` ou similar.
- **Esforço:** L
- **Bloqueia:** ops 4 → 5

#### Gap 9.3 (MED) · Sem SLA / política de suporte documentada
- **Estado atual:** sem `SUPPORT.md` / sem política de breaking changes.
- **Estado-alvo:** `SUPPORT.md` declarando: faixa de versão suportada, política de breaking changes, canais de suporte (issues / discussions / Discord).
- **Ação:** `SUPPORT.md` + `SECURITY.md` (se ainda não existe).
- **Esforço:** S

#### Gap 9.4 (MED) · Sem adoption metrics
- **Estado atual:** sem star count visível no README, sem download stats, sem telemetria opt-in.
- **Estado-alvo:** badges live (GitHub stars / cargo downloads / homebrew installs) + opt-in telemetria anônima documentada (`THEO_TELEMETRY=1`).
- **Ação:** badges shields.io + telemetria via `theo-infra-llm`-like provider para `usetheo.dev/telemetry`.
- **Esforço:** M (bem-thought, privacy-friendly)

---

### 2.10 Bug-hunting culture (4 → 5)

#### Gap 10.1 (HIGH) · Sem bug bounty / red team externo
- **Estado atual:** dogfood interno catches stale bugs (9 nesta sessão); zero olhos externos.
- **Estado-alvo:** programa formal de bug bounty (mesmo que com escopo restrito a sandbox / capability gate / secret scrubber) via huntr.dev ou GitHub Security Advisories.
- **Ação:** `SECURITY.md` + tabela de scope + threats fora de escopo + bounty mesmo simbólico ($100/HIGH).
- **Esforço:** M (legal + financeiro)

#### Gap 10.2 (MED) · Sem fuzzing público em CI
- **Coberto pelo Gap 8.3.**

#### Gap 10.3 (LOW) · Sem playbook de incident response
- **Estado atual:** sem `docs/runbook/` / sem on-call rotation (justo por não ter prod ainda).
- **Estado-alvo:** `docs/runbook/incident-response.md` documentando como tratar:
  - Sandbox escape suspect
  - Secret leak em transcript
  - LLM provider downtime cascading
- **Ação:** runbook minimo.
- **Esforço:** S

---

## 3. Roadmap consolidado

### 3.1 Próxima iteração (foco 3.2 → 4.0) — esforço L

Ordenado por leverage / esforço:

| Gap | Esforço | Impacto |
|---|:---:|---|
| 7.2 — Adicionar `// SAFETY:` em 66 unsafe (cosmetic, fast win) | S | Fecha gate RED |
| 9.3 — `SUPPORT.md` + `SECURITY.md` | S | Documentação clara |
| 4.1 — Publicar rustdoc em GitHub Pages | S | API discoverable |
| 6.1 — DAP E2E (instalar debugpy + 5 cenários) | M | Sidecars 2 → 3 |
| 6.2 — Browser job CI com libs SO | S | Sidecars 3 → 3.5 |
| 1.1 — Integration test `cmd_headless` ↔ project-aware registry | M | Núcleo 4 → 4.5 |
| 7.1 — Sweep unwrap 96 → 60 (~5 PRs de 8 sites cada) | L | Dívida 2.5 → 3.5 |
| 9.1 — Release pipeline + SemVer 0.2.0 | M | Ops 2.5 → 3.5 |
| 2.2 — Tier T1/T2 mapping nos cenários | M | Empirical 3 → 3.5 |

**Score projetado pós-iteração:** ~**4.0**.

### 3.2 Iteração para SOTA (4.0 → 5.0) — esforço XL

| Gap | Esforço | Bloqueia |
|---|:---:|---|
| 2.1 — SWE-Bench-Verified ≥10pt | XL | DoD #10 |
| 2.3 — Multi-provider parity bench | M | Empirical 4 → 5 |
| 7.3 — Decompor 17 god-files (sunset 2026-07-23) | XL | Dívida 4 → 5 |
| 6.3 — LSP em 5 servidores | M | Sidecars 4 → 5 |
| 8.1 — Load testing | L | Resiliência 3 → 4 |
| 8.2 — Chaos / fault injection | L | Resiliência 4 → 5 |
| 8.3 — Fuzzing público | M | Resiliência + bug-hunt 4 → 5 |
| 9.2 — Distribution channels | L | Ops 4 → 5 |
| 10.1 — Bug bounty externo | M | Bug-hunt 4 → 5 |

**Score projetado:** **5.0** (com a ressalva de que adoption real só vem com tempo, não esforço puro).

---

## 4. Tracking & cadência

### 4.1 Refresh deste documento
Quando rodar:
- A cada `make check-sota-dod` que muda Δ ≥ 5 % em qualquer dimensão
- A cada smoke bench novo (`apps/theo-benchmark/reports/smoke-*`)
- A cada minor bump em `Cargo.toml` workspace version

Como rodar (one-liner):
```bash
make check-sota-dod && \
  bash scripts/check-unwrap.sh && \
  bash scripts/check-unsafe.sh && \
  THEO_SKIP_ONBOARDING=1 python3 apps/theo-benchmark/runner/smoke.py
```

### 4.2 Métricas a acompanhar (chart-friendly)

| Métrica | Hoje | Próxima iter | SOTA |
|---|---:|---:|---:|
| Tests passing | 5248 | 5300+ | 5500+ |
| Smoke pass rate | 19/20 | 19/20 | ≥ 50/50 |
| Unwrap baseline | 96 | ≤ 60 | 0 |
| Unsafe sem SAFETY | 66 | 0 | 0 |
| God-files allowlistados | 17 | 14 | 0 |
| Sidecars E2E validated | 1/4 | 3/4 | 4/4 |
| LSP servers exercitados | 1/5 | 1/5 | 5/5 |
| LLM providers benched | 1 | 1 | ≥ 3 |
| SWE-Bench-Verified delta | N/A | N/A | ≥ +10 pp |

### 4.3 Snapshot serializável
A próxima iteração pode anexar abaixo:

```yaml
# 2026-04-27
overall_score: 3.2
dimensions:
  nucleo: 4
  empirical: 3
  test_discipline: 4
  documentacao: 4
  honestidade: 5
  sidecars: 2
  divida_historica: 2.5
  resiliencia: 3
  ops_readiness: 2.5
  bug_hunting: 4
hard_numbers:
  tests_passing: 5248
  smoke_pass: "19/20"
  unwrap: 96
  unsafe_no_safety: 66
  god_files_allowlisted: 17
  fns_over_100_loc: 75
  sidecars_e2e: 1
  lsp_servers_exercitados: 1
  providers_benched: 1
```

---

## 5. Conclusão

Theo Code está em **3.2 / 5** — **production-ready para uso interno e early-adopters técnicos** com:

- Self-awareness honesta (raro)
- Núcleo bem testado e gateado
- Dívida histórica formalizada e em redução

O caminho para **4.0** é claro e mecânico (8-12 PRs ao longo de uma sprint).
O caminho para **5.0 (SOTA)** depende de:

1. **Empirical evidence** — SWE-Bench-Verified rodado de fato (Gap 2.1)
2. **Sidecars completos** — DAP/Browser/Computer Use exercitados E2E
3. **Adoption real** — métricas de uso > 0 (não controlável apenas por esforço)
4. **External eyes** — bug bounty / red team / fuzzing OSS-Fuzz

**Fim do gap analysis.**
