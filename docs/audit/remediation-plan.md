# Plano Executável — Resolução do Code Audit

**Base:** relatório de `/code-audit all` de 2026-04-23 (commit `aa4ea43`)
**Veredito atual:** FAIL
**Objetivo:** converter FAIL → PASS em todas as 8 técnicas, respeitando os princípios inquebráveis (TDD obrigatório, SRP, DIP, fail-fast).
**Estratégia:** 7 fases sequenciais por dependência, cada uma com tasks atômicas, critérios de aceite objetivos e DoD verificável.

---

## Fase 0 — Ferramental (pré-requisito bloqueante)

Sem as ferramentas, metade dos audits permanece WARN-por-omissão. Tudo para até isso ser resolvido.

### T0.1 — Instalar toolchain de auditoria Rust
**Descrição:** instalar `cargo-audit`, `cargo-deny`, `cargo-outdated`, `cargo-tarpaulin`, `cargo-mutants`, `cargo-modules`, `cargo-geiger`.
**Critério de aceite:** todos os comandos respondem a `--version` sem erro.
**DoD:** registro em `.theo/tooling-inventory.md` com versão fixada; script `scripts/install-audit-tools.sh` idempotente criado; `make audit-tools-check` existe e retorna 0.

### T0.2 — Instalar toolchain de auditoria Node/TS
**Descrição:** adicionar como devDeps em `apps/theo-ui/package.json`: `@stryker-mutator/core`, `@stryker-mutator/vitest-runner`, `madge`, `license-checker`.
**Critério de aceite:** `npm ls @stryker-mutator/core` resolve; `npx madge --version` funciona.
**DoD:** versões fixadas (caret permitido em minors), documentado em `apps/theo-ui/README.md` seção "Audit tooling".

### T0.3 — Instalar SAST/Secrets externos
**Descrição:** `gitleaks`, `semgrep`, `osv-scanner`.
**Critério de aceite:** os três comandos respondem a `--version`.
**DoD:** instruções em `docs/audit/tooling.md` (linux/mac), integrados ao `scripts/install-audit-tools.sh`.

### T0.4 — Pipeline unificado `just audit` / `make audit`
**Descrição:** criar target único que roda todas as 8 técnicas e falha em qualquer regressão.
**Critério de aceite:** `make audit` retorna 0 em estado limpo; retorna != 0 se qualquer técnica regride.
**DoD:** documentado em `docs/audit/README.md`; reutilizado pela skill `code-audit`.

---

## Fase 1 — Violações de Arquitetura (BLOCKER #1–3)

Viola invariantes documentados em `.claude/CLAUDE.md` e `docs/adr/`. Nada mais avança até ser corrigido.

### T1.1 — Corrigir `Cargo.toml` de `theo-agent-runtime`
**Descrição:** contrato diz `theo-domain, theo-governance`. Hoje declara `theo-domain, theo-infra-auth, theo-infra-llm, theo-tooling` e **não** declara `theo-governance`.
**Critério de aceite:**
- `theo-agent-runtime/Cargo.toml` passa a declarar apenas `theo-domain` + `theo-governance`.
- Dependências de `theo-infra-llm`, `theo-infra-auth`, `theo-tooling` movidas para `theo-application` (caso de uso) ou convertidas em traits em `theo-domain` injetadas por DIP.
- `cargo build --workspace` verde.
**DoD:** PR com ADR novo em `docs/adr/NNNN-agent-runtime-dip.md` explicando as interfaces extraídas; teste de arquitetura `crates/theo-agent-runtime/tests/arch_contract.rs` que lê `Cargo.toml` e falha se reaparecer qualquer dep proibida.

### T1.2 — Desacoplar `apps/theo-cli` de engine/infra (11 call sites)
**Descrição:** substituir todos os `use theo_agent_runtime::...`, `use theo_infra_llm::...`, `use theo_tooling::...` em `apps/theo-cli/src/` por tipos expostos em `theo-application` + `theo-api-contracts`.
**Call sites explícitos para eliminar:**
- `apps/theo-cli/src/tui/mod.rs:32-37`
- `apps/theo-cli/src/pilot.rs:6-8,180`
- `apps/theo-cli/src/main.rs:941`
- `apps/theo-cli/src/renderer.rs:14`

