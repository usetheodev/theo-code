# Plano: Auto-Evolução SOTA — Theo Ahead of Hermes (Phase II)

> **Status**: DRAFT — 2026-04-22
> **Baseline**: `PLAN_MEMORY_SUPERIORITY.md` (APPROVED) + `PLAN_CONTEXT_WIRING.md` (DRAFT) wired.
> **Referência SOTA**: Claude Code autodream, OpenClaw bootstrap, Hermes Agent (nudge counter + skill generator).
> **Decisão técnica**: Tantivy (feature-gated, já disponível) em vez de SQLite+FTS5.
> **Estimativa total**: ~1020 LOC, ~28 testes novos, 5 fases.

---

## Diagnóstico

```
O que EXISTE hoje (inventário 2026-04-22):           O que FALTA para SOTA:
──────────────────────────────────────────           ──────────────────────────
MemoryProvider + 4 hooks wired ✓                     Nudge counter + background reviewer ✗
7-gate lesson chain + Laplace smoothing ✓            Autodream pós-sessão (LLM consolidation) ✗
EpisodeSummary + Hypothesis + TTL ✓                  Skill generator autônomo (5+ tool calls) ✗
BuiltinMemoryProvider + security scan ✓              Tantivy index persistente para transcripts ✗
skill_catalog two-tier (list/view) ✓                 Onboarding proativo (BOOTSTRAP.md) ✗
SessionSummary cold-start handoff ✓                  Skill auto-improvement in-place prompts ✗
MemGPT 3-tier decay ✓                                skill_manage tool (create/patch/edit) ✗
EventBus + RetrievalExecuted ✓                       Tool-call counter per task ✗
MemoryTantivyIndex em RAM (feature-gated) ✓          Tantivy persistence em disco ✗
FsSessionSearch (keyword + recency) ✓                Transcript indexing (incremental) ✗
```

**Distância para SOTA: ~1020 LOC de novos componentes + wiring em `run_engine.rs` e `memory_lifecycle.rs`.**

---

## Critério de Sucesso Global

| # | Critério | Hermes hoje | Theo target (este plano) |
|---|----------|-------------|--------------------------|
| S1 | Memory reviewer autônomo | `_turns_since_memory >= 10` → spawn | Igual, via `tokio::spawn` |
| S2 | Skill generator autônomo | 5+ tool calls sem skill → reviewer | Igual, via EventBus counter |
| S3 | Safety scan skills geradas | ~80 regex patterns | Reuso `security::scan()` + novos patterns specíficos de skill |
| S4 | Autodream (consolidation pós-sessão) | Claude Code only | LLM-driven via `on_session_end` async |
| S5 | Histórico pesquisável | SQLite FTS5 | Tantivy persistente com tipo `transcript` |
| S6 | Onboarding proativo | OpenClaw only | BOOTSTRAP.md Q&A na primeira sessão |
| S7 | Skill auto-improvement | Prompt-driven (Hermes) | Prompt-driven (mesmo approach) |
| S8 | Cross-session search completa | Transcript-level | Transcript-level (Tantivy) + summary-level (existente) |

---

## Estrutura de Fases

```
Phase 1 — Nudge Counter + Memory Reviewer Background          (~150 LOC)
Phase 2 — Autodream Daemon (LLM-driven consolidation)         (~200 LOC)
Phase 3 — Skill Generator Autônomo + skill_manage tool        (~300 LOC)
Phase 4 — Tantivy Persistente para Transcripts                (~400 LOC)
Phase 5 — Onboarding Proativo + Auto-improvement Prompts      (~170 LOC)
                                                               ─────────────
                                                               ~1020 LOC
```

Cada fase é independentemente mergeável e passa todos os testes isolada.

---

## Phase 1: Nudge Counter + Memory Reviewer Background

> **Objetivo**: após N turns (default: 10), spawnar sub-agente background que revê conversa recente e extrai fatos para USER.md/MEMORY.md sem bloquear o loop principal.
>
> **Dependências**: `memory_lifecycle.rs` (existe), `EventBus` (existe), `MemoryProvider::sync_turn` (existe).

### Task 1.1: Campo `_turns_since_memory` no `RunEngine`

**Arquivo**: `crates/theo-agent-runtime/src/run_engine.rs`

**Mudança**: adicionar contador de turnos interno ao state do engine. Incrementado a cada turno completo (após `execute` ou `execute_with_history`).

```rust
pub struct RunEngine {
    // ...existing fields...
    turns_since_memory_review: AtomicUsize,
}
```

### Task 1.2: Config `memory_review_nudge_interval` em `AgentConfig`

**Arquivo**: `crates/theo-agent-runtime/src/config.rs`

**Mudança**: novo campo `pub memory_review_nudge_interval: usize` com default `10`. Se `0`, desabilita o nudge.

```rust
pub struct AgentConfig {
    // ...existing...
    /// Number of turns before spawning background memory reviewer.
    /// Set to 0 to disable. Default: 10 (Hermes SOTA).
    pub memory_review_nudge_interval: usize,
}
```

