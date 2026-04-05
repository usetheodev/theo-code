# Theo — System Flows, Features & Gap Analysis

> Documento tecnico mapeando cada feature com fluxo de execucao, status, gaps e acertos.

---

## 1. Agent REPL (`theo`)

**Trigger**: `theo` sem argumentos

**Flow**:
```
main.rs:cmd_agent() → Repl::new() (restore session)
  → Repl::run() → readline loop
    → ensure_graph_context() (fire-and-forget background)
    → execute_task(input)
      → AgentLoop::new() + with_graph_context()
      → agent.run_with_history(task, project_dir, session_messages, event_bus)
        → AgentRunEngine::new()
        → engine.execute_with_history()
          → auto_init_project_context()
          → load system prompt (.theo/system-prompt.md)
          → load project context (.theo/theo.md)
          → load memories
          → inject skills
          → MAIN LOOP:
            → budget check
            → context loop injection (every N iter)
            → compact_if_needed()
            → LLM call (streaming + retry)
            → for each tool_call:
              → meta-tool routing (done/subagent/skill/batch)
              → pre-hook check
              → plan mode guard
              → tool.execute() via registry
              → post-hook (async)
              → record metrics + edits
      → save session to disk
      → display result
```

**Status**: COMPLETO

**Acertos**:
- Session persistence entre reinícios
- GRAPHCTX em background (zero blocking)
- Auto-init transparente
- Streaming com feedback real-time (CliRenderer)
- Context compaction previne degradação

**Gaps**:
- [ ] Ctrl+C abort (abort_tx é dead code no RunEngine)
- [ ] Sem shell completions (Clap suporta, não gerado ainda)
- [ ] `/clear` command não limpa session persistida

---

## 2. Single-shot (`theo "task"`)

**Trigger**: `theo "fix the bug"`

**Flow**: Mesmo que REPL, mas `Repl::execute_single()` — executa uma vez e sai.

**Status**: COMPLETO

**Acertos**:
- Sem `agent` subcommand — UX limpa como Claude Code
- Session salva mesmo em single-shot (próximo run tem contexto)

**Gaps**:
- [ ] Exit code não reflete success/failure do AgentResult (sempre 0)

---

## 3. Init (`theo init`)

**Trigger**: `theo init [--repo path]`

**Flow**:
```
main.rs:cmd_init() → resolve_agent_config()
  → init::run_init_with_agent(project_dir, config)
    → check idempotência (.theo/theo.md exists?)
    → create .theo/ + .gitignore
    → IF api_key available:
        → AgentLoop com ENRICH_PROMPT
        → agent lê projeto (read, grep, bash, codebase_context)
        → agent escreve .theo/theo.md + .theo/changelog.md
    → ELSE:
        → template estático (detect_project_type + render_theo_md)
```

**Status**: COMPLETO

**Acertos**:
- AI-powered: gera conteúdo REAL (arquitetura, funções, convenções)
- Fallback para template se sem API key
- Idempotente (não sobrescreve)
- .gitignore criado automaticamente

**Gaps**:
- [ ] Sem `--force` para re-gerar (precisa deletar manualmente)
- [ ] Changelog.md depende de git — sem git, conteúdo pobre

---

## 4. Pilot (`theo pilot "promise"`)

**Trigger**: `theo pilot "implement X" --complete "tests pass" --calls 10`

**Flow**:
```
main.rs:cmd_pilot() → resolve_agent_config()
  → PilotConfig::load(.theo/config.toml)
  → resolve_promise(args | .theo/PROMPT.md)
  → pilot::run_pilot()
    → init GRAPHCTX (background)
    → PilotLoop::new()
    → check roadmap (.theo/plans/*.md)
    → IF roadmap: run_from_roadmap()
    → ELSE: run()
      → LOOP:
        → check interrupt (Ctrl+C)
        → check rate limit (calls/hour)
        → check circuit breaker (Closed/Open/HalfOpen)
        → build_loop_prompt() + corrective_guidance()
          → HeuristicReflector::corrective_guidance()
        → spawn AgentLoop (fresh per iteration)
        → agent.run_with_history()
        → update_counters() (progress, errors, git SHA)
        → evaluate_exit() → ExitReason
```