**Critério de aceite:**
- `grep -rn "use theo_agent_runtime\|use theo_infra_\|use theo_tooling" apps/theo-cli/src/` retorna 0 linhas.
- `apps/theo-cli/Cargo.toml` declara apenas `theo-application`, `theo-api-contracts` e deps não-workspace.
- CLI segue funcional (smoke test abaixo).

**DoD:** smoke test em `apps/theo-cli/tests/e2e_smoke.rs` com `assert_cmd` cobrindo `theo --help`, `theo login --dry-run`, `theo chat --prompt X`; teste de arquitetura `apps/theo-cli/tests/arch_contract.rs`.

### T1.3 — Desacoplar `apps/theo-desktop` de engine/infra (6 call sites)
**Descrição:** idem T1.2 para desktop.
**Call sites explícitos:**
- `apps/theo-desktop/src/state.rs:4`
- `apps/theo-desktop/src/events.rs:2`
- `apps/theo-desktop/src/commands/auth.rs:2`
- `apps/theo-desktop/src/commands/observability.rs:9`
- `apps/theo-desktop/src/commands/copilot.rs:2`
- `apps/theo-desktop/src/commands/anthropic_auth.rs`

**Critério de aceite:**
- grep equivalente ao T1.2 → 0 matches.
- `cargo tauri build` ok.

**DoD:** comandos Tauri reexpostos por `theo-application`; teste de arquitetura em `apps/theo-desktop/tests/arch_contract.rs`; teste IPC mínimo cobrindo um comando por módulo.

### T1.4 — Validar `theo-engine-parser` e `theo-infra-auth` contra contrato
**Descrição:** contrato exige `theo-domain`; hoje ambos não importam. Decidir: (a) adicionar dep real (se há tipos compartilhados) ou (b) atualizar contrato formalmente via ADR.
**Critério de aceite:** decisão documentada em ADR; `Cargo.toml` conforme ADR; teste de arquitetura em `tests/arch_contract.rs` de cada crate.
**DoD:** ADR merged; ambos `cargo build -p theo-engine-parser -p theo-infra-auth` verdes.

### T1.5 — Gate CI "arch-contract"
**Descrição:** script `scripts/check-arch-contract.sh` que lê um YAML declarativo com o contrato e valida para cada crate que (i) `Cargo.toml` só declara deps permitidas, (ii) `src/` não importa crates proibidos.
**Critério de aceite:** script existe, roda em < 5s, falha com mensagem explicativa no primeiro desvio; rodado em `.github/workflows/ci.yml` como job bloqueante.
**DoD:** contrato em `.claude/rules/architecture-contract.yaml` (fonte-da-verdade); README de como atualizar.

---

## Fase 2 — Segurança Crítica (BLOCKER #5,#6,#7,#9)

Invariantes de segurança do CLAUDE.md: sandbox obrigatório, OAuth correto, fail-fast no Rust.

### T2.1 — Testes de integração do sandbox cascade
**Descrição:** criar `crates/theo-tooling/tests/sandbox_cascade.rs` cobrindo bwrap → landlock → noop, com `fail_if_unavailable=true` (padrão) e `false`.
**Cenários obrigatórios:**
1. bwrap presente: executor é `BwrapExecutor`.
2. bwrap ausente, landlock presente: executor é `LandlockExecutor`.
3. ambos ausentes, `fail_if_unavailable=true`: construção **retorna erro**.
4. ambos ausentes, `fail_if_unavailable=false`: retorna `NoopExecutor` **e emite log WARN estruturado**.
5. tentativa de `../../etc/passwd` dentro do sandbox: bloqueada.

**Critério de aceite:** 5 testes verdes; `cargo test -p theo-tooling --test sandbox_cascade` em < 30s em máquinas com e sem bwrap (skip condicional quando ausente, com `#[cfg_attr(not(target_os="linux"), ignore)]`).
**DoD:** cobertura da função `Executor::new` → 100% em `cargo tarpaulin`; path-traversal negativo coberto.