### Task 1.3: Trait `MemoryReviewer` + implementação stub

**Arquivo novo**: `crates/theo-agent-runtime/src/memory_reviewer.rs`

**Mudança**: novo trait assíncrono para revisores de memória, com implementação stub `NullMemoryReviewer` e concreta `LlmMemoryReviewer` (usa o LLM provider do config).

```rust
#[async_trait]
pub trait MemoryReviewer: Send + Sync {
    /// Review recent turns and persist extracted facts to USER.md/MEMORY.md.
    /// Non-blocking from caller's perspective; errors logged, never bubbled.
    async fn review(&self, recent_turns: &[Message]) -> Result<usize, MemoryReviewError>;
}

pub struct NullMemoryReviewer;
pub struct LlmMemoryReviewer { /* inner */ }
```

### Task 1.4: Spawn background reviewer em `memory_lifecycle.rs`

**Arquivo**: `crates/theo-agent-runtime/src/memory_lifecycle.rs`

**Mudança**: nova função `maybe_spawn_reviewer(cfg, turns_counter, recent_messages)` chamada dentro de `sync_turn`. Se counter atinge `nudge_interval`, `tokio::spawn` o reviewer e reseta counter.

```rust
pub async fn maybe_spawn_reviewer(
    cfg: &AgentConfig,
    turns_counter: &AtomicUsize,
    recent_turns: Vec<Message>,
) {
    let interval = cfg.memory_review_nudge_interval;
    if interval == 0 { return; }
    let current = turns_counter.fetch_add(1, Ordering::Relaxed) + 1;
    if current < interval { return; }
    turns_counter.store(0, Ordering::Relaxed);

    if let Some(reviewer) = &cfg.memory_reviewer {
        let reviewer = reviewer.clone();
        tokio::spawn(async move {
            if let Err(e) = reviewer.review(&recent_turns).await {
                tracing::warn!(error = ?e, "memory reviewer failed");
            }
        });
    }
}
```

### Task 1.5: Testes de integração

**Arquivo novo**: `crates/theo-agent-runtime/tests/memory_reviewer_nudge.rs`

```rust
#[tokio::test]
async fn test_reviewer_spawns_after_nudge_interval() { /* ... */ }

#[tokio::test]
async fn test_reviewer_does_not_spawn_before_interval() { /* ... */ }

#[tokio::test]
async fn test_reviewer_disabled_when_interval_zero() { /* ... */ }

#[tokio::test]
async fn test_reviewer_failure_does_not_crash_main_loop() { /* ... */ }

#[tokio::test]
async fn test_reviewer_counter_resets_after_spawn() { /* ... */ }
```

### Critérios de Aceite Phase 1

- **AC-1.1**: Contador `turns_since_memory_review` incrementa a cada turno completo.
- **AC-1.2**: Ao atingir `memory_review_nudge_interval`, reviewer é spawned via `tokio::spawn` (não bloqueia).
- **AC-1.3**: Counter reseta para 0 após spawn.
- **AC-1.4**: `interval = 0` desabilita completamente o mecanismo.
- **AC-1.5**: Falha do reviewer é logada mas nunca propaga para o main loop.
- **AC-1.6**: Recent turns passados ao reviewer incluem ao menos as últimas `min(interval, 20)` mensagens.

### DoD Phase 1

- [ ] Trait `MemoryReviewer` definido em `memory_reviewer.rs` com 2 implementações (Null + Llm stub).
- [ ] Campo `memory_review_nudge_interval` em `AgentConfig` (default 10).
- [ ] `maybe_spawn_reviewer` chamado em `sync_turn` path.
- [ ] 5 testes passando em `memory_reviewer_nudge.rs`.
- [ ] `cargo test -p theo-agent-runtime` 0 falhas.
- [ ] `cargo clippy --workspace` 0 warnings.
- [ ] Documentação inline: por que 10 turns, referências Hermes Issue #8506 (bug de reset).
- [ ] CHANGELOG.md atualizado em `[Unreleased] Added`.

---

## Phase 2: Autodream Daemon — LLM-driven Consolidation

> **Objetivo**: pós-sessão, spawnar daemon assíncrono que re-lê memórias existentes, detecta obsolescência e consolida via LLM — padrão Claude Code autodream.
>
> **Dependências**: Phase 1 (trait pattern estabelecido), `on_session_end` hook (existe), LLM provider (existe).

### Task 2.1: Trait `AutodreamExecutor` + implementação

**Arquivo novo**: `crates/theo-agent-runtime/src/autodream.rs`

```rust
#[async_trait]
pub trait AutodreamExecutor: Send + Sync {
    /// Post-session consolidation: read existing memories, detect stale
    /// entries, rewrite/merge via LLM. Non-blocking; safe to drop.
    async fn consolidate(&self, session_id: &str) -> Result<ConsolidationReport, AutodreamError>;
}

pub struct ConsolidationReport {
    pub memories_reviewed: usize,
    pub memories_updated: usize,
    pub memories_removed: usize,
    pub duration_ms: u64,
}

pub struct LlmAutodreamExecutor { /* provider, memory_path */ }
```