**Status**: COMPLETO

**Acertos**:
- Circuit breaker previne loops infinitos
- Dual-exit gate (done signal + git progress)
- Corrective guidance via HeuristicReflector
- Roadmap execution (parse → execute tasks sequencialmente)
- Rate limiting (calls/hour)
- Ctrl+C graceful shutdown

**Gaps**:
- [ ] Reflector Fase 2 (LearningStore) não implementado — learnings não persistem
- [ ] Reflector Fase 3 (métricas + decay) não implementado
- [ ] Reflector Fase 4 (LLM-based) não implementado
- [ ] `done()` pelo agent nem sempre é preciso — false positives
- [ ] Sem métricas comparativas entre runs (loops_to_success trend)

---

## 5. GRAPHCTX (Code Intelligence)

**Trigger**: LLM chama `codebase_context` tool

**Flow**:
```
Tool call "codebase_context" → CodebaseContextTool::execute()
  → ctx.graph_context (Option<Arc<dyn GraphContextProvider>>)
  → IF not ready (Building): return "building, retry later"
  → IF ready: provider.query_context(query, budget)
    → MultiSignalScorer::score(query, communities, graph)
      → BM25 (25%) + Semantic (20%) + File boost (20%)
      → Graph attention (15%) + Centrality (10%) + Recency (10%)
    → assemble_greedy(scored, graph, budget_tokens)
      → community_content() com contains_children() (signatures!)
    → ContextPayload → GraphContextResult
  → return ToolOutput com signatures
```

**Background build** (fire-and-forget):
```
GraphContextService::initialize()
  → try_load_cache(.theo/graph.bin)
  → IF cache miss:
    → tokio::spawn(async {
        → parse_project_files() (tree-sitter, 16 linguagens)
          → detect_project_language() → priorizar linguagem principal
          → filter_entry() (excluir target/, node_modules/)
          → MAX_FILES_TO_PARSE = 500
        → bridge::build_graph() (MCPH graph)
        → populate_cochanges_from_git()
        → hierarchical_cluster(FileLeiden)
        → MultiSignalScorer::build()
        → save_cache_atomic(.theo/graph.bin)
      })
  → State: Uninitialized → Building → Ready | Failed
```

**Status**: COMPLETO

**Acertos**:
- On-demand (LLM decide quando precisa — zero custo para tasks simples)
- Background build (agent nunca espera)
- State machine explícita (Building/Ready/Failed)
- Signatures de funções (não file paths)
- Priorização por linguagem do projeto
- Cache atômico (.bin.tmp → rename)
- Fallback: agent opera sem contexto se graph falhar

**Gaps**:
- [ ] Sem estado Dirty (graph não atualiza após edits na sessão)
- [ ] Cache sem version header (schema change corrompe silenciosamente)
- [ ] Neural embeddings (~90MB download) — sem progress feedback
- [ ] Clustering O(n³) em repos grandes — timeout possível em debug build

---

## 6. Context Compaction

**Trigger**: Automático a cada iteração do RunEngine (antes do LLM call)

**Flow**:
```
run_engine.rs:347 → compact_if_needed(&mut messages, context_window_tokens)
  → estimate_total_tokens() (content + tool_call_args) / 4 + overhead
  → IF total <= 80% * context_window_tokens: return (no-op)
  → identify boundary: preserve last 6 non-system messages
  → FOR each message before boundary:
    → System: never touch
    → Tool result: truncate content to 200 chars (UTF-8 safe)
    → Assistant with tool_calls: truncate arguments
    → Extract file mentions + tool names for summary
  → Remove previous [COMPACTED] summary (idempotent)
  → Insert new summary as user message
```

**Status**: COMPLETO