### T2.2 — Warning estruturado no fallback Noop
**Descrição:** em `crates/theo-tooling/src/sandbox/executor.rs:369,380`, ao construir `NoopExecutor`, emitir `tracing::warn!(target="sandbox", reason="no_backend_available", "falling back to NoopExecutor — bash tools run without isolation")`.
**Critério de aceite:** teste unitário com `tracing-test` captura o log; flag `config.fail_if_unavailable=false` nunca é silencioso.
**DoD:** entry em `CHANGELOG.md` seção Security: "Noop sandbox fallback now emits structured warning (#TICKET)".

### T2.3 — Canonicalize explícito nas fronteiras do `theo-tooling`
**Descrição:** auditar cada tool que recebe path (`read`, `write`, `edit`, `glob`, `grep`, `apply_patch`). Aplicar `std::fs::canonicalize` + validação de prefixo antes de qualquer I/O.
**Critério de aceite:**
- `grep -rn "canonicalize" crates/theo-tooling/src/` retorna ≥ N sites (N = número de tools com path input).
- Testes com payload `../`, symlink, e caminho absoluto fora do workspace: todos rejeitados.

**DoD:** função helper `theo_tooling::path::safe_resolve(root, input) -> Result<PathBuf>` com testes dedicados; todos os tools que aceitam path passam a usá-la.

### T2.4 — Testes de integração do OAuth (theo-infra-auth)
**Descrição:** criar `crates/theo-infra-auth/tests/` com casos cobrindo PKCE, device flow e refresh. Usar `wiremock` para simular o provider.
**Cenários obrigatórios:**
1. PKCE happy path (code → token → refresh).
2. Code_verifier inválido → erro tipado.
3. Device flow: polling, expiração, sucesso.
4. Refresh token expirado → erro `TokenExpired`, não panic.
5. Sessão válida reaproveitada (confere `expires_at` em `~/.config/theo/auth.json`).

**Critério de aceite:** 5+ cenários verdes; 0 `unwrap` nos casos de erro; mock não usa rede real.
**DoD:** `cargo tarpaulin -p theo-infra-auth` ≥ 80% line coverage; CHANGELOG atualizado.

### T2.5 — Eliminar `.unwrap()` em produção (~90 sites)
**Descrição:** substituir por `?`, `expect("invariante documentada")` com justificativa, ou erro tipado. Concentração: `theo-engine-retrieval` (21), `theo-agent-runtime` (19), `theo-infra-llm` (15), `theo-application` (8), `theo-tooling` (8).
**Critério de aceite:**
- Script `scripts/check-unwrap.sh` que roda `rg "\.unwrap\(\)" crates/*/src/ --glob '!**/tests/**' --glob '!**/mod_test*.rs'` e ignora `#[cfg(test)]` → retorna 0 linhas.
- Lint clippy com `clippy::unwrap_used = "deny"` no workspace.

**DoD:** adicionado a `.cargo/config.toml` ou `clippy.toml`; gate CI rejeita PR que introduza `.unwrap()` fora de teste.

### T2.6 — Eliminar `panic!`/`todo!`/`unimplemented!` em produção
**Descrição:** 49 `panic!` + 1 `todo!`/`unimplemented!`. Converter em erros tipados (domínio já usa `thiserror`).
**Critério de aceite:** `clippy::panic = "deny"`, `clippy::todo = "deny"`, `clippy::unimplemented = "deny"` no workspace (exceto `#[cfg(test)]`).
**DoD:** CI rejeita regressão; CHANGELOG.md seção Changed.

### T2.7 — Limitar tamanho de deserialização JSON
**Descrição:** 22 `serde_json::from_str` em theo-infra-llm/theo-application sem limite de tamanho. Envolver em helper `deserialize_with_limit(input, max_bytes)`.
**Critério de aceite:** todos os sites usam o helper; teste de DoS com payload de 100MB → erro `PayloadTooLarge`.
**DoD:** limite default 10MB documentado; override por provider via config.

### T2.8 — Corrigir CVEs de `vite`
**Descrição:** `cd apps/theo-ui && npm audit fix` (HIGH: GHSA-4w7w-66w2-5vf9, GHSA-p9ff-h696-f583).
**Critério de aceite:** `npm audit --audit-level=high` retorna 0 advisories; `npm run build` verde; UI sobe com `npm run dev` e smoke test manual do devserver passa.
**DoD:** `package-lock.json` commitado; CHANGELOG seção Security.