### Task 2.2: Config `autodream_enabled` + timeout

**Arquivo**: `crates/theo-agent-runtime/src/config.rs`

```rust
pub struct AgentConfig {
    // ...existing...
    /// Enable post-session autodream consolidation. Default: true.
    pub autodream_enabled: bool,
    /// Max wall time for autodream run before abort. Default: 60s.
    pub autodream_timeout_secs: u64,
    /// Autodream executor (injected at application layer).
    pub autodream: Option<AutodreamHandle>,
}
```

### Task 2.3: Staleness detector

**Arquivo**: `crates/theo-agent-runtime/src/autodream.rs`

**Mudança**: função pura `detect_stale_memories(memories, session_summary) -> Vec<StalenessReason>`. Reasons: `ContradictedByNewEvidence`, `SupersededByLesson`, `ExpiredTtl`, `UnreferencedForN`.

```rust
pub enum StalenessReason {
    ContradictedByNewEvidence { memory_id: String, evidence_event: String },
    SupersededByLesson { memory_id: String, lesson_id: String },
    ExpiredTtl { memory_id: String },
    UnreferencedForN { memory_id: String, turns_unused: u64 },
}

pub fn detect_stale_memories(
    memories: &[MemoryEntry],
    summary: &SessionSummary,
    current_turn: u64,
) -> Vec<StalenessReason> { /* ... */ }
```

### Task 2.4: Spawn em `on_session_end`

**Arquivo**: `crates/theo-agent-runtime/src/memory_lifecycle.rs`

**Mudança**: após `record_session_exit_public`, se `cfg.autodream_enabled`, spawn tarefa com timeout. Não aguarda — fire-and-forget.

```rust
pub async fn on_session_end(cfg: &AgentConfig, /* ... */) {
    // ...existing shutdown logic...

    if cfg.autodream_enabled
        && let Some(executor) = &cfg.autodream {
            let executor = executor.clone();
            let timeout = Duration::from_secs(cfg.autodream_timeout_secs);
            let session_id = /* ... */;
            tokio::spawn(async move {
                match tokio::time::timeout(timeout, executor.consolidate(&session_id)).await {
                    Ok(Ok(report)) => tracing::info!(?report, "autodream completed"),
                    Ok(Err(e)) => tracing::warn!(error = ?e, "autodream failed"),
                    Err(_) => tracing::warn!("autodream timed out"),
                }
            });
        }
}
```

### Task 2.5: Safety guard — autodream não escreve se scan falha

**Arquivo**: `crates/theo-agent-runtime/src/autodream.rs`

**Mudança**: antes de persistir memória consolidada, passar por `theo_infra_memory::security::scan()`. Se detectar padrão, aborta consolidação daquela entrada (log + skip, não panic).

### Task 2.6: Testes de integração

**Arquivo novo**: `crates/theo-agent-runtime/tests/autodream_integration.rs`

```rust
#[tokio::test]
async fn test_autodream_spawns_on_session_end_when_enabled() { /* ... */ }

#[tokio::test]
async fn test_autodream_respects_timeout() { /* ... */ }

#[tokio::test]
async fn test_autodream_skips_memories_that_fail_safety_scan() { /* ... */ }

#[tokio::test]
async fn test_staleness_detector_flags_contradicted_evidence() { /* ... */ }

#[tokio::test]
async fn test_staleness_detector_flags_expired_ttl() { /* ... */ }

#[tokio::test]
async fn test_autodream_disabled_skips_spawn() { /* ... */ }
```

### Critérios de Aceite Phase 2

- **AC-2.1**: `AutodreamExecutor::consolidate` é chamado fire-and-forget em `on_session_end`.
- **AC-2.2**: Execution respeita `autodream_timeout_secs` (default 60s). Timeout não panica.
- **AC-2.3**: Memórias detectadas como stale são reescritas/removidas pelo LLM.
- **AC-2.4**: Toda memória consolidada passa por `security::scan()` antes de persistir.
- **AC-2.5**: Falha/timeout de autodream é logada mas não bloqueia shutdown.
- **AC-2.6**: `autodream_enabled = false` desabilita completamente.

### DoD Phase 2

- [ ] Trait `AutodreamExecutor` + `LlmAutodreamExecutor` implementados.
- [ ] Staleness detector com 4 `StalenessReason` variants.
- [ ] Spawn integrado em `on_session_end` com `tokio::time::timeout`.
- [ ] Safety scan obrigatório antes de persistir.
- [ ] 6 testes passando em `autodream_integration.rs`.
- [ ] `ConsolidationReport` telemetria emitida via `tracing::info!`.
- [ ] CHANGELOG.md atualizado.

---

## Phase 3: Skill Generator Autônomo + skill_manage Tool