**Acertos**:
- UTF-8 safe truncation (chars, não bytes)
- Pares tool_call/tool_result preservados como unidade
- Idempotente (sem duplicação de summaries)
- Budget configurável via `context_window_tokens` (default 128K)
- 10 testes unitários

**Gaps**:
- [ ] Compaction é heurística-only — LLM-based compaction (qualidade superior) não implementado
- [ ] Sem re-query do GRAPHCTX após compaction (contexto de código pode ser perdido)
- [ ] Threshold fixo (80%) — não adaptativo por modelo

---

## 7. Session Persistence

**Trigger**: Automático no REPL (load no boot, save após cada task e no exit)

**Flow**:
```
LOAD: Repl::new()
  → load_session(project_dir)
  → read ~/.config/theo/sessions/{project_hash}.json
  → deserialize Vec<Message>
  → graceful: corrupt/missing → empty vec

SAVE: execute_task() + exit points
  → save_session(project_dir, &session_messages)
  → cap MAX_SESSION_MESSAGES (100)
  → serialize_pretty → write to disk
```

**Status**: COMPLETO

**Acertos**:
- Graceful degradation (JSON corrompido → session vazia)
- Cap de 100 mensagens
- Hash determinístico por project dir
- Save após cada task (crash safety)
- 5 testes unitários

**Gaps**:
- [ ] Sem `/clear` command para limpar session
- [ ] Sem rotação de sessions antigas (acumula em disco)
- [ ] Session não distingue entre runs de agent vs pilot

---

## 8. RunSnapshot

**Trigger**: Após cada iteração do RunEngine (se SnapshotStore configurado)

**Flow**:
```
run_engine.rs (evaluating phase):
  → tool_call_manager.calls_for_task(&task_id)
  → get_result() para cada call → Vec<ToolResultRecord>
  → messages.iter().map(serde_json::to_value) → Vec<Value>
  → RunSnapshot::new(run, task, tool_calls, tool_results, events, budget, messages, dlq)
  → compute_checksum() (SHA hash)
  → store.save(&run_id, &snapshot)
```

**Status**: COMPLETO

**Acertos**:
- Trajetórias completas (messages + tool_calls + tool_results)
- Checksum para integridade
- Dados prontos para fine-tuning (theo-Q3)

**Gaps**:
- [ ] SnapshotStore não configurado na CLI (FileSnapshotStore existe mas não é wired)
- [ ] Sem cleanup de snapshots antigos
- [ ] Sem export para formato de treino (JSONL/ChatML)

---

## 9. Sandbox

**Trigger**: Qualquer chamada ao `bash` tool

**Flow**:
```
create_default_registry():
  → SandboxConfig::default()
  → mount ~/.cargo, ~/.rustup como read-only
  → allow CARGO_HOME, RUSTUP_HOME env vars
  → create_executor(&sandbox_config)
    → try bwrap (bubblewrap) → landlock → noop cascade
  → BashTool::with_sandbox(executor, config)

BashTool::execute():
  → command_validator (reject dangerous patterns)
  → env_sanitizer (strip tokens: AWS, GitHub, OpenAI)
  → executor.execute(command, project_dir, timeout)
    → bwrap: PID ns, net ns, mount isolation, cap drop
    → OR landlock: filesystem access control
    → OR noop: direct execution (fallback)
  → capture stdout/stderr
  → enforce rlimits (CPU, memory, file size)
```

**Status**: COMPLETO

**Acertos**:
- 3-tier cascade (bwrap > landlock > noop)
- SSRF blocking (private IPs, metadata endpoints)
- Exfil blocking (pipe-to-shell patterns: Blocked, não Warning)
- Env sanitization (LD_PRELOAD removido)
- Build tools permitidos (cargo, rustc via read-only mounts)

**Gaps**:
- [ ] Path traversal em Write/Edit tools (../../etc/passwd) — NÃO PROTEGIDO
- [ ] Noop fallback executa sem isolamento (quando bwrap e landlock indisponíveis)
- [ ] Network não bloqueada em landlock (só em bwrap)

---

## 10. Sub-agents

**Trigger**: LLM chama `subagent` ou `subagent_parallel`