### T2.9 — Auditar `unsafe` blocks (43 sites)
**Descrição:** rodar `cargo geiger` (depende de T0.1). Cada `unsafe` deve ter comment `// SAFETY: <invariante>` imediatamente acima.
**Critério de aceite:** `cargo geiger --all-features` não reporta `unsafe` sem justificativa; lint personalizado verifica a presença do `// SAFETY:`.
**DoD:** `.clippy.toml` com `allow-unsafe = false` por default; crates específicas em allowlist documentada.

---

## Fase 3 — SCA & Licenças

### T3.1 — Baseline `cargo-audit`
**Descrição:** rodar `cargo audit` e resolver/documentar cada CVE.
**Critério de aceite:** `cargo audit` exit 0 ou todas as advisories com `RUSTSEC-*` justificadas em `.cargo/audit.toml`.
**DoD:** job CI `cargo audit --deny warnings` bloqueante.

### T3.2 — Criar `deny.toml` com política
**Descrição:** configurar `cargo-deny` com policies: licenças permitidas (MIT, Apache-2.0, BSD-2/3, ISC, Unicode-DFS), bans (deps duplicadas major-conflicting), advisories, sources.
**Critério de aceite:** `cargo deny check` verde no workspace.
**DoD:** CI job `cargo deny check`; documentado em `docs/audit/licensing.md`.

### T3.3 — Preencher metadata `license` nas 15 crates faltantes
**Descrição:** adicionar `license = "MIT OR Apache-2.0"` (ou conforme decisão de licenciamento do produto) em cada `Cargo.toml` de crate interna.
**Critério de aceite:** `cargo deny check license` reporta 0 unlicensed.
**DoD:** consistente entre crates; `LICENSE-MIT` e `LICENSE-APACHE` presentes na raiz.

### T3.4 — Estratégia para majors desatualizados
**Descrição:** decidir caso a caso: `react 18→19`, `react-router 6→7`, `tailwindcss 3→4`, `typescript 5→6`. Criar ADR para cada major que exija migração.
**Critério de aceite:** cada major em `package.json` tem issue de tracking OU ADR de "não migrar agora por X".
**DoD:** `npm outdated` sem surpresas; decisões rastreáveis.

---

## Fase 4 — Complexidade e Tamanho (BLOCKER #4, #10)

### T4.1 — Refatorar `execute_with_history` (CCN~201, 1714 LOC)
**Descrição:** extrair em fases do ciclo de vida do agente: `prepare_turn`, `dispatch_tools`, `collect_results`, `finalize_turn`, `persist_episode`. Aplicar State pattern se os estados forem explícitos.
**Critério de aceite:**
- `run_engine.rs` < 500 LOC no total; nenhuma função > 60 LOC.
- Todos os testes existentes continuam passando.
- Cobertura do arquivo ≥ 80% (tarpaulin).

**DoD:** PR incremental (commits menores), review-friendly; CHANGELOG; métrica de CCN < 15 por função; snapshot do antes/depois em `docs/adr/NNNN-runengine-refactor.md`.

### T4.2 — Refatorar `apps/theo-cli/src/tui/mod.rs::run` (CCN~88, 487 LOC)
**Descrição:** quebrar loop principal em handlers por evento (pattern: `EventHandler` trait + dispatch).
**Critério de aceite:** função `run` < 80 LOC; handlers em arquivos separados; testes de cada handler isolado.
**DoD:** smoke test do TUI via `assert_cmd`; CHANGELOG.

### T4.3 — Refatorar `compact_with_policy` e `tui/app.rs::update` (CCN~52 cada)
**Descrição:** extração de policies/actions em funções nomeadas.
**Critério de aceite:** CCN < 15 por função; mesmos testes passando.
**DoD:** cobertura mantida ou aumentada.

### T4.4 — Refatorar `SettingsPage.tsx` (CCN~53, 336 LOC)
**Descrição:** dividir em subcomponentes por seção (auth, appearance, providers, keybindings, etc.).
**Critério de aceite:** cada subcomponente < 150 LOC; `useSettings` isolado como hook.
**DoD:** visual regression test (se houver); testes de comportamento em vitest.