> **Objetivo**: após 5+ tool calls em uma task sem skill criada, spawnar sub-agente que avalia se vale criar/atualizar skill. Expõe `skill_manage` tool com `create/patch/edit/delete`.
>
> **Dependências**: `skill_catalog.rs` (existe, 274 LOC), `security::scan()` (existe), EventBus (existe).

### Task 3.1: Contador de tool calls por task

**Arquivo**: `crates/theo-agent-runtime/src/run_engine.rs`

**Mudança**: campo `tool_calls_in_task: AtomicUsize` resetado no início de cada task (quando há novo user prompt após resposta final). Incrementado a cada `ToolExecuted` event.

```rust
pub struct RunEngine {
    // ...existing...
    tool_calls_in_task: AtomicUsize,
    skill_created_in_task: AtomicBool,
}
```

### Task 3.2: Trait `SkillReviewer` + implementação

**Arquivo novo**: `crates/theo-agent-runtime/src/skill_reviewer.rs`

```rust
#[async_trait]
pub trait SkillReviewer: Send + Sync {
    /// Review task conversation and decide whether to create/update a skill.
    /// Returns None if no skill is warranted.
    async fn review(
        &self,
        conversation: &[Message],
        existing_skills: &[SkillMetadata],
    ) -> Result<Option<SkillAction>, SkillReviewError>;
}

pub enum SkillAction {
    Create { name: String, category: String, body: String },
    Patch { name: String, diff: String },
    Edit { name: String, new_body: String },
}
```

### Task 3.3: `skill_manage` tool

**Arquivo novo**: `crates/theo-tooling/src/skill_manage/mod.rs`

**Mudança**: novo tool conforme `Tool` trait. Aceita operation (`create/patch/edit/delete/supporting_file`) + args. Delega para `SkillCatalog::{create,patch,edit,delete}`.

```rust
pub struct SkillManageTool;

impl Tool for SkillManageTool {
    fn schema(&self) -> ToolSchema { /* ... */ }
    fn category(&self) -> ToolCategory { ToolCategory::Meta }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        // 1. Parse operation.
        // 2. Safety scan body (for create/edit/patch).
        // 3. Delegate to SkillCatalog.
        // 4. Return structured result.
    }
}
```

### Task 3.4: Safety scan específico de skill

**Arquivo**: `crates/theo-infra-memory/src/security.rs`

**Mudança**: nova função `scan_skill_body(body)` com padrões adicionais:
- Shell: `rm -rf /`, `mkfs`, `dd` em partições
- Exfil: `curl`/`wget` com `$API_KEY|$TOKEN|$SECRET`
- Persistence: `~/.ssh/authorized_keys`, `crontab -e`, systemd hijack
- Crypto mining patterns
- Reverse shell patterns (bash -i, nc -e)

```rust
pub fn scan_skill_body(body: &str) -> Result<(), SecurityViolation> {
    scan(body)?; // reuso de todos os padrões existentes
    scan_destructive_commands(body)?;
    scan_credential_exfil(body)?;
    scan_persistence_patterns(body)?;
    scan_reverse_shells(body)?;
    Ok(())
}
```

### Task 3.5: Origem de skill + política de aprovação

**Arquivo**: `crates/theo-agent-runtime/src/skill_catalog.rs`

**Mudança**: estender frontmatter YAML com campo `origin: agent | community | user`. Política:
- `origin: community` + padrão perigoso → BLOCK (rejeição automática)
- `origin: agent` + padrão perigoso → ASK (permission prompt ao usuário)
- `origin: user` → passa (confiança explícita)

### Task 3.6: Spawn do skill reviewer via EventBus

**Arquivo**: `crates/theo-agent-runtime/src/memory_lifecycle.rs`

**Mudança**: assinar `ToolExecuted` event. A cada event, incrementar counter. Se atinge 5 e `skill_created_in_task` é false, spawn reviewer background.

```rust
impl EventListener for SkillReviewerTrigger {
    fn on_event(&self, event: &DomainEvent) {
        if event.event_type != EventType::ToolExecuted { return; }
        let count = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
        if count >= 5 && !self.skill_created.load(Ordering::Relaxed) {
            let reviewer = self.reviewer.clone();
            let conversation = self.recent_messages();
            tokio::spawn(async move {
                match reviewer.review(&conversation, &[]).await {
                    Ok(Some(action)) => { /* delegate to SkillManage */ }
                    _ => {}
                }
            });
        }
    }
}
```

### Task 3.7: Auto-improvement prompt no system prompt

**Arquivo**: `crates/theo-agent-runtime/src/prompts/system.rs` (ou onde system prompt é construído)

**Mudança**: adicionar ao system prompt (após carga de skills):

```
When using a skill and finding it outdated, incomplete, or wrong,
patch it immediately using skill_manage. Don't wait to be asked.
Skills that are unmaintained become liabilities.
```

### Task 3.8: Testes de integração