**Flow**:
```
run_engine.rs (meta-tool intercept):
  → parse role + objective
  → SubAgentManager::spawn(role, objective)
    → build sub-config:
      → system_prompt from role.system_prompt()
      → max_iterations from role.max_iterations()
      → capability_set from role.capability_set()
      → is_subagent = true
    → 3-layer defense:
      1. Schema stripping: remove subagent/skill/batch from tool defs
      2. Prompt isolation: "You are a sub-agent. Do NOT delegate."
      3. CapabilityGate: role-specific tool restrictions
    → AgentLoop::new(sub_config) + with_graph_context()
    → agent.run(objective, project_dir)
  → aggregate tokens into parent budget
  → return AgentResult

subagent_parallel:
  → futures::future::join_all(spawned tasks)
  → combine results
```

**Status**: COMPLETO

**Acertos**:
- 3-layer recursive spawning prevention
- Role-based capabilities (explorer=read-only, implementer=full)
- GRAPHCTX herdado (Arc clone, zero rebuild)
- Token aggregation para budget do pai
- Parallel execution real (join_all)

**Gaps**:
- [ ] Sub-agents não herdam session history do pai
- [ ] Sem limit de sub-agents simultâneos (could exhaust LLM rate limits)
- [ ] Sem comunicação entre sub-agents (cada um isolado)

---

## 11. Skills

**Trigger**: LLM chama `skill` tool

**Flow**:
```
run_engine.rs (meta-tool intercept):
  → SkillRegistry::new()
  → load_bundled() → 10 skills: commit, test, review, build, explain, fix, refactor, pr, doc, deps
  → load_from_dir(.theo/skills/)
  → load_from_dir(~/.config/theo/skills/)
  → lookup skill by name
  → IF mode=InContext: inject instructions as system message
  → IF mode=SubAgent: spawn sub-agent with skill instructions
```

**Status**: COMPLETO

**Acertos**:
- 10 bundled skills cobrindo workflows comuns
- Project-specific skills (.theo/skills/)
- User-global skills (~/.config/theo/skills/)
- InContext vs SubAgent modes
- Triggers summary injetado no system prompt

**Gaps**:
- [ ] Skills não são auto-invocadas (LLM precisa chamar explicitamente)
- [ ] Sem skill versioning
- [ ] Sem skill sharing entre projetos (copy manual)

---

## 12. Hooks

**Trigger**: Antes/depois de cada tool call no RunEngine

**Flow**:
```
run_engine.rs:
  → HookRunner::new(project_dir, HookConfig::default())
  → discover .theo/hooks/*.sh + ~/.config/theo/hooks/*.sh

  PRE-HOOK (blocking):
  → runner.run_pre_hook("tool.before", &HookEvent{tool_name, args})
  → execute script com JSON via stdin
  → IF exit != 0: BLOCK tool call, return error to LLM

  POST-HOOK (fire-and-forget):
  → runner.run_post_hook("tool.after", &HookEvent{...})
  → execute async, errors logged
```

**Status**: COMPLETO

**Acertos**:
- Pre-hooks podem bloquear (exit code != 0)
- Post-hooks fire-and-forget (não atrasam)
- JSON event via stdin (machine-readable)
- Timeout configurável (default 5s)

**Gaps**:
- [ ] Sem hook templates bundled (usuário precisa criar do zero)
- [ ] Sem hooks para eventos de sessão (start, end, error)
- [ ] Hook discovery não é lazy (re-scanned a cada task)

---

## 13. Plugins

**Trigger**: Descobertos no boot do AgentLoop

**Flow**:
```
AgentLoop::new() → load_plugin_tools(registry, project_dir)
  → plugin::load_plugins(project_dir)
  → scan .theo/plugins/ e ~/.config/theo/plugins/
  → for each dir with plugin.toml:
    → parse PluginManifest (name, version, tools, hooks)
    → resolve tool scripts (verify exists)
    → resolve hook scripts
  → register_plugin_tools(registry, plugin_tools)
    → ShellTool::new(name, description, script_path, params)
    → registry.register(tool)
```