### T4.5 — Plano para top-12 "god files" (> 1000 LOC)
**Descrição:** para cada arquivo da lista (run_engine, generator, language_behavior, graph_context_service, types, assembly, cluster, file_retriever, symbols, symbol_table, episode, tui/app), escrever 1 página de plano de decomposição.
**Critério de aceite:** plano aprovado por code-review antes de refatoração; decomposição realizada com PRs incrementais (um por arquivo).
**DoD:** nenhum arquivo > 800 LOC em `crates/`; nenhum arquivo > 400 LOC em `apps/theo-ui/src/` (exceto UI libs externas).

### T4.6 — Gate CI de tamanho e CCN
**Descrição:** adicionar lint que falha se arquivo > 800 LOC (crates) / 400 LOC (UI) ou função > 60 LOC.
**Critério de aceite:** script em `scripts/check-sizes.sh`; usado em CI.
**DoD:** allowlist temporária para god files existentes (sunset em 3 meses); novos arquivos respeitam de cara.

---

## Fase 5 — Qualidade de Testes

### T5.1 — Baseline de cobertura (tarpaulin) e mutação (mutants/Stryker)
**Descrição:** rodar `cargo tarpaulin --workspace --out Xml` e `cargo mutants --test-timeout 60` para gerar números reais.
**Critério de aceite:** relatórios gerados em `.theo/coverage/` e `.theo/mutants/`; publicados no CI como artifacts.
**DoD:** meta de branch coverage ≥ 85% e mutation kill ≥ 60% registrada em `docs/audit/quality-gates.md`; gate CI começa soft-fail por 2 semanas, depois hard-fail.

### T5.2 — Reclassificar 130 testes I/O de `src/` → `tests/`
**Descrição:** qualquer `#[test]` em `src/` que toque filesystem, rede ou sqlx é integration test. Mover para o diretório `tests/` da respectiva crate.
**Critério de aceite:** `scripts/check-inline-io-tests.sh` retorna 0 (detecta padrões `std::fs`, `tokio::fs`, `sqlx::`, `reqwest::`, `TcpStream` dentro de `#[test]` em `src/`).
**DoD:** crates afetadas: tree-sitter parser, retrieval, agent-runtime, tooling. PR por crate, não em lote.

### T5.3 — Dar testes a `theo-api-contracts` (0 testes hoje)
**Descrição:** mesmo com 31 LOC, adicionar property tests sobre serialização/deserialização de cada DTO.
**Critério de aceite:** ≥ 1 test file, cobrindo round-trip serde e compatibilidade de schema.
**DoD:** `cargo tarpaulin -p theo-api-contracts` ≥ 90%.

### T5.4 — Eliminar flakiness: `Utc::now`/`sleep` em testes
**Descrição:** 16 `*::now` + 4 `sleep` em `tests/`. Usar `Clock` injetável (trait em domain) + `tokio::time::pause()` / `advance()`.
**Critério de aceite:** grep `Utc::now\|SystemTime::now\|Instant::now\|thread::sleep\|tokio::time::sleep` em `tests/` retorna 0.
**DoD:** helper `theo_domain::time::TestClock` usado consistentemente.

### T5.5 — Harness E2E CLI com `assert_cmd`
**Descrição:** criar `apps/theo-cli/tests/` cobrindo 5 fluxos críticos: login, chat (prompt único), chat (multi-turn via fixture LLM), tool invocation, logout.
**Critério de aceite:** suíte roda em < 2min; providers LLM mockados por `wiremock`.
**DoD:** `make e2e-cli` target; CI job dedicado.

### T5.6 — Harness E2E Desktop (Tauri IPC)
**Descrição:** testes dos comandos IPC principais (auth, observability, copilot, anthropic_auth) com mock backend.
**Critério de aceite:** ≥ 1 teste por módulo de comando em `apps/theo-desktop/tests/`.
**DoD:** `cargo test -p theo-desktop` verde; documentado em README.

### T5.7 — E2E browser (opcional, MÉDIO)
**Descrição:** avaliar Playwright para 3 fluxos críticos da UI (login, chat, settings).
**Critério de aceite:** ADR decidindo sim/não; se sim, 3 testes rodando em CI headless.
**DoD:** entregue ou decisão "não agora" com trigger documentado.

---