**Arquivo novo**: `crates/theo-agent-runtime/tests/skill_generator_integration.rs`

```rust
#[tokio::test]
async fn test_skill_reviewer_spawns_after_5_tool_calls() { /* ... */ }

#[tokio::test]
async fn test_skill_reviewer_does_not_spawn_if_skill_already_created() { /* ... */ }

#[tokio::test]
async fn test_skill_manage_create_passes_security_scan() { /* ... */ }

#[tokio::test]
async fn test_skill_manage_blocks_dangerous_body() { /* ... */ }

#[tokio::test]
async fn test_skill_with_community_origin_rejected_on_pattern() { /* ... */ }

#[tokio::test]
async fn test_skill_with_agent_origin_prompts_user_on_pattern() { /* ... */ }

#[tokio::test]
async fn test_counter_resets_between_tasks() { /* ... */ }
```

### Critérios de Aceite Phase 3

- **AC-3.1**: Counter de tool calls reseta ao início de cada task (novo user turn após resposta final).
- **AC-3.2**: Spawn do skill reviewer ocorre em `count >= 5 && !skill_created`.
- **AC-3.3**: `skill_manage` tool implementa 5 operações: create/patch/edit/delete/supporting_file.
- **AC-3.4**: Todo body de skill passa por `scan_skill_body` antes de persistir.
- **AC-3.5**: Política de aprovação baseada em `origin` respeita: community=BLOCK, agent=ASK, user=PASS.
- **AC-3.6**: Frontmatter YAML parser lê/escreve `origin` field.
- **AC-3.7**: System prompt instrui auto-improvement de skills.

### DoD Phase 3

- [ ] `SkillReviewer` trait + implementação LLM concreta.
- [ ] `skill_manage` tool registrado em `create_default_registry`.
- [ ] `scan_skill_body` com 5 categorias de padrões perigosos.
- [ ] `SkillOrigin` enum + política de aprovação.
- [ ] 7 testes passando em `skill_generator_integration.rs`.
- [ ] Auto-improvement instruction no system prompt.
- [ ] `skill_catalog.rs` estende frontmatter com `origin`, `created`, `updated`.
- [ ] CHANGELOG.md atualizado.

---

## Phase 4: Tantivy Persistente para Transcripts

> **Objetivo**: elevar `MemoryTantivyIndex` de RAM para disco, indexar transcripts completos de conversa (não só summaries), expor via tool `memory_search`.
>
> **Dependências**: `memory_tantivy.rs` (existe, 276 LOC, RAM-only), Tantivy já em workspace.
> **Decisão arquitetural**: Tantivy (não SQLite). Justificativa: já temos a crate, já temos feature flag, BM25 nativo é melhor que FTS5 para cross-session semantic search.

### Task 4.1: Migrar `MemoryTantivyIndex` de RAM para disco

**Arquivo**: `crates/theo-engine-retrieval/src/memory_tantivy.rs`

**Mudança**: substituir `Index::create_in_ram(schema)` por `Index::open_or_create(MmapDirectory::open(path), schema)`. Nova assinatura aceita `index_dir: &Path`.

```rust
impl MemoryTantivyIndex {
    pub fn open_or_create(index_dir: &Path) -> Result<Self, tantivy::TantivyError> {
        std::fs::create_dir_all(index_dir)?;
        let dir = MmapDirectory::open(index_dir)?;
        let schema = Self::build_schema();
        let index = Index::open_or_create(dir, schema)?;
        // ...same tokenizer setup...
    }

    pub fn commit(&mut self) -> Result<(), tantivy::TantivyError> { /* ... */ }
}
```

### Task 4.2: Novo `source_type = "transcript"` + schema estendido

**Arquivo**: `crates/theo-engine-retrieval/src/memory_tantivy.rs`

**Mudança**: adicionar campos ao schema: `session_id` (STRING), `turn_index` (U64), `timestamp_unix` (U64). Tipo `source_type: "transcript"` reservado para messages completas.

```rust
pub struct TranscriptDoc {
    pub session_id: String,
    pub turn_index: u64,
    pub timestamp_unix: u64,
    pub role: String,     // user | assistant | tool
    pub body: String,     // full message content
}

impl MemoryTantivyIndex {
    pub fn add_transcript(&mut self, doc: TranscriptDoc) -> Result<(), tantivy::TantivyError> { /* ... */ }
    pub fn search_transcripts(&self, query: &str, limit: usize) -> Result<Vec<TranscriptHit>, _> { /* ... */ }
}
```

### Task 4.3: Indexer incremental em `on_session_end`

**Arquivo**: `crates/theo-agent-runtime/src/memory_lifecycle.rs`

**Mudança**: após shutdown, indexar mensagens da sessão. Usar hash do transcript para skip se não mudou (SHA-256 nos últimos N msgs).

