# SOTA Criteria — Auto-Evolution SOTA (cycle evolution/apr22-1618)

**Target:** Implement `docs/plans/PLAN_AUTO_EVOLUTION_SOTA.md` to reach "Auto-Evolution: Theo ≥ Hermes + Claude Code autodream".

---

## Global Success Criteria (from plan)

| # | Criterion | Hermes today | Theo target |
|---|-----------|--------------|-------------|
| S1 | Memory reviewer autônomo | `_turns_since_memory >= 10` → spawn thread | Idem via `tokio::spawn` + `AtomicUsize` (elimina Issue #8506) |
| S2 | Skill generator autônomo | 5+ tool calls sem skill → reviewer | Idem via EventBus counter |
| S3 | Safety scan skills geradas | ~80 regex patterns | Reuso `security::scan()` + novos patterns skill-specific |
| S4 | Autodream (consolidation) | — | LLM-driven pós-sessão + 24h cooldown (OpenDev pattern) |
| S5 | Histórico pesquisável | SQLite FTS5 | Tantivy persistente com tipo `transcript` |
| S6 | Onboarding proativo | — (OpenClaw only) | BOOTSTRAP.md Q&A na primeira sessão |
| S7 | Skill auto-improvement | Prompt-driven | Prompt-driven (mesmo approach) |
| S8 | Cross-session search completa | Transcript-level | Transcript + summary level (Tantivy unified) |

---

## Per-Phase Acceptance Criteria

### Phase 1 — Nudge Counter + Memory Reviewer Background

- **AC-1.1**: Contador `turns_since_memory_review` incrementa a cada turno completo (via `fetch_add(1, Relaxed)`).
- **AC-1.2**: Ao atingir `memory_review_nudge_interval`, reviewer é spawned via `tokio::spawn` (não bloqueia o loop).
- **AC-1.3**: Counter reseta para 0 após spawn.
- **AC-1.4**: `interval = 0` desabilita completamente o mecanismo.
- **AC-1.5**: Falha do reviewer é logada mas nunca propaga para o main loop.
- **AC-1.6**: Recent turns passados ao reviewer incluem ao menos as últimas `min(interval, 20)` mensagens.
- **AC-1.7**: Reviewer forked tem `memory_review_nudge_interval = 0` (anti-recursão — padrão Hermes `run_agent.py:2820`).

### Phase 2 — Autodream Daemon

- **AC-2.1**: `AutodreamExecutor::consolidate` chamado ao **início** da sessão (não ao fim — adotando OpenDev vs plan-original).
- **AC-2.2**: Respeita `autodream_timeout_secs` (default 60s). Timeout não panica.
- **AC-2.3**: Memórias `stale` são reescritas/removidas via LLM.
- **AC-2.4**: Toda memória consolidada passa por `security::scan()` antes de persistir.
- **AC-2.5**: Falha/timeout logado via `tracing::warn!`, não bloqueia shutdown.
- **AC-2.6**: `autodream_enabled = false` desabilita.
- **AC-2.7**: Lock file `.consolidation.lock` previne execução concorrente.
- **AC-2.8**: 24h cooldown entre execuções (OpenDev pattern).
- **AC-2.9**: Backup `.bak` criado antes de mutação (OpenDev `files_backed_up` counter).

### Phase 3 — Skill Generator + skill_manage Tool

- **AC-3.1**: Counter de tool calls reseta ao início de cada task (novo user turn após resposta final).
- **AC-3.2**: Spawn do skill reviewer em `count >= 5 && !skill_created`.
- **AC-3.3**: `skill_manage` tool expõe 5 operações: create/patch/edit/delete/supporting_file.
- **AC-3.4**: Body de skill passa por `scan_skill_body` antes de persistir.
- **AC-3.5**: Política de origin: `community=BLOCK`, `agent=ASK/BLOCK-if-critical`, `user=WARN`.
- **AC-3.6**: Frontmatter YAML parser lê/escreve `origin` field.
- **AC-3.7**: System prompt instrui auto-improvement de skills.

### Phase 4 — Tantivy Persistente

- **AC-4.1**: `MemoryTantivyIndex::open_or_create(&Path)` persiste via `MmapDirectory`.
- **AC-4.2**: Schema suporta `source_type = "transcript"` com `session_id`, `turn_index`, `timestamp_unix`.
- **AC-4.3**: `on_session_end` indexa transcripts automaticamente.
- **AC-4.4**: Re-indexação com mesmo hash é no-op (idempotente).
- **AC-4.5**: Tool `memory_search` implementa 3 tiers.
- **AC-4.6**: Index sobrevive restart do processo.
- **AC-4.7**: BM25 scoring cross-session (queries recuperam msgs de sessões antigas).

### Phase 5 — Onboarding + Auto-improvement

- **AC-5.1**: `needs_bootstrap` retorna true quando `USER.md` ausente ou < 50 chars.
- **AC-5.2**: Na primeira sessão, `BOOTSTRAP_PROMPT` prepended ao system message.
- **AC-5.3**: Q&A coleta 4 tópicos (role, preferences, boundaries, language).
- **AC-5.4**: `UserProfile` serializa/deserializa markdown com frontmatter YAML.
- **AC-5.5**: Após `USER.md` populado, `needs_bootstrap` retorna false.
- **AC-5.6**: Auto-improvement reminder injetado a cada N prompts do usuário.

---

## SOTA Rubric (5 dimensions, score 0-3 per phase)

| Dimension | Signal |
|---|---|
| **Pattern Fidelity** | Does the implementation mirror the SOTA reference pattern? Cite specific file/lines. |
| **Architectural Fit** | Respects theo-code bounded contexts + dependency graph? |
| **Completeness** | Production-ready error handling + graceful degradation? |
| **Testability** | Meaningful tests (not just happy path)? Regression protection? |
| **Simplicity** | Minimal surface area? Avoids unnecessary abstractions? |

**Convergence**: average ≥ 2.5 per phase AND all ACs green.

---

## DoD Global (from plan)

- [ ] All 5 phases with DoD individual checked.
- [ ] `cargo build --workspace --exclude theo-code-desktop` — 0 warnings.
- [ ] `cargo clippy --workspace --exclude theo-code-desktop --all-targets` — 0 warnings.
- [ ] `cargo test --workspace --exclude theo-code-desktop` — baseline 3046 + ~28 new = ~3074, 0 failed.
- [ ] E2E: bootstrap triggers, reviewer spawns at 10 turns, skill reviewer at 5 tool calls, `memory_search` returns hits, autodream runs.
- [ ] `docs/current/memory-architecture.md` updated.
- [ ] `CHANGELOG.md` 5 entries (one per phase).
- [ ] `docs/adr/009-auto-evolution-sota.md` written.
- [ ] Benchmarks: reviewer spawn < 10ms, autodream < 30s typical, Tantivy search cross-session < 50ms.
- [ ] Zero regression in existing benchmarks (MRR, compression ratio).

---

## Hygiene Floor

- `theo-evaluate.sh` score must not decrease from baseline.
- If decreases: `git reset --hard BEFORE_SHA` and re-plan.
- Max 5 consecutive reverts → re-read references.