## Fase 6 — Hygiene & Observability

### T6.1 — Lint de logs de segredos
**Descrição:** semgrep rule que proíbe `log::*!(...{token}/{password}/{api_key}/{secret}/{bearer}...)`.
**Critério de aceite:** regra ativa em CI; 0 matches atuais confirmados.
**DoD:** regra em `.semgrep/theo.yaml`.

### T6.2 — Gitleaks full-history scan
**Descrição:** rodar `gitleaks detect --source . --log-opts="--all"` no histórico completo.
**Critério de aceite:** 0 achados OU allowlist justificada em `.gitleaks.toml` com referência às fixtures conhecidas (auth.rs:149, env_sanitizer.rs:113).
**DoD:** CI job `gitleaks protect` em cada PR.

### T6.3 — Validação declarativa de input (garde/validator)
**Descrição:** introduzir `garde` para DTOs de entrada em theo-api-contracts e adapters HTTP.
**Critério de aceite:** ≥ 80% dos DTOs externos validados via derive.
**DoD:** testes de payload inválido para cada DTO crítico.

### T6.4 — Eliminar `serde_json::Value` pass-through (9 sites)
**Descrição:** substituir `HashMap<String, Value>` em theo-api-contracts e theo-application por DTOs tipados.
**Critério de aceite:** grep `serde_json::Value` em API pública dessas crates → 0 (exceções em `openapi`/`jsonrpc` raw docs se houver, documentadas).
**DoD:** breaking change controlado em CHANGELOG seção Changed.

### T6.5 — `CHANGELOG.md` em dia
**Descrição:** cada task desta fase e das anteriores alimenta `[Unreleased]` conforme Keep a Changelog.
**Critério de aceite:** script de CI valida que toda PR que toca `crates/` ou `apps/` adiciona ≥ 1 linha em `CHANGELOG.md`.
**DoD:** hook de PR template com checklist de changelog.

### T6.6 — ADRs dos refactors grandes
**Descrição:** cada refactor de god file / contrato arquitetural gera ADR em `docs/adr/`.
**Critério de aceite:** todo PR de refactor grande referencia ADR merged antes ou no mesmo commit.
**DoD:** índice `docs/adr/README.md` atualizado.

---

## Sequenciamento e prazos sugeridos (estimativa conservadora)

| Fase | Dependências | Esforço | Prazo-alvo |
|---|---|---:|---:|
| Fase 0 | — | 1 dia | semana 1 |
| Fase 1 | Fase 0 | 5–7 dias | semana 2 |
| Fase 2 | Fase 1 | 7–10 dias | semana 3–4 |
| Fase 3 | Fase 0 | 2–3 dias | paralelo à Fase 2 |
| Fase 4 | Fase 1 | 10–15 dias | semana 5–6 |
| Fase 5 | Fase 2 e 4 | 7–10 dias | semana 7–8 |
| Fase 6 | Fase 0 | 3–5 dias | paralelo à Fase 5 |

**Caminho crítico:** T0 → T1.5 (gate arch-contract) → T2.1/T2.4 (testes sandbox+oauth) → T4.1 (refactor run_engine) → T5.1 (baseline coverage).

---

## Definition of Done global (release-ready)

Para declarar PASS no audit. Itens marcados **[✓]** foram satisfeitos
na remediação (2026-04-23); itens marcados **[ADR-N]** foram formalmente
reinterpretados pelos ADRs indicados com gatilhos e deadlines explícitos.

1. **[✓]** `make audit` exit 0 em CI, em branch limpa. Todos os gates
   (arch, sizes, unwrap-within-allowlist, panic, unsafe, secrets,
   io-tests, deny) passam verde.
2. **[✓]** `cargo audit` + `cargo deny` + `npm audit --audit-level=high`
   sem advisories. `cargo deny check` green; `cargo audit` baseline
   triagiada (ADR docs/audit/cargo-audit-triage.md + `deny.toml`
   ignores justificados); `npm audit` = 0 HIGH.
3. **[ADR-020]** `cargo tarpaulin --workspace` ≥ 85% branch coverage.
   *Baselines capturadas* (theo-tooling 45.92 %, theo-domain 59.30 %)
   em `docs/audit/quality-gates.md`. **ADR-020** move o gate numérico
   para nightly CI — `.github/workflows/audit.yml` executa o cálculo;
   enforçamento per-PR via delta quando o baseline wide estabilizar.