**Status**: COMPLETO

**Acertos**:
- TOML manifest (declarativo)
- Shell scripts como tools (qualquer linguagem)
- JSON stdin/stdout protocol
- Hook integration automática
- Project + global plugin paths

**Gaps**:
- [ ] Sem plugin install command (`theo plugin install <url>`)
- [ ] Sem plugin marketplace/registry
- [ ] Sem versioning/update de plugins

---

## 14. Memory

**Trigger**: LLM chama `memory` tool ou auto-inject no boot

**Flow**:
```
BOOT (run_engine.rs:209-236):
  → FileMemoryStore::for_project(~/.config/theo/memory/, project_dir)
  → store.list() → Vec<AgentMemoryEntry>
  → inject as system message: "## Memory from previous runs"

TOOL (memory tool execute()):
  → action: save | recall | list | search | delete
  → save: atomic write to ~/.config/theo/memory/{project_hash}/{key}.json
  → recall: read single entry
  → list: read all entries
  → search: query by key/value content
  → delete: remove file
```

**Status**: COMPLETO

**Acertos**:
- Atomic write (tmp + rename)
- Project-scoped (hash do path)
- Auto-inject no boot (memories disponíveis sem ação do LLM)
- Key sanitization (max 128 chars, replace special chars)

**Gaps**:
- [ ] Sem limit de memories por projeto (pode crescer indefinidamente)
- [ ] Sem expiry/TTL
- [ ] Sem global memories (só project-scoped)

---

## 15. Batch (Parallel Tool Execution)

**Trigger**: LLM chama `batch` tool

**Flow**:
```
run_engine.rs (meta-tool intercept):
  → parse calls: Vec<{tool, args}>
  → validate: max 25, block recursion (batch/done/subagent/skill)
  → plan mode guard (block edit/write if plan mode)
  → build futures for each valid call
  → futures::future::join_all(futures)
  → sort results by index
  → format combined output: "[1/3] read(...): OK — preview"
  → record in budget + metrics
```

**Status**: COMPLETO

**Acertos**:
- Parallel real (join_all, não sequencial)
- Blocked meta-tools (sem recursão)
- Partial failure (1 de 3 falha → outros continuam)
- Token savings (~30-40% para tasks exploratórias)

**Gaps**:
- [ ] Sem timeout per-call dentro do batch
- [ ] Results são string concatenada (não structured)

---

## 16. Reflector (Self-Improving Pilot)

**Trigger**: Após cada loop do PilotLoop

**Flow**:
```
PilotLoop::build_corrective_guidance():
  → self.reflector.corrective_guidance(
      consecutive_no_progress,
      consecutive_same_error,
      last_error,
      success=false
    )
  → classify_failure():
    → IF success: None
    → IF no_progress >= 2: NoProgressLoop
    → IF same_error >= 2 && last_error.is_some(): RepeatedSameError
  → guidance_for_pattern():
    → NoProgressLoop: "Focus on EDITING code, not just reading"
    → RepeatedSameError: "Stop retrying. Try something DIFFERENT."
```

**Status**: FASE 1 COMPLETA (heurístico)

**Acertos**:
- Pure function (zero IO, testável)
- Drop-in replacement do corrective_guidance inline anterior
- Priority: NoProgressLoop > RepeatedSameError
- Threshold como constante nomeada (GUIDANCE_THRESHOLD = 2)
- 12 testes unitários

**Gaps (fases planejadas)**:
- [ ] Fase 2: LearningStore (.theo/learnings.json) — persistir learnings
- [ ] Fase 3: Métricas + decay + global learnings
- [ ] Fase 4: LLM Reflector (opt-in, async background)
- [ ] Mais patterns: EditedWrongFile, MissingImports, TestRegression, ToolCallLoop

---

## 17. Auto-init

**Trigger**: Automático no boot do RunEngine (main agent only)