```rust
pub async fn on_session_end(cfg: &AgentConfig, messages: &[Message]) {
    // ...existing logic...

    if let Some(index) = &cfg.transcript_index {
        let session_id = /* ... */;
        let hash = compute_transcript_hash(messages);
        if !index.contains_session_with_hash(&session_id, &hash) {
            for (i, msg) in messages.iter().enumerate() {
                let doc = TranscriptDoc {
                    session_id: session_id.clone(),
                    turn_index: i as u64,
                    timestamp_unix: msg.timestamp_unix().unwrap_or_default(),
                    role: msg.role.to_string(),
                    body: msg.content.clone().unwrap_or_default(),
                };
                let _ = index.add_transcript(doc);
            }
            let _ = index.commit();
        }
    }
}
```

### Task 4.4: Tool `memory_search` (3-tier expansion)

**Arquivo novo**: `crates/theo-tooling/src/memory_search/mod.rs`

**Mudança**: implementa o 3-tier do memsearch:

```rust
pub struct MemorySearchTool;

impl Tool for MemorySearchTool {
    async fn execute(&self, args: Value, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        let mode = args["mode"].as_str().unwrap_or("search");
        match mode {
            "search" => {
                // Tier 1: BM25 search, return ranked chunks (hash + snippet)
                let query = args["query"].as_str().ok_or(ToolError::Validation("query required".into()))?;
                let hits = index.search_transcripts(query, 10)?;
                Ok(serde_json::to_value(hits)?.into())
            }
            "get" => {
                // Tier 2: Expand chunk hash to full message content
                let chunk_hash = args["chunk_hash"].as_str().ok_or_else(|| ToolError::Validation("chunk_hash required".into()))?;
                let msg = index.get_by_hash(chunk_hash)?;
                Ok(serde_json::to_value(msg)?.into())
            }
            "transcript" => {
                // Tier 3: Return full session transcript by session_id
                let session_id = args["session_id"].as_str().ok_or_else(|| ToolError::Validation("session_id required".into()))?;
                let msgs = index.get_session_messages(session_id)?;
                Ok(serde_json::to_value(msgs)?.into())
            }
            _ => Err(ToolError::Validation(format!("unknown mode: {mode}"))),
        }
    }
}
```

### Task 4.5: Config `transcript_index_path` em `AgentConfig`

**Arquivo**: `crates/theo-agent-runtime/src/config.rs`

```rust
pub struct AgentConfig {
    // ...existing...
    /// Path to persistent Tantivy index. Default: `$XDG_DATA_HOME/theo/transcripts/`.
    pub transcript_index_path: Option<PathBuf>,
    pub transcript_index: Option<TranscriptIndexHandle>,
}
```

### Task 4.6: Content hash para evitar re-indexação

**Arquivo**: `crates/theo-engine-retrieval/src/memory_tantivy.rs`

**Mudança**: nova função `contains_session_with_hash(session_id, hash)` que verifica se sessão já foi indexada com o mesmo hash. Evita work duplicado em sessões retomadas.

### Task 4.7: Testes de integração

**Arquivo novo**: `crates/theo-engine-retrieval/tests/tantivy_transcript_integration.rs`

```rust
#[test]
fn test_index_persists_to_disk() { /* tempdir, close, reopen, search */ }

#[test]
fn test_search_transcripts_ranks_by_bm25() { /* ... */ }

#[test]
fn test_incremental_index_skips_duplicate_session_hash() { /* ... */ }

#[test]
fn test_memory_search_tool_tier1_returns_hits() { /* ... */ }

#[test]
fn test_memory_search_tool_tier2_expands_chunk() { /* ... */ }

#[test]
fn test_memory_search_tool_tier3_returns_full_transcript() { /* ... */ }

#[test]
fn test_transcript_index_survives_process_restart() { /* ... */ }

#[test]
fn test_session_end_indexes_all_messages() { /* ... */ }
```

### Critérios de Aceite Phase 4

- **AC-4.1**: `MemoryTantivyIndex` persiste em disco via `MmapDirectory`.
- **AC-4.2**: Schema suporta `source_type = "transcript"` com `session_id`, `turn_index`, `timestamp_unix`.
- **AC-4.3**: `on_session_end` indexa transcripts automaticamente.
- **AC-4.4**: Re-indexação da mesma sessão com mesmo hash é no-op (idempotente).
- **AC-4.5**: Tool `memory_search` implementa 3 tiers: search → get → transcript.
- **AC-4.6**: Index sobrevive restart do processo (persistência real).
- **AC-4.7**: BM25 scoring funciona cross-session (queries recuperam msgs de sessões antigas).

### DoD Phase 4

- [ ] `MemoryTantivyIndex::open_or_create(&Path)` em produção.
- [ ] Schema estendido com 3 campos para transcripts.
- [ ] Indexer incremental em `on_session_end` com content hash.
- [ ] Tool `memory_search` registrado e testado.
- [ ] 8 testes passando em `tantivy_transcript_integration.rs`.
- [ ] Feature `tantivy-backend` permanece default em `theo-cli` (já é).
- [ ] Benchmark: indexação de 1000 msgs < 500ms, search < 50ms.
- [ ] CHANGELOG.md atualizado.