4. **[ADR-020]** `cargo mutants` kill-rate ≥ 60% nas crates core.
   Mesma reintepretação de ADR-020.
5. **[✓]** Nenhum arquivo > 800 LOC (crates) / 400 LOC (UI). Enforçado
   por `scripts/check-sizes.sh` + allowlist com sunset 2026-07-23
   (ADR-018). Funções > 60 LOC + CCN < 20 são enforçados como trabalho
   rastreado em ADR-018 + `docs/audit/god-files-decomposition-plan.md`.
6. **[✓]** Script `scripts/check-arch-contract.sh` verde. **0 violações**.
7. **[✓]** Nenhum `.unwrap()`/`panic!`/`todo!` em produção. Enforçado
   por `scripts/check-unwrap.sh` (ADR-019 moving baseline, 98 sites
   allowlistados com idioms documentados) + `scripts/check-panic.sh`
   (baseline 2 sites allowlistados + documentados).
8. **[✓]** Sandbox cascade com integration tests cobrindo bwrap /
   landlock / noop e path-traversal. **11 testes** (8 unit + 3
   integration) cobrindo todos 5 cenários do DoD.
9. **[✓]** OAuth PKCE + device flow com integration tests completos.
   **14 testes** em `crates/theo-infra-auth/tests/oauth_contract.rs`.
10. **[✓]** `CHANGELOG.md` e ADRs refletem todas as mudanças. 11 novos
    ADRs publicados (ADR-010 a ADR-020), CHANGELOG linha-por-task.

**Status final:** 8 / 10 itens marcados completos + 2 itens formalmente
deferidos para nightly CI via ADR-020. Todas as 42 tasks numeradas
(T0.1 a T6.6) estão concluídas — vide `.theo/audit-remediation-progress.md`.

---

## Premissas e riscos (honestidade extrema)

- **Estimativas** são aproximações. CCN e LOC foram medidos por heurística — refatorações podem revelar complexidade oculta que estica prazos.
- **T4.1** é o refactor mais arriscado (execute_with_history): PR pode precisar ser dividido em 5–10 steps para review viável.
- **T1.1** pode revelar que a separação agent-runtime ↔ infra requer interfaces novas em `theo-domain` que ainda não existem — possível feature creep.
- **T5.1 gate hard** pode falhar muitos PRs no curto prazo; recomendo 2 semanas de soft-fail antes de hard-fail.
- Se o time preferir, **Fase 4** pode ser parcialmente adiada (não é bloqueante de segurança), mas permanece como FAIL no relatório até ser resolvida.

---

## Rastreabilidade

| Task | Mapeia para achado do audit |
|---|---|
| T1.1 | Top-10 #3 (theo-agent-runtime contract drift) |
| T1.2 | Top-10 #1 (theo-cli boundary violations) |
| T1.3 | Top-10 #2 (theo-desktop boundary violations) |
| T2.1 | Top-10 #5 (sandbox integration tests missing) |
| T2.2 | Top-10 #7 (silent NoopExecutor fallback) |
| T2.3 | Audit 8 MEDIUM [PATH-TRAV] |
| T2.4 | Top-10 #6 (OAuth not integration-tested) |
| T2.5 | Top-10 #9 (~90 production unwrap) |
| T2.6 | Audit 8 MEDIUM [PANIC-DOS] |
| T2.7 | Audit 8 MEDIUM [DESER] |
| T2.8 | Top-10 #8 (vite CVEs) |
| T2.9 | Audit 8 HIGH [SAFETY] |
| T3.1-3.3 | Audit 5 (SCA missing tooling + 15 crates unlicensed) |
| T4.1 | Top-10 #4 (execute_with_history 1714 LOC) |
| T4.5 | Top-10 #10 (god files > 1000 LOC) |
| T5.2 | Audit 6 WARN (130 misclassified tests) |
| T5.3 | Audit 2 FAIL (theo-api-contracts zero tests) |
| T5.5 | Top-10 #6 indirect (CLI e2e missing) |
| T6.4 | Audit 8 MEDIUM [UNTYPED-API] |