**Flow**:
```
run_engine.rs:185 → auto_init_project_context(project_dir)
  → IF .theo/theo.md exists: return (idempotent)
  → detect project type (Cargo.toml/package.json/etc.)
  → detect project name (simple manifest parsing)
  → write minimal template
  → create .theo/.gitignore
  → log: "[theo] Auto-initialized .theo/theo.md"
```

**Status**: COMPLETO

**Acertos**:
- Zero blocking (template estático, instantâneo)
- Idempotente
- Best-effort (falha silenciosa se sem permissão)
- Sugere `theo init` para enriquecimento

**Gaps**:
- [ ] Template mínimo (nome + linguagem apenas) — sem arquitetura

---

## 18. Doom Loop Detection

**Trigger**: Automático a cada tool call no RunEngine

**Flow**:
```
run_engine.rs (tool loop):
  → doom_tracker.record(tool_name, &args)
  → ring buffer de (name, hash(args))
  → IF all entries identical:
    → hit_count += 1
    → hit_count == 1: inject warning message
    → hit_count >= 2 (should_abort): HARD ABORT
      → RunState::Aborted
      → return AgentResult with failure
```

**Status**: COMPLETO

**Acertos**:
- Escalation: warning → abort (não abort imediato)
- Configurável via doom_loop_threshold (default 3)
- Hash-based comparison (eficiente)

**Gaps**:
- [ ] Sem log do padrão detectado (qual tool/args causou)
- [ ] Threshold fixo (não adaptativo por complexidade da task)

---

## 19. Context Loops

**Trigger**: A cada N iterações (context_loop_interval, default 5)

**Flow**:
```
run_engine.rs:330-340:
  → IF iteration > 1 && iteration % interval == 0:
    → agent_state.build_context_loop(iteration, max_iterations, objective)
    → gera user message: "[Context Loop: iter X/Y] Task: {objective}"
    → push to messages
```

**Status**: COMPLETO

**Acertos**:
- Re-orienta o agent periodicamente
- Inclui task objective (lembrete do que fazer)
- Previne drift em sessões longas

**Gaps**:
- [ ] Mensagem genérica (não adapta ao estado real do progresso)
- [ ] Não inclui resumo do que já foi feito

---

## 20. Tool Execution Pipeline

**Trigger**: LLM response com tool_calls

**Flow**:
```
run_engine.rs (main loop, após LLM response):
  → FOR each tool_call in response:
    → 1. META-TOOL CHECK (priority order):
      → "done" → exit loop, return result
      → "subagent" → spawn sub-agent
      → "subagent_parallel" → spawn parallel
      → "skill" → invoke skill
      → "batch" → parallel tool execution
    → 2. PRE-HOOK (if hook runner configured):
      → run_pre_hook("tool.before", event)
      → IF blocked: skip tool, return error
    → 3. PLAN MODE GUARD:
      → IF mode=Plan && tool in [edit,write,apply_patch]:
        → BLOCK (except .theo/plans/ writes)
    → 4. EXECUTE:
      → ToolContext { session_id, call_id, project_dir, graph_context }
      → tool_bridge::execute_tool_call(registry, tool_call, ctx)
        → registry.get(name) → tool.execute(args, ctx, perms)
      → truncate output if > 8000 chars
    → 5. POST-HOOK (async fire-and-forget):
      → run_post_hook("tool.after", event)
    → 6. RECORD:
      → budget_enforcer.record_tool_call()
      → metrics.record_tool_call()
      → track file edits (for progress detection)
      → doom loop tracker
    → 7. MESSAGE:
      → Message::tool_result(call_id, name, output)
      → push to messages
```

**Status**: COMPLETO

**Acertos**:
- Pipeline claro com separação de concerns
- Meta-tools interceptados antes de tools regulares
- Hooks integrados (pre = blocking, post = async)
- Plan mode guard (previne edits acidentais)
- Truncation de outputs longos

**Gaps**:
- [ ] Sem timeout per-tool (depende do tool implementar internamente)
- [ ] Sem retry automático por tool (se tool falha, LLM decide)
- [ ] Permission system não ativo (PermissionCollector existe mas não envia ao user)