---

## Phase 5: Onboarding Proativo + Auto-improvement Prompts

> **Objetivo**: primeira sessão gera `BOOTSTRAP.md` com Q&A interativo, persiste em `USER.md`, depois deleta o bootstrap. Adiciona prompts SOTA de auto-improvement.
>
> **Dependências**: `MemoryProvider` (existe), `skill_catalog` auto-improvement prompt (Phase 3 Task 3.7).

### Task 5.1: Detector de primeira sessão

**Arquivo novo**: `crates/theo-agent-runtime/src/onboarding.rs`

**Mudança**: função pura `needs_bootstrap(memory_dir: &Path) -> bool`. Retorna true se `USER.md` não existe OU está vazio (< 50 chars sem frontmatter).

```rust
pub fn needs_bootstrap(memory_dir: &Path) -> bool {
    let user_path = memory_dir.join("USER.md");
    if !user_path.exists() { return true; }
    match std::fs::read_to_string(&user_path) {
        Ok(s) => s.trim().len() < 50,
        Err(_) => true,
    }
}
```

### Task 5.2: Template `BOOTSTRAP.md` + Q&A flow

**Arquivo novo**: `crates/theo-agent-runtime/src/onboarding.rs`

**Mudança**: injetar system message de bootstrap na primeira sessão:

```rust
const BOOTSTRAP_PROMPT: &str = r#"
This is your first session with this user. Before helping, gather
enough context to personalize your behavior. Ask ONE question at a time,
wait for the answer, then ask the next. Topics:

1. User's role and work context (what they build, what tech stack).
2. Preferences for your behavior (terse vs. verbose, autonomous vs. ask-first).
3. Important boundaries (what NOT to do, destructive ops, etc.).
4. Communication preferences (language, formality, emoji).

After gathering answers, write them to memory/USER.md using memory_tool.
Then confirm you're ready and proceed with the actual task.
"#;
```

### Task 5.3: Hook no início da primeira sessão

**Arquivo**: `crates/theo-agent-runtime/src/run_engine.rs`

**Mudança**: antes da primeira iteração, verificar `needs_bootstrap`. Se sim, prepend `BOOTSTRAP_PROMPT` ao system message.

```rust
pub async fn run(&self) -> AgentResult {
    if onboarding::needs_bootstrap(&self.cfg.memory_dir) {
        self.inject_bootstrap_prompt();
    }
    // ...existing loop...
}
```

### Task 5.4: USER.md schema

**Arquivo novo**: `crates/theo-infra-memory/src/user_profile.rs`

**Mudança**: struct `UserProfile` serializável como markdown com frontmatter YAML.

```rust
pub struct UserProfile {
    pub role: Option<String>,
    pub tech_stack: Vec<String>,
    pub preferences: PreferenceSet,
    pub boundaries: Vec<String>,
    pub language: Option<String>,
    pub updated_at_unix: u64,
}

pub struct PreferenceSet {
    pub verbosity: Verbosity,       // Terse | Normal | Verbose
    pub autonomy: Autonomy,         // AskFirst | Autonomous
    pub formality: Formality,       // Casual | Formal
}

impl UserProfile {
    pub fn to_markdown(&self) -> String { /* frontmatter + body */ }
    pub fn from_markdown(s: &str) -> Result<Self, ProfileError> { /* ... */ }
}
```

### Task 5.5: Auto-improvement reminder hook

**Arquivo**: `crates/theo-agent-runtime/src/hooks.rs`

**Mudança**: novo hook `UserPromptSubmit` que injeta reminder curto (~50 tokens) periodicamente:

```
Periodic reminder: if this conversation reveals something
about the user's preferences, workflow, or environment,
save it to memory before ending the turn.
```

### Task 5.6: Testes de integração

**Arquivo novo**: `crates/theo-agent-runtime/tests/onboarding_integration.rs`

```rust
#[tokio::test]
async fn test_bootstrap_triggers_when_user_md_missing() { /* ... */ }

#[tokio::test]
async fn test_bootstrap_skipped_when_user_md_populated() { /* ... */ }

#[tokio::test]
async fn test_bootstrap_prompt_injected_before_first_turn() { /* ... */ }

#[tokio::test]
async fn test_user_profile_roundtrip_markdown() { /* ... */ }

#[tokio::test]
async fn test_auto_improvement_reminder_injected_periodically() { /* ... */ }
```

### Critérios de Aceite Phase 5

- **AC-5.1**: `needs_bootstrap` retorna true quando `USER.md` ausente ou < 50 chars.
- **AC-5.2**: Na primeira sessão, `BOOTSTRAP_PROMPT` é prepended ao system message.
- **AC-5.3**: Q&A coleta 4 tópicos (role, preferences, boundaries, language).
- **AC-5.4**: `UserProfile` serializa/deserializa markdown com frontmatter YAML.
- **AC-5.5**: Após preenchimento de `USER.md`, `needs_bootstrap` retorna false.
- **AC-5.6**: Auto-improvement reminder injetado a cada N prompts do usuário.

### DoD Phase 5

- [ ] `onboarding::needs_bootstrap` implementado e testado.
- [ ] `BOOTSTRAP_PROMPT` injetado condicionalmente em `run_engine`.
- [ ] `UserProfile` struct + serialização markdown.
- [ ] 5 testes passando em `onboarding_integration.rs`.
- [ ] `UserPromptSubmit` hook configurado com reminder.
- [ ] CHANGELOG.md atualizado.

---

## DoD Global

Para declarar o plano COMPLETO:

- [ ] Todas as 5 fases com DoD individual checked.
- [ ] `cargo build --workspace --exclude theo-code-desktop` sem warnings.
- [ ] `cargo clippy --workspace --exclude theo-code-desktop --all-targets` sem warnings.
- [ ] `cargo test --workspace --exclude theo-code-desktop` 3046 + ~28 testes novos = **~3074 tests passing**, 0 failed.
- [ ] E2E manual: sessão fresh com OAuth demonstra:
  1. BOOTSTRAP.md disparado na primeira run
  2. Memory reviewer dispara após 10 turns (log visível)
  3. Skill reviewer dispara após 5 tool calls sem skill
  4. `memory_search` tool retorna hits de sessões anteriores
  5. Autodream dispara no `/exit` e log mostra `ConsolidationReport`
- [ ] Documentação atualizada em:
  - `docs/current/memory-architecture.md` (nova seção Auto-Evolution)
  - `CHANGELOG.md` 5 entries (uma por fase)
  - `docs/adr/009-auto-evolution-sota.md` (novo ADR explicando decisões)
- [ ] Benchmarks:
  - Memory reviewer spawn latency < 10ms
  - Autodream total duration < 30s (típico)
  - Tantivy search cross-session < 50ms
  - Skill scan overhead < 5ms por skill
- [ ] Zero regressão em benchmarks existentes (MRR, compression ratio).

---

## Riscos e Mitigações

| Risco | Probabilidade | Impacto | Mitigação |
|-------|---------------|---------|-----------|
| Background tasks vazam memória | Média | Alto | Timeout hard + `tokio::spawn` com handle drop; integration test verifica |
| Tantivy persistence corrompe em crash | Baixa | Médio | Commit atomic + `.corrupt` fallback (mesmo padrão de episodes) |
| LLM autodream fica em loop infinito | Baixa | Médio | Timeout 60s default + iteration cap no executor |
| Skill auto-generation produz lixo | Média | Baixo | 7-gate pattern reutilizado (quarantine antes de active) |
| Bootstrap prompt confunde usuário experiente | Baixa | Baixo | Config `skip_onboarding: true` + detecção via "I already know X" heuristic |
| Nudge counter race em multi-thread | Média | Baixo | `AtomicUsize` com `fetch_add` — já previsto |
| Token cost de reviewers explode | Média | Médio | Skills usam modelo pequeno (sonnet) + soft budget cap per-session |

---

## Referências Cruzadas

- **Hermes Agent** — `docs/pesquisas/` + [hermes-agent.nousresearch.com](https://hermes-agent.nousresearch.com/docs/)
  - Issue #8506 (memory nudge reset bug) — mitigado por `AtomicUsize` shared state
  - Issue #496 (Promptware attack) — mitigado por `scan_skill_body` multi-layer
- **Claude Code autodream** — feature disclosed em apresentação Anthropic
- **OpenClaw BOOTSTRAP.md** — [docs.openclaw.ai/start/bootstrapping](https://docs.openclaw.ai/start/bootstrapping)
- **memsearch 3-tier** — [github.com/zilliztech/memsearch](https://github.com/zilliztech/memsearch)
- **Plano base**: `docs/plans/PLAN_MEMORY_SUPERIORITY.md` (fornece MemoryProvider, 7-gate, Hypothesis, Decay)
- **Plano base**: `docs/plans/PLAN_CONTEXT_WIRING.md` (fornece EventBus wiring, RetrievalExecuted)
- **ADR-008** — `docs/adr/008-theo-infra-memory.md` (justifica crate boundary)

---

## Ordem de Execução Recomendada

```
Sprint 1 (1-2 dias):
  Phase 1 — Nudge Counter + Memory Reviewer        [150 LOC]
  Phase 2 — Autodream Daemon                       [200 LOC]

Sprint 2 (2-3 dias):
  Phase 3 — Skill Generator Autônomo               [300 LOC]

Sprint 3 (2-3 dias):
  Phase 4 — Tantivy Persistente                    [400 LOC]

Sprint 4 (1 dia):
  Phase 5 — Onboarding + Auto-improvement          [170 LOC]
  Documentação + ADR-009 + CHANGELOG consolidado
```

Cada fase mergeável independentemente. Phase 4 é a mais arriscada (persistência em disco) — recomendo feature-flag inicial e gradual rollout.