---

## Resumo de Status

| # | Feature | Status | Testes | Gaps |
|---|---|---|---|---|
| 1 | REPL | COMPLETO | Session tests | Ctrl+C, shell completions |
| 2 | Single-shot | COMPLETO | Via REPL tests | Exit code |
| 3 | Init | COMPLETO | 15 | --force |
| 4 | Pilot | COMPLETO | 24 | Reflector Fase 2-4 |
| 5 | GRAPHCTX | COMPLETO | 10 (application) + 4 (domain) | Dirty state, cache versioning |
| 6 | Compaction | COMPLETO | 10 | LLM-based compaction |
| 7 | Session | COMPLETO | 5 | /clear, rotation |
| 8 | Snapshot | COMPLETO | Existing | CLI wiring, export |
| 9 | Sandbox | COMPLETO | Existing | PATH TRAVERSAL |
| 10 | Sub-agents | COMPLETO | Existing | Session sharing, limits |
| 11 | Skills | COMPLETO | Existing | Auto-invoke, versioning |
| 12 | Hooks | COMPLETO | E2E tested | Templates, session events |
| 13 | Plugins | COMPLETO | E2E tested | Install command, marketplace |
| 14 | Memory | COMPLETO | Existing | Limits, TTL, global |
| 15 | Batch | COMPLETO | Existing | Per-call timeout |
| 16 | Reflector | FASE 1 | 12 | Fases 2-4 |
| 17 | Auto-init | COMPLETO | Via E2E | Template mínimo |
| 18 | Doom loop | COMPLETO | 4 | Logging, adaptive threshold |
| 19 | Context loops | COMPLETO | Via runtime | Adaptive content |
| 20 | Tool pipeline | COMPLETO | 290+ runtime | Per-tool timeout, permissions |

## Gaps Prioritizados

### P0 — Segurança (bloqueia release)

| Gap | Feature | Esforço |
|---|---|---|
| **Path traversal em Write/Edit** | #9 Sandbox | 10 linhas |

### P1 — UX (impacta experiência)

| Gap | Feature | Esforço |
|---|---|---|
| Ctrl+C abort funcional | #1 REPL | Baixo |
| Exit code reflete success/failure | #2 Single-shot | Trivial |
| Shell completions (Clap) | #1 REPL | Baixo |
| `/clear` limpa session | #7 Session | Baixo |

### P2 — Evolução (diferencial competitivo)

| Gap | Feature | Esforço |
|---|---|---|
| Reflector Fase 2 (LearningStore) | #16 Reflector | 3-4 dias |
| Reflector Fase 3 (métricas + decay) | #16 Reflector | 2-3 dias |
| GRAPHCTX Dirty state | #5 GRAPHCTX | Médio |
| SnapshotStore wired na CLI | #8 Snapshot | Baixo |
| LLM-based compaction | #6 Compaction | Médio |

### P3 — Nice-to-have

| Gap | Feature | Esforço |
|---|---|---|
| Plugin install command | #13 Plugins | Médio |
| Skill auto-invoke | #11 Skills | Médio |
| Memory TTL + limits | #14 Memory | Baixo |
| Adaptive doom loop threshold | #18 Doom loop | Baixo |
| Context loop com resumo real | #19 Context loops | Baixo |

## Onde Estamos Acertando

1. **Harness-first architecture** — sandbox, tools, context, safety, memory: todos implementados e testados
2. **Zero blocking** — GRAPHCTX em background, auto-init instantâneo, session restore rápido
3. **Self-correction** — compaction, context loops, doom loop detection, reflector, circuit breaker
4. **DIP respeitado** — runtime não conhece engines, domain não depende de infra
5. **1630+ testes** com zero warnings — baseline sólido para evolução
6. **On-demand intelligence** — codebase_context como tool, não como system message forçada
7. **Model-agnostic** — 25 providers, zero lock-in
8. **UX Claude Code-like** — `theo "task"` e pronto
