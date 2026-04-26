# Plan: SOTA Tier 1 + Tier 2 — Closing the Gap to Claude Code / Cursor / Devin

> **Version 1.0** — Plano executável que fecha os 16 gaps T1+T2 identificados no diagnóstico SOTA. Sete trabalhos T1 (multimodal, browser, LSP, replanning, multi-agent, computer use, auto-test-gen) abrem mercado e diferenciação. Nove trabalhos T2 (reranker, skills marketplace, cost routing, prompt compression, eval CI, DAP, streaming UI, external docs RAG, RLHF feedback) consolidam qualidade. Resultado esperado: Theo Code mensurável em SWE-Bench-Verified e terminal-bench em paridade ou acima de Aider/Continue, com diferenciais únicos (schema-validated plans + multi-agent + browser + LSP integrados).

## Context

**Hoje (commit `37cb3b2` em `develop`):**
- 16 crates Rust, 47k+ LOC em `theo-agent-runtime`, dep contract enforced (0 violations).
- 21 default tools + 5 meta + 8 experimental + 2 internal; SOTA Planning System acabou de ser integrado (Plan/Phase/PlanTask + `run_from_plan`).
- 26 LLM providers OA-compat, MCP discovery, sandbox (bwrap > landlock), Tantivy memory, OTLP exporter, A/B benchmark com McNemar.
- 2.827 testes passando, 0 falhas. Arch contract clean.

**Lacunas concretas identificadas (ref. diagnóstico de 2026-04-26):**

| # | Tier | Gap |
|---|------|-----|
| 1 | T1 | **Multimodal/vision** — `FileAttachment` existe (`theo-domain/src/tool.rs:97`) mas `theo-infra-llm::types::Message.content` é `Option<String>` plana — sem suporte a `image_url` blocks (Anthropic/OpenAI vision). |
| 2 | T1 | **Browser automation / live preview** — Zero. `webfetch` é apenas HTTP GET com filtro de HTML. Sem playwright/chromedp wrapper, sem screenshot. |
| 3 | T1 | **LSP real** — `crates/theo-tooling/src/lsp/mod.rs` é stub (`status: Stub` no manifest); descrição "experimental, requires Language Server Protocol integration". |
| 4 | T1 | **Adaptive replanning T2** — `run_from_plan` (acabado de mergiar) executa DAG estática. Sem hook que mute o plano quando uma task falha repetidamente. |
| 5 | T1 | **Multi-agent paralelo + plan claim** — `SubAgentManager` existe; `subagent_runs` track runs; mas não há `Plan::claim_task(agent)` ou worker pool sobre `next_actionable_task`. |
| 6 | T1 | **Computer Use** — Anthropic adapter (`theo-infra-llm/src/providers/anthropic.rs`) não expõe `computer_20250124`. Sem tool `computer_*`. |
| 7 | T1 | **Auto-test-generation** — TDD é exigência humana. Agente não chama `proptest`/`quickcheck`/`cargo-mutants` proativamente. |
| 8 | T2 | **Cross-encoder reranker** — Já existe (`reranker.rs` com Jina v2) mas `feature = "reranker"` está OFF por default e não é cabo na pipeline RRF padrão. |
| 9 | T2 | **Skill marketplace** — `skill_catalog.rs` está marcado `#[allow(dead_code)]` (commit `37cb3b2`). Zero loader, zero registry, zero `theo skill install`. |
| 10 | T2 | **Cost-aware routing por complexidade** — `AutomaticModelRouter` existe (`theo-infra-llm/src/routing/auto.rs`) mas o `ComplexityClassifier` é heurístico simples e não está cabo no AgentLoop. |
| 11 | T2 | **Compactação semântica** — `compaction_stages.rs` (Mask/Prune) e `compaction_summary.rs` (LLM-summary) estão `#[allow(dead_code)]`. Pipeline rola só Mask por threshold. |
| 12 | T2 | **CI SOTA eval** — `apps/theo-benchmark/runner/ab_compare.py` faz pareado mas não roda em PR. Sem leaderboard SWE-Bench/HumanEval. |
| 13 | T2 | **DAP integration** — Zero. |
| 14 | T2 | **Live tool streaming UI** — `stdout_tx` em `ToolContext` (cabo só em `bash/mod.rs:253`); TUI não renderiza progress/markdown ao vivo. |
| 15 | T2 | **External docs RAG** — Wiki é interno; `webfetch` é one-shot. Sem index de docs externos persistido. |
| 16 | T2 | **RLHF feedback loop** — `.theo/trajectories/*.jsonl` são gravados; sem coleta 👍/👎 nem export para fine-tune dataset. |

**Evidência de tração:** SOTA Planning System (T1 do plan anterior) provou que o ciclo plano-validado→tools→pilot funciona. Replanning, multi-agent claim e auto-test são extensões naturais sobre o mesmo modelo `Plan`.

## Objective

**Done quando:** Theo Code roda SWE-Bench-Verified em CI e a curva move ≥10 pontos vs baseline (commit `37cb3b2`), com **todos os 16 gaps T1+T2 fechados** e validados via testes RED-GREEN-REFACTOR.

Metas mensuráveis:
1. Multimodal: tool `screenshot` + Anthropic vision messages funcionando E2E (1 RED test passa).
2. Browser: tool `browser_open/click/screenshot/eval` rodando em sandbox (chromium headless via Playwright).
3. LSP: `rename_symbol` + `find_references` reais via `tower-lsp` adapter contra `rust-analyzer`.
4. Replanning: `Plan::replan(failure)` chamado automático após N falhas; cobertura ≥85%.
5. Multi-agent: 3 sub-agents executam em paralelo `next_actionable_task` distintas em um worktree por agent.
6. Computer Use: tool `computer_*` no Anthropic provider passa smoke E2E.
7. Auto-test-gen: tool `gen_property_test` + `gen_mutation_test` integrado a `cargo-mutants`.
8. Reranker: `feature = "reranker"` ON por default; queries top-K reranqueadas.
9. Skill marketplace: `skill_catalog.rs` 100% wired; `theo skill install <name>` funcional.
10. Cost routing: AgentLoop respeita `complexity_hint`; A/B mostra ≥20% redução de custo sem queda de success.
11. Compactação: stages Prune+Compact ON; redução >40% de tokens em runs longos.
12. Eval CI: GitHub Actions roda terminal-bench reduced em cada PR.
13. DAP: tool `debug_*` (set_breakpoint, step, watch) sobre DAP server.
14. Streaming: bash + plan tools emitem `PartialToolResult`; TUI renderiza ao vivo.
15. External docs: tool `docs_search` indexa crates.io, MDN, npm em Tantivy local.
16. RLHF: trajectory carrega rating; export `tbench/build_rlhf_dataset.py` gera JSONL para fine-tune.

## ADRs

### D1 — Multimodal via blocos de conteúdo, não strings
- **Decision:** `Message.content` muda de `Option<String>` para `Option<Vec<ContentBlock>>` com `ContentBlock::{Text, ImageUrl, ImageBase64}`.
- **Rationale:** Anthropic/OpenAI já modelam assim; preservar string-only seria forçar adapters a stringificar imagens, perdendo fidelidade. `serde(untagged)` no boundary mantém OA-compat.
- **Consequences:** Ativa visão; quebra serialização de transcripts antigos — exige bump de `state_manager` schema (D9).

### D2 — Browser via Playwright sidecar, não embutido
- **Decision:** Browser automation roda via subprocesso `playwright` Node.js gerenciado pelo `theo-tooling::browser` que comunica por WebSocket local (CDP).
- **Rationale:** chromiumoxide/headless_chrome em Rust são imaturos para CDP completo. Playwright tem 1k+ contributors e é o padrão. Custo: dependência Node.
- **Consequences:** Requer `node` no PATH ou bundle. Ganho: paridade total de capacidades com Cursor/Lovable.

### D3 — LSP via `tower-lsp`/`lsp-types` cliente, servers externos
- **Decision:** Theo embute um *cliente* LSP (`lsp-client` crate) que conversa com servers externos (`rust-analyzer`, `pyright`, `typescript-language-server`) discovered no PATH.
- **Rationale:** Reusar servers já instalados pelo dev; rolling our own LSP server seria insano.
- **Consequences:** Funciona apenas onde server existe; ganho: cross-language sem novos parsers.

### D4 — Replanning é uma operação do `Plan`, não um agent novo
- **Decision:** `Plan::replan(failed_task, failure_reason) -> ReplanResult` — chama LLM com plano atual + contexto da falha, retorna patch sobre o plano.
- **Rationale:** Mantém o Plan como fonte de verdade. Evita criar "PlannerAgent" separado que duplica state.
- **Consequences:** Necessita LLM call no caminho crítico; mitigação: `replan_attempts_max` config, opt-out por task.

### D5 — Multi-agent claim via field `assignee` com `compare-and-swap` no plan_store
- **Decision:** `PlanTask.assignee: Option<String>` (run_id do agent que reivindicou) + `plan_store::claim_task(plan_path, task_id, agent_id, expected_version)`.
- **Rationale:** Lock-free coordination; o version check evita dois agents reivindicarem simultaneamente.
- **Consequences:** Persistência se torna ponto de coordenação; throughput limitado pelo I/O da rename atômica (~1 claim/ms — suficiente).

### D6 — Computer Use feature-gated por provider
- **Decision:** Tool `computer_*` registra-se condicionalmente quando provider ativo é `anthropic-vision` ou similar. Default registry permanece igual.
- **Rationale:** Apenas alguns providers suportam; YAGNI surfacing global.
- **Consequences:** Branching no `create_default_registry`; ganho: zero overhead para uso não-Anthropic.

### D7 — Auto-test-gen via tools especializadas, não modo do edit
- **Decision:** Novas tools `gen_property_test`, `gen_mutation_test`, `gen_unit_test` invocadas pelo agente (não automáticas).
- **Rationale:** Manter agência do LLM; auto-disparar geraria testes inúteis. Ganho: opt-in explícito.
- **Consequences:** LLM precisa aprender quando chamar; mitigação: instrução system-prompt + few-shot examples.

### D8 — Reranker é always-on, não feature flag
- **Decision:** Remover `#[cfg(feature = "reranker")]`. Sempre incluir no build; gate runtime via `RetrievalConfig.use_reranker: bool`.
- **Rationale:** Feature flag está em zero usuários; runtime gate dá controle sem fragmentar build.
- **Consequences:** ~50MB modelo ONNX baixado no primeiro run. Mitigação: lazy load, opt-out env var.

### D9 — Bump de schema para state e plans
- **Decision:** `state_manager::SCHEMA_VERSION = 2`, `PLAN_FORMAT_VERSION = 2`. v1→v2 migrado on-load (idempotente).
- **Rationale:** D1 quebra wire format; sem bump, transcripts antigos panicam.
- **Consequences:** Migration code, mais um teste de roundtrip por struct.

### D10 — Skill marketplace usa o mesmo formato do `agent_spec`
- **Decision:** Skills são markdown com frontmatter YAML idêntico a `.theo/agents/`, persistidos em `~/.theo/skills/<name>/SKILL.md`.
- **Rationale:** Reusa parser existente; uniformidade.
- **Consequences:** Capabilities/permissions herdam mesmas regras.

### D11 — Cost routing usa `complexity_hint` no AgentConfig, não env
- **Decision:** `AgentConfig.routing.cost_aware: bool` + `complexity_classifier: Box<dyn ComplexityClassifier>`.
- **Rationale:** Config-driven, testável; env-only é difícil debug.
- **Consequences:** Mais um campo no nested config (T3.2 já cobre profundidade).

### D12 — Compactação Compact stage usa LLM auxiliary (cheaper model)
- **Decision:** `compaction_summary` chama um modelo barato (Haiku/gpt-4o-mini) para gerar summary, não o modelo principal.
- **Rationale:** Custo controlado; perda de qualidade é aceitável para summary background.
- **Consequences:** Requer auxiliary client config; já parcialmente em `theo-infra-llm::routing`.

### D13 — Eval CI roda em GitHub Actions com modelo *gratuito* primeiro
- **Decision:** CI default usa `Llama-3.3-70B` via Groq free tier para smoke; modelos pagos rodam só em `[bench]` label do PR.
- **Rationale:** Sem queimar orçamento por PR; smoke já pega regressões grosseiras.
- **Consequences:** False negatives em casos sutis; mitigação: nightly full bench em modelos pagos.

### D14 — DAP via `debug_adapter_protocol` cliente Rust
- **Decision:** Reusar crate `dap` (do projeto `dap-rs`) para conversar com `lldb-vscode`/`debugpy`/`vscode-js-debug`.
- **Rationale:** Mesmo padrão do D3 (LSP) — cliente Rust contra servers externos.
- **Consequences:** Funciona onde adapter existe; suporte por linguagem é gradual.

### D15 — Streaming via canal MPSC tipo `PartialToolResult` por chunk
- **Decision:** Tools emitem `tokio::sync::mpsc::Sender<PartialToolResult>` (já existe trait); TUI consome com debounce de 50ms.
- **Rationale:** Trait estabelecida (`crates/theo-domain/src/tool.rs:88`); precisa só plumbar consumers.
- **Consequences:** Latência percebida cai; nada de novo na infra.

### D16 — RLHF dataset é apenas export, não treina dentro do Theo
- **Decision:** `theo trajectory export-rlhf` gera JSONL pronto para `axolotl`/`trl`; treinamento é fora do scope.
- **Rationale:** Manter Theo como agent runtime; treino é especialização separada.
- **Consequences:** Usuário precisa pipeline próprio; ganho: foco preservado.

## Dependency Graph

```
                        ┌──────────────────────┐
                        │ Phase 0: Foundations │
                        │ D1, D9 (schema bump) │
                        └──────────┬───────────┘
                                   │
              ┌────────────────────┼────────────────────┐
              ▼                    ▼                    ▼
     ┌─────────────┐      ┌─────────────┐      ┌─────────────┐
     │ P1: Multi-  │      │ P2: Browser │      │ P3: LSP     │
     │ modal (T1)  │      │ (T1)        │      │ (T1)        │
     └──────┬──────┘      └──────┬──────┘      └──────┬──────┘
            │                    │                    │
            └─────────┬──────────┴──────────┬─────────┘
                      ▼                     ▼
              ┌──────────────┐      ┌──────────────┐
              │ P4: Compu-   │      │ P5: Auto-    │
              │ ter Use (T1) │      │ test-gen (T1)│
              └──────┬───────┘      └──────┬───────┘
                     │                     │
                     └──────────┬──────────┘
                                ▼
                  ┌────────────────────────┐
                  │ P6: Replanning (T1)    │
                  │ — depends on Plan v2   │
                  └────────────┬───────────┘
                               ▼
                  ┌────────────────────────┐
                  │ P7: Multi-agent claim  │
                  │ (T1) — extends Plan    │
                  └────────────┬───────────┘
                               │
       ┌───────────────────────┼───────────────────────┐
       ▼                       ▼                       ▼
┌─────────────┐         ┌─────────────┐         ┌─────────────┐
│ P8: Reranker│         │ P9: Skills  │         │ P10: Cost   │
│ (T2)        │         │ market (T2) │         │ routing (T2)│
└──────┬──────┘         └──────┬──────┘         └──────┬──────┘
       │                       │                       │
       └───────────┬───────────┴───────────┬───────────┘
                   ▼                       ▼
        ┌────────────────────┐    ┌────────────────────┐
        │ P11: Compaction    │    │ P12: Eval CI (T2)  │
        │ (T2)               │    │                    │
        └─────────┬──────────┘    └─────────┬──────────┘
                  │                         │
                  └────────────┬────────────┘
                               ▼
       ┌───────────────────────┼───────────────────────┐
       ▼                       ▼                       ▼
┌─────────────┐         ┌─────────────┐         ┌─────────────┐
│ P13: DAP    │         │ P14: Stream │         │ P15: Ext    │
│ (T2)        │         │ UI (T2)     │         │ docs RAG(T2)│
└──────┬──────┘         └──────┬──────┘         └──────┬──────┘
       │                       │                       │
       └───────────────────────┼───────────────────────┘
                               ▼
                  ┌────────────────────────┐
                  │ P16: RLHF export (T2)  │
                  └────────────────────────┘
```

**Paralelizável:** P1/P2/P3 (depois de P0); P4/P5; P8/P9/P10 (depois de P7); P13/P14/P15.
**Bloqueador sequencial:** P0 → P1 → P6 → P7 → resto. P12 (CI) só faz sentido depois de P1+P3 (sinais reais).

---

## Phase 0: Foundations — schema bump

**Objective:** Migrar `Message.content` para blocks tipados e bumpar versions de plan/state para destravar multimodal.

### T0.1 — Bump `theo_infra_llm::types::Message` para `Vec<ContentBlock>`

#### Objective
Permitir mensagens com `text`, `image_url`, `image_base64` no payload OA-compat interno.

#### Evidence
`crates/theo-infra-llm/src/types.rs:18` mostra `pub content: Option<String>`. Anthropic e OpenAI ambos suportam content arrays. Sem isso, P1 (multimodal) e P4 (computer use) são impossíveis.

#### Files to edit
```
crates/theo-infra-llm/src/types.rs — adicionar `ContentBlock`, mudar `Message.content`
crates/theo-infra-llm/src/providers/anthropic.rs — adapter blocks→Anthropic API
crates/theo-infra-llm/src/providers/openai.rs — adapter blocks→OpenAI API
crates/theo-infra-llm/src/providers/*.rs — todos os 26 providers, transformar em `text` block na borda
crates/theo-domain/src/tool.rs — `FileAttachment::to_content_block()` helper
crates/theo-agent-runtime/src/state_manager.rs — schema_version bump v1→v2 + migration
crates/theo-agent-runtime/src/transcript_indexer.rs — handle ambos os formatos
```

#### Deep file dependency analysis
- `theo-infra-llm/types.rs` é importado por toda call chain (run_engine, agent_loop, pilot, state_manager). Mudar `content` quebra todas — exigir helpers `Message::text(s)` e `Message::with_image(url)` que produzem blocks.
- `state_manager.rs` persiste mensagens em `.theo/state/<run>/state.jsonl`. Schema v1 (`content: String`) precisa migrar para v2 (`content: Vec<Block>`). Migrate-on-load, save-as-v2.
- `transcript_indexer` faz BM25 sobre conteúdo; precisa concatenar text blocks.
- 26 provider adapters: cada `serialize_message` precisa transformar `ContentBlock::Text` em string (legacy providers) ou array (vision-capable).

#### Deep Dives

**`ContentBlock` enum:**
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ImageUrl { image_url: ImageUrlBlock },
    ImageBase64 { source: ImageSource },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageUrlBlock {
    pub url: String,
    #[serde(default)]
    pub detail: Option<ImageDetail>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageDetail { Low, High, Auto }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,  // "base64"
    pub media_type: String,   // "image/png"
    pub data: String,         // base64-encoded
}
```

**Migration path:**
```rust
impl Message {
    pub fn text(role: Role, body: impl Into<String>) -> Self {
        Self { role, content: Some(vec![ContentBlock::Text { text: body.into() }]), ... }
    }
    pub fn from_legacy_v1(legacy: LegacyMessage) -> Self { /* string → Vec<Text> */ }
}
```

**Invariant:** `content == Some(vec![])` é inválido (sem semântica). `content == None` significa apenas tool_call (assistant turn).

**Edge cases:** mensagens com content `Some("")` em v1 viram `Some(vec![Text { text: "" }])` para preservar bytes.

#### Tasks
1. Adicionar `ContentBlock` em `types.rs`.
2. Refator `Message.content` para `Option<Vec<ContentBlock>>`.
3. Helpers `Message::text/system/user/assistant/tool_result` mantidos com semântica antiga.
4. Adapter Anthropic: `ContentBlock::ImageUrl` → `{type:"image", source:...}`.
5. Adapter OpenAI: `ContentBlock::ImageUrl` → `{type:"image_url", image_url:{url}}`.
6. Adapters legacy: extrair text-only e enviar como string.
7. `state_manager`: bump SCHEMA_VERSION; load v1→v2.
8. Atualizar 26 providers para o novo serializador.

#### TDD
```
RED: t01_message_text_helper_produces_single_text_block
RED: t01_message_with_image_url_produces_two_blocks
RED: t01_legacy_v1_state_loads_as_v2_text_blocks
RED: t01_anthropic_serialize_image_url_emits_correct_shape
RED: t01_openai_serialize_image_url_emits_correct_shape
RED: t01_legacy_provider_drops_image_blocks_with_warning
RED: t01_state_manager_round_trip_v2_image_message
GREEN: implementar `ContentBlock` + adapters
REFACTOR: extrair `serialize_blocks_for_legacy(blocks: &[ContentBlock]) -> String` shared
VERIFY: cargo test -p theo-infra-llm; cargo test -p theo-agent-runtime state_manager
```

#### Acceptance Criteria
- [ ] `Message::text("hi")` produz `content: Some(vec![ContentBlock::Text{text:"hi"}])`
- [ ] Anthropic vision call passa smoke (real API ou mock-record)
- [ ] OpenAI vision call passa smoke
- [ ] State v1 carrega sem erro, salva como v2
- [ ] Providers legacy emitem warning ao receber image blocks (não panicam)
- [ ] cargo clippy --workspace -- -D warnings limpo
- [ ] Pass: /code-audit complexity (CCN ≤ 10 nos novos arquivos)
- [ ] Pass: /code-audit coverage (≥ 90% em `types.rs`)
- [ ] Pass: /code-audit lint (zero warnings)
- [ ] Pass: /code-audit size (≤ 500 lines em arquivos novos)

#### DoD
- [ ] All 7 RED tests passam
- [ ] cargo test -p theo-infra-llm green
- [ ] cargo test -p theo-agent-runtime green
- [ ] Backward compat: state v1 carrega
- [ ] code-audit checks passam

---

## Phase 1: Multimodal / Vision

**Objective:** Permitir input de imagens (screenshot, design mockup) no LLM via tool e via CLI.

### T1.1 — Tool `screenshot`

#### Objective
Tool nativa que captura tela inteira ou janela específica, retorna `ContentBlock::ImageBase64` no `ToolOutput`.

#### Evidence
Sem screenshot tool, agente não pode "ver" o estado atual de browser/desktop. Hermes-agent tem (`screenshots_tool.py`).

#### Files to edit
```
crates/theo-tooling/src/screenshot/mod.rs (NEW) — implementação
crates/theo-tooling/src/registry/mod.rs — registrar
crates/theo-tooling/src/tool_manifest.rs — entry
crates/theo-domain/src/tool.rs — `ToolOutput::with_image_block(block)` helper
crates/theo-agent-runtime/src/tool_bridge.rs — propagar image blocks ao Message
```

#### Deep file dependency analysis
- `screenshot/mod.rs` (new): usa `xcap` crate (screenshot cross-platform) ou shellando `gnome-screenshot`/`screencapture`. Linux requer X11/Wayland session.
- `tool_bridge.rs`: hoje stringifica `ToolOutput.output`. Precisa anexar image blocks de `metadata` ao Message do user response.
- `ToolContext` precisa flag `vision_enabled` para gate (D6).

#### Deep Dives

**Crate choice:** `xcap = "0.0.13"` é cross-platform pure-Rust. Alternative: `screenshots = "0.8"`. Escolha: `xcap` (mais ativo).

**Sandbox:** screenshots leem framebuffer — precisa permissão. Em Linux com bwrap, requer `--bind /tmp/.X11-unix /tmp/.X11-unix --setenv DISPLAY $DISPLAY`. Mitigação: tool roda fora do sandbox quando declara `requires_display(): true`.

**Output:** PNG comprimido base64 → `ImageSource{type:"base64", media_type:"image/png", data:base64}`. Resize para max 1568×1568 (Anthropic limit). Se maior, downscale via `image` crate.

#### Tasks
1. Adicionar `xcap = "0.0.13"` em workspace deps.
2. Criar `screenshot/mod.rs` com tool `screenshot`.
3. Schema params: `display: Option<u32>`, `region: Option<{x,y,w,h}>`, `format: png|jpeg`.
4. Captura → encode PNG → base64 → `ToolOutput.metadata.image_block`.
5. `tool_bridge` propaga `metadata.image_block` para `Message::user` do próximo turn.
6. Registrar (gate por `vision_enabled`).

#### TDD
```
RED: t11_screenshot_tool_returns_base64_png_metadata
RED: t11_screenshot_resizes_oversized_to_1568_max
RED: t11_screenshot_with_region_clips_correctly
RED: t11_tool_bridge_propagates_image_block_to_next_message
GREEN: implementar
REFACTOR: extrair `image_pipeline.rs` para encode/resize
VERIFY: cargo test -p theo-tooling screenshot
```

#### Acceptance Criteria
- [ ] Tool registrada em `DefaultRegistry` (gated por `vision_enabled`)
- [ ] Output traz `metadata.image_block` válido
- [ ] Headless test usa fixture PNG
- [ ] code-audit complexity ≤ 10
- [ ] code-audit coverage ≥ 90%
- [ ] code-audit size ≤ 500 lines

#### DoD
- [ ] 4 RED tests passam
- [ ] E2E manual: `theo "descreva esta tela" --vision` funciona
- [ ] code-audit OK

### T1.2 — Tool `read_image` (file → vision block)

#### Objective
Tool que lê PNG/JPEG/WebP do filesystem e retorna como vision block.

#### Files to edit
```
crates/theo-tooling/src/read_image/mod.rs (NEW)
crates/theo-tooling/src/registry/mod.rs
crates/theo-tooling/src/tool_manifest.rs
```

#### Deep file dependency analysis
Análoga a T1.1 mas sem captura — apenas leitura + encode. Reusa `image_pipeline.rs` extraído lá.

#### Deep Dives
**Validation:** rejeitar arquivos > 20MB (custo Anthropic). Detectar mime via magic bytes.
**Path safety:** mesma `path::absolutize` + `is_contained` que `read` tool.

#### Tasks
1. Criar tool reusando pipeline.
2. Validar mime + size.
3. Permission `ExternalDirectory` se fora do projeto.

#### TDD
```
RED: t12_read_image_returns_block_for_png
RED: t12_read_image_rejects_oversized_file
RED: t12_read_image_records_external_permission_for_outside_paths
GREEN: implementar
VERIFY: cargo test -p theo-tooling read_image
```

#### Acceptance Criteria
- [ ] PNG/JPEG/WebP suportados
- [ ] Reject > 20MB com mensagem actionable
- [ ] code-audit OK

#### DoD
- [ ] 3 RED tests passam
- [ ] code-audit OK

---

## Phase 2: Browser automation

### T2.1 — `browser` tool family via Playwright sidecar

#### Objective
Tools `browser_open`, `browser_click`, `browser_type`, `browser_screenshot`, `browser_eval`, `browser_close` operando contra chromium headless via Playwright Node sidecar.

#### Evidence
Cursor, Lovable, Bolt fazem isso; Theo zero. Tasks E2E (testar URL, scrape autorizado, design-to-code) são impossíveis sem.

#### Files to edit
```
crates/theo-tooling/src/browser/mod.rs (NEW)
crates/theo-tooling/src/browser/sidecar.rs (NEW) — gerenciamento do processo Playwright
crates/theo-tooling/src/browser/cdp_client.rs (NEW) — WebSocket CDP wrapper
crates/theo-tooling/scripts/playwright_sidecar.js (NEW) — Node sidecar
crates/theo-tooling/src/registry/mod.rs
crates/theo-tooling/src/tool_manifest.rs
```

#### Deep file dependency analysis
- `sidecar.rs` lifecycles um `tokio::process::Child` rodando o JS sidecar; expõe `BrowserSession` handle.
- `cdp_client.rs` usa `tokio-tungstenite` para WebSocket; reusa pattern do `theo-infra-mcp::transport_http`.
- Sandbox: chromium headless dentro de namespace; bwrap profile específico.
- Cleanup: Drop kills sidecar; signal handlers garantem limpeza.

#### Deep Dives

**Sidecar protocol:** JSON-RPC sobre WS local (`ws://127.0.0.1:<port>`). Métodos: `open(url)`, `click(selector)`, `type(selector, text)`, `screenshot({fullPage,format})`, `eval(js)`, `wait(selector|ms)`, `close()`.

**Provisioning:** `npx playwright install chromium` no primeiro `theo browser open`. Idempotente; cache em `~/.cache/theo/playwright`.

**Vision integration:** `browser_screenshot` retorna ImageBase64 block (T0.1) — fechando loop com vision.

**Security:** `Capability::Browser` por padrão off; usuário ativa via `--allow-browser` ou `.theo/config.toml`.

#### Tasks
1. Decidir crate WS (`tokio-tungstenite`).
2. Sidecar JS (~150 linhas, autocontido).
3. `BrowserSession` struct com sidecar handle + WS.
4. 6 tools: open, click, type, screenshot, eval, close.
5. `Capability::Browser` adicionado a `CapabilitySet`.
6. Bundle ou doc Node prerequisite.

#### TDD
```
RED: t21_browser_open_navigates_to_url
RED: t21_browser_screenshot_returns_image_block
RED: t21_browser_click_dispatches_event
RED: t21_browser_type_fills_input
RED: t21_browser_eval_returns_value
RED: t21_capability_browser_off_blocks_open
RED: t21_sidecar_killed_on_drop
GREEN: implementar
REFACTOR: extrair `JsonRpcWs` reusable para outros sidecars (DAP em P13)
VERIFY: cargo test -p theo-tooling browser -- --test-threads=1
```

#### Acceptance Criteria
- [ ] 7 testes passam (E2E contra `http://localhost:<port>` mock server)
- [ ] Capability gate funciona
- [ ] code-audit OK

#### DoD
- [ ] cargo test verde
- [ ] Sidecar não-leak entre runs (verificado por `pgrep`)

---

## Phase 3: LSP real

### T3.1 — `lsp-client` crate + integração com `rust-analyzer`/`pyright`/`tsserver`

#### Objective
Substituir stub `lsp/mod.rs` por cliente LSP real com `rename_symbol`, `find_references`, `goto_definition`, `hover`, `code_actions`.

#### Evidence
`lsp/mod.rs` está marcado `Stub` no manifest. Refatorações grandes (renomear símbolo cross-file, encontrar todos usuários) são impossíveis sem.

#### Files to edit
```
crates/theo-tooling/src/lsp/client.rs (NEW)
crates/theo-tooling/src/lsp/discovery.rs (NEW) — encontrar servers no PATH
crates/theo-tooling/src/lsp/operations.rs (NEW) — ops mapeadas para JSON-RPC
crates/theo-tooling/src/lsp/mod.rs — substituir stub
Cargo.toml workspace — adicionar `lsp-types = "0.97"`
```

#### Deep file dependency analysis
- `discovery.rs`: detecta servers (`rust-analyzer --version`, `pyright --version`, `typescript-language-server --stdio`). Cache em `~/.cache/theo/lsp.toml`.
- `client.rs`: maneja stdio JSON-RPC com framing LSP (`Content-Length: N\r\n\r\n{json}`). Reusa `tower-lsp` patterns.
- `operations.rs`: maps LSP `textDocument/rename`, `textDocument/references`, `textDocument/definition`, `textDocument/hover`, `textDocument/codeAction`.

#### Deep Dives

**Server lifecycle:** lazy spawn; cached por (project_dir, language). Reutilizado entre tool calls da mesma sessão. Drop encerra com `shutdown` LSP.

**Workspace folders:** `initialize` envia `workspaceFolders: [{uri: file://<project_dir>}]`. Updates via `didOpen`/`didChange` quando agente edita.

**Edit application:** `WorkspaceEdit` retornado por rename → aplica via `apply_patch` tool internamente; agente pode revisar antes de commit.

#### Tasks
1. Adicionar `lsp-types`.
2. `discovery.rs` para 3 servers iniciais.
3. `client.rs` com framing.
4. `operations.rs` para 5 ops.
5. Schema da tool com `op` discriminator.

#### TDD
```
RED: t31_discovery_finds_rust_analyzer_in_path
RED: t31_client_initializes_and_handles_response
RED: t31_rename_symbol_returns_workspace_edit
RED: t31_find_references_returns_locations
RED: t31_goto_definition_returns_location
RED: t31_hover_returns_markdown
RED: t31_session_reused_within_project
GREEN: implementar
REFACTOR: extrair `JsonRpcStdio` shared para DAP (P13)
VERIFY: cargo test -p theo-tooling lsp -- --test-threads=1
```

#### Acceptance Criteria
- [ ] 7 testes passam (mock LSP server fixture)
- [ ] Manifest entry muda para `Implemented`
- [ ] code-audit OK

#### DoD
- [ ] cargo test verde
- [ ] E2E manual: `theo "renomeie X para Y em todo o projeto"` funciona contra rust-analyzer

---

## Phase 4: Computer Use

### T4.1 — Anthropic Computer Use adapter + tool family

#### Objective
Tool `computer_screenshot/click/type/key/scroll` mapeada à Anthropic Computer Use API (`computer_20250124`).

#### Evidence
Anthropic já expõe; Theo não consome. P1 (vision) e P2 (browser) cobrem ~70% dos casos, mas Computer Use cobre apps GUI tradicionais (admins, dashboards proprietários).

#### Files to edit
```
crates/theo-infra-llm/src/providers/anthropic.rs — registrar tool name `computer_20250124`
crates/theo-tooling/src/computer/mod.rs (NEW)
crates/theo-tooling/src/registry/mod.rs — gate por provider (D6)
```

#### Deep file dependency analysis
- `anthropic.rs`: tool definition tem schema próprio (`type: computer_20250124, display_width_px, display_height_px, display_number`). Theo precisa expor ao Anthropic *além* das tools normais.
- `computer/mod.rs`: cliente x11/Wayland que executa actions. Linux: `xdotool`. macOS: `cliclick`. Windows: `nircmd`.

#### Deep Dives

**Action mapping:**
- `screenshot` → reusa T1.1 internally
- `click(x,y,button)` → `xdotool mousemove $x $y click $button`
- `type(text)` → `xdotool type "$text"`
- `key(name)` → `xdotool key $name`
- `scroll(direction,amount)` → `xdotool key "Page_Down"` etc.

**Capability gate:** `Capability::ComputerUse` default off. Risco alto.

#### Tasks
1. Adicionar tool `computer_20250124` ao Anthropic provider.
2. Implementar `computer/mod.rs` com xdotool/cliclick wrappers.
3. Capability gate.

#### TDD
```
RED: t41_anthropic_serialize_includes_computer_tool_when_enabled
RED: t41_computer_screenshot_returns_image_block
RED: t41_computer_click_dispatches_xdotool
RED: t41_capability_off_blocks_action
GREEN: implementar
VERIFY: cargo test -p theo-infra-llm computer; cargo test -p theo-tooling computer
```

#### Acceptance Criteria
- [ ] 4 testes passam
- [ ] Linux smoke E2E (CI roda Xvfb)
- [ ] code-audit OK

#### DoD
- [ ] cargo test verde
- [ ] code-audit OK

---

## Phase 5: Auto-test-generation

### T5.1 — Tool `gen_property_test` via proptest

#### Objective
Tool que recebe `function_signature` e gera arquivo de testes proptest.

#### Files to edit
```
crates/theo-tooling/src/test_gen/property.rs (NEW)
crates/theo-tooling/src/test_gen/mod.rs (NEW)
crates/theo-tooling/src/registry/mod.rs
```

#### Deep file dependency analysis
- Tool não roda proptest; apenas *gera* o arquivo. Execução é via `bash` tool depois.
- LLM compõe estratégias proptest a partir do tipo da function. Tool fornece template + scaffolding.

#### Deep Dives

**Schema:**
```
function_path: "src/foo.rs"
function_name: "calculate_tax"
strategies: ["any::<f64>()", "any::<u32>()"]
output_path: "tests/calculate_tax_property.rs"
```

#### Tasks
1. Template scaffolding com `proptest::prop_compose!` e `proptest!`.
2. Tool `gen_property_test`.
3. Doc + few-shot example no system prompt.

#### TDD
```
RED: t51_gen_property_test_creates_compilable_file
RED: t51_gen_property_test_includes_function_import
RED: t51_invalid_strategy_returns_error
GREEN: implementar
VERIFY: cargo test -p theo-tooling test_gen
```

#### Acceptance Criteria
- [ ] Arquivo gerado compila com `cargo check --tests`
- [ ] code-audit OK

#### DoD
- [ ] 3 RED tests passam
- [ ] E2E manual: `theo "gere property tests para fn calculate_tax"` produz arquivo válido

### T5.2 — Tool `gen_mutation_test` via cargo-mutants

#### Objective
Tool que invoca `cargo-mutants --check` e relata sobreviventes; opcional gera commit pra mata-los.

#### Files to edit
```
crates/theo-tooling/src/test_gen/mutation.rs (NEW)
```

#### Tasks
1. Wrap `cargo-mutants --json --in-place=false`.
2. Parse `mutants.out/outcomes.json`.
3. Retornar lista de mutações sobreviventes com sugestões.

#### TDD
```
RED: t52_mutation_test_parses_outcomes_json
RED: t52_mutation_test_returns_survivors_only
GREEN: implementar
VERIFY: cargo test -p theo-tooling test_gen::mutation
```

#### Acceptance Criteria
- [ ] Parser handles cargo-mutants 24+
- [ ] code-audit OK

#### DoD
- [ ] 2 RED tests passam

---

## Phase 6: Adaptive replanning

### T6.1 — `Plan::replan(failure_context)` + `replan` tool

#### Objective
Quando `next_actionable_task` retorna mesma task já falhou ≥N vezes, chamar LLM para mutar o plano.

#### Evidence
`run_from_plan` (commit `37cb3b2`) tem comentário "T1 SOTA feedback-loop foundation". GoalAct paper reporta +12% success com replanning. Hoje, falha repetida loop infinito até max_calls.

#### Files to edit
```
crates/theo-domain/src/plan.rs — adicionar `Plan::replan_with(patch: PlanPatch)`
crates/theo-domain/src/plan_patch.rs (NEW) — `PlanPatch` enum (AddTask, RemoveTask, EditTask, ReorderDeps)
crates/theo-tooling/src/plan/mod.rs — tool `plan_replan(failure_context, llm_proposed_patch)`
crates/theo-agent-runtime/src/pilot/mod.rs — gatilho automático após N retries
crates/theo-application/src/use_cases/replan_advisor.rs (NEW) — chama LLM com prompt template
```

#### Deep file dependency analysis
- `plan_patch.rs`: tipos para mutações tipadas (não free-form JSON merge); valida invariants pós-aplicação (re-roda `plan.validate()`).
- `pilot/mod.rs`: contador `replan_attempts` por task; após threshold (config), chama `replan_advisor`.
- `replan_advisor.rs`: usa LLM com system prompt "você é um planejador. Plano + falha. Proponha PlanPatch.".

#### Deep Dives

**`PlanPatch`:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PlanPatch {
    AddTask { phase: PhaseId, task: PlanTask, position: InsertPosition },
    RemoveTask { id: PlanTaskId },
    EditTask { id: PlanTaskId, edits: TaskEdits },
    ReorderDeps { id: PlanTaskId, new_deps: Vec<PlanTaskId> },
    SkipTask { id: PlanTaskId, rationale: String },
}
```

**Apply algorithm:**
1. Clone plan.
2. Apply patch.
3. `clone.validate()` — reject patch that breaks DAG.
4. Persist via `plan_store::save_plan`.

**Trigger:** `task.status == Failed && task_failure_count(id) >= config.replan_threshold (default: 2)`.

**Prompt template:** carrega plano current + último error + outcome → pede patch mínimo.

#### Tasks
1. `PlanPatch` enum em theo-domain.
2. `Plan::apply_patch(patch) -> Result<(), PlanValidationError>`.
3. `plan_replan` tool em theo-tooling.
4. `replan_advisor` use case em theo-application.
5. Hook em pilot `run_from_plan` para autofire.

#### TDD
```
RED: t61_apply_patch_add_task_validates
RED: t61_apply_patch_remove_task_rejects_orphan_dep
RED: t61_apply_patch_skip_task_marks_skipped
RED: t61_replan_tool_persists_patched_plan
RED: t61_pilot_autofires_replan_after_n_failures
RED: t61_replan_does_not_loop_infinitely
GREEN: implementar
REFACTOR: extrair `apply_*_patch` privados
VERIFY: cargo test -p theo-domain plan; cargo test -p theo-agent-runtime pilot
```

#### Acceptance Criteria
- [ ] 6 RED tests passam
- [ ] Bench manual: tasks que antes loop-infinitam agora reroteiam
- [ ] code-audit OK

#### DoD
- [ ] cargo test verde
- [ ] Plan v1 → v2 schema bump (D9 já cobre)

---

## Phase 7: Multi-agent claim + parallel

### T7.1 — `Plan::claim_task` + `assignee` field + worktree-per-agent

#### Objective
N sub-agents executam `next_actionable_task` em paralelo, cada um em worktree próprio.

#### Files to edit
```
crates/theo-domain/src/plan.rs — `PlanTask.assignee: Option<String>`, `claim_task`/`release_task`
crates/theo-agent-runtime/src/plan_store.rs — `claim_task(path, id, agent, expected_version)`
crates/theo-agent-runtime/src/pilot/mod.rs — `run_parallel_from_plan(n_workers)`
crates/theo-agent-runtime/src/subagent/manager.rs — pool ↔ plan integration
```

#### Deep file dependency analysis
- `plan_store::claim_task`: read-modify-write atômico via `version` field bumped a cada save (CAS).
- `pilot::run_parallel_from_plan`: spawn N `SubAgentManager::spawn_with_spec_with_override` com `WorktreeStrategy::Recreate`.
- Cada sub-agent é uma task `tokio::spawn` com canal de signaling.

#### Deep Dives

**CAS protocol:**
```rust
pub fn claim_task(path, task_id, agent_id) -> Result<ClaimResult> {
    loop {
        let plan = load_plan(path)?;
        let version = plan.version_counter; // novo campo, monotônico
        let task = plan.find_task(task_id)?;
        if task.assignee.is_some() {
            return Ok(ClaimResult::AlreadyClaimed);
        }
        let mut updated = plan.clone();
        updated.find_task_mut(task_id)?.assignee = Some(agent_id);
        updated.version_counter = version + 1;
        match save_plan_if_version(path, &updated, expected = version) {
            Ok(()) => return Ok(ClaimResult::Claimed),
            Err(VersionMismatch) => continue, // retry
            Err(e) => return Err(e),
        }
    }
}
```

**Worker loop:**
```rust
while let Some(task) = plan.next_actionable_task() {
    match plan_store::claim_task(path, task.id, agent_id) {
        Ok(Claimed) => { execute(task); release(task); }
        Ok(AlreadyClaimed) => continue, // outro worker pegou
        Err(e) => { log; break; }
    }
}
```

#### Tasks
1. Adicionar `assignee` + `version_counter` em PlanTask/Plan.
2. `save_plan_if_version` (CAS).
3. `claim_task`/`release_task` helpers.
4. `run_parallel_from_plan(n)` em pilot.
5. Worker loop com canal de erro.

#### TDD
```
RED: t71_claim_succeeds_when_unclaimed
RED: t71_claim_fails_when_already_claimed
RED: t71_concurrent_claim_one_winner
RED: t71_release_clears_assignee
RED: t71_run_parallel_completes_all_tasks
RED: t71_worker_failure_does_not_kill_others
GREEN: implementar
REFACTOR: extrair `Worker` struct com lifecycle
VERIFY: cargo test -p theo-agent-runtime plan_store::claim; cargo test pilot::parallel
```

#### Acceptance Criteria
- [ ] 6 RED tests passam (1 com `tokio::spawn` para concorrência)
- [ ] Bench: 4-task plan executa em paralelo (3 workers)
- [ ] code-audit OK

#### DoD
- [ ] cargo test verde

---

## Phase 8: Cross-encoder reranker (T2)

### T8.1 — Always-on reranker, runtime gate

#### Objective
Cabar `CrossEncoderReranker` na pipeline RRF default; gate via config, não feature.

#### Files to edit
```
crates/theo-engine-retrieval/Cargo.toml — remover `feature = "reranker"`
crates/theo-engine-retrieval/src/lib.rs — sempre exportar
crates/theo-engine-retrieval/src/pipeline.rs — adicionar reranker stage
crates/theo-engine-retrieval/src/reranker.rs — remover `#[cfg]`
```

#### Deep file dependency analysis
`pipeline.rs` orquestra BM25 → dense → tantivy → fusion. Reranker entra como stage 5 (top-50 → top-K).

#### Deep Dives

**Lazy load:** modelo ONNX baixado on first use; ~50MB. Cache em `~/.cache/fastembed`.
**Opt-out:** `THEO_NO_RERANK=1` ou `RetrievalConfig.use_reranker=false`.

#### Tasks
1. Remover feature flag.
2. Cabar no pipeline.
3. Métrica: `reranker_latency_ms` em metrics.

#### TDD
```
RED: t81_pipeline_with_reranker_returns_higher_ndcg_on_fixture
RED: t81_reranker_disabled_skips_stage
RED: t81_reranker_lazy_loads_on_first_use
GREEN: implementar
VERIFY: cargo test -p theo-engine-retrieval reranker
```

#### Acceptance Criteria
- [ ] NDCG@5 sobe ≥ 10% no fixture
- [ ] code-audit OK

#### DoD
- [ ] 3 RED tests passam

---

## Phase 9: Skill marketplace (T2)

### T9.1 — `skill_catalog` wired + `theo skill install/list/view`

#### Objective
Remover `#[allow(dead_code)]` do `skill_catalog.rs`; cabar nos AgentLoop e CLI.

#### Files to edit
```
crates/theo-agent-runtime/src/skill_catalog.rs — remover allow, expor traits
crates/theo-application/src/use_cases/skills.rs (NEW) — install/list/view use cases
apps/theo-cli/src/main.rs — subcommand `skill`
```

#### Deep file dependency analysis
- `skill_catalog::list_skills(home)`: já existe, dead code. Apenas precisa caller.
- Install: download de URL → validate frontmatter → save em `~/.theo/skills/<name>/SKILL.md`.

#### Deep Dives
**Source:** initial registry hardcoded em `apps/theo-cli/src/skills/registry.toml` com URLs assinadas. Future: marketplace HTTP service.

#### Tasks
1. Cabar `skill_catalog`.
2. CLI `theo skill list/install/view/remove`.
3. AgentLoop carrega skills via tier 1 (lista) → injeta no system prompt.
4. Tier 2 (full body) via tool `skill_view`.

#### TDD
```
RED: t91_skill_install_downloads_and_validates
RED: t91_skill_list_returns_metadata_only
RED: t91_skill_view_returns_full_body
RED: t91_invalid_frontmatter_rejected
GREEN: implementar
VERIFY: cargo test -p theo-agent-runtime skill_catalog
```

#### Acceptance Criteria
- [ ] 4 RED tests passam
- [ ] CLI smoke: install + list + view
- [ ] code-audit OK

#### DoD
- [ ] cargo test verde

---

## Phase 10: Cost-aware routing (T2)

### T10.1 — `ComplexityClassifier` cabo no AgentLoop

#### Objective
AgentLoop classifica task → tier (Simple/Medium/Complex) → modelo (Haiku/Sonnet/Opus).

#### Files to edit
```
crates/theo-infra-llm/src/routing/auto.rs — já existe; cabar no AgentLoop
crates/theo-agent-runtime/src/agent_loop.rs — consumir router
crates/theo-agent-runtime/src/config.rs — `RoutingConfig.cost_aware: bool`
```

#### Tasks
1. AgentLoop chama `router.route(ctx)` antes de cada LLM call.
2. Métrica `routing_decision{tier,model}`.
3. A/B harness valida custo vs success.

#### TDD
```
RED: t101_simple_task_routed_to_cheap_model
RED: t101_complex_task_routed_to_capable_model
RED: t101_override_respected
GREEN: cabar
VERIFY: cargo test -p theo-agent-runtime agent_loop::routing
```

#### Acceptance Criteria
- [ ] A/B mostra ≥20% redução custo, ≤5% queda success
- [ ] code-audit OK

#### DoD
- [ ] cargo test verde

---

## Phase 11: Compaction stages on (T2)

### T11.1 — Wire `compaction_stages` + `compaction_summary`

#### Objective
Remover `#[allow(dead_code)]` dos stages; cabar Prune+Compact após threshold.

#### Files to edit
```
crates/theo-agent-runtime/src/compaction_stages.rs — remover allow
crates/theo-agent-runtime/src/compaction_summary.rs — remover allow
crates/theo-agent-runtime/src/compaction/mod.rs — adicionar stages ao policy engine
crates/theo-application/src/use_cases/auxiliary_llm.rs (NEW) — cliente Haiku/mini
```

#### Tasks
1. Cabar Prune.
2. Cabar Compact com auxiliary LLM.
3. Métricas tokens-saved.

#### TDD
```
RED: t111_prune_reduces_tool_results_above_threshold
RED: t111_compact_summarizes_via_auxiliary
RED: t111_aggressive_only_when_budget_critical
GREEN: cabar
VERIFY: cargo test compaction
```

#### Acceptance Criteria
- [ ] Token reduction ≥40% em runs longos
- [ ] code-audit OK

#### DoD
- [ ] 3 RED tests passam

---

## Phase 12: Continuous SOTA evaluation (T2)

### T12.1 — GitHub Actions `eval` job

#### Objective
PR runs reduced terminal-bench (10 tasks); main runs full nightly.

#### Files to edit
```
.github/workflows/eval.yml (NEW)
apps/theo-benchmark/runner/ci_smoke.py (NEW) — entrada CI
docs/audit/eval-baseline.md (NEW) — baseline 2026-04-26
```

#### Tasks
1. Workflow com `[bench]` label gate.
2. CI smoke roda com Groq free tier.
3. Posta comment no PR com diff vs baseline.

#### Acceptance Criteria
- [ ] PR sem `[bench]` skip eval
- [ ] PR com `[bench]` roda em <15min
- [ ] Comment automático

#### DoD
- [ ] Workflow merged e funciona em PR de teste

---

## Phase 13: DAP integration (T2)

### T13.1 — `dap-client` + tool family `debug_*`

#### Objective
Tools `debug_set_breakpoint`, `debug_step`, `debug_eval`, `debug_watch` contra `lldb-vscode`/`debugpy`/`vscode-js-debug`.

#### Files to edit
```
crates/theo-tooling/src/dap/client.rs (NEW)
crates/theo-tooling/src/dap/discovery.rs (NEW)
crates/theo-tooling/src/dap/operations.rs (NEW)
crates/theo-tooling/src/dap/mod.rs (NEW)
```

#### Deep file dependency analysis
Análogo a P3 (LSP). Reusa `JsonRpcStdio` extraído lá. DAP usa Content-Length framing similar.

#### Tasks
1. Adicionar `dap = "0.4"`.
2. Discovery dos 3 adapters.
3. 4 tools.

#### TDD
```
RED: t131_set_breakpoint_acks
RED: t131_step_advances_pc
RED: t131_eval_returns_value
RED: t131_watch_persists_across_steps
GREEN: implementar
VERIFY: cargo test dap
```

#### Acceptance Criteria
- [ ] 4 RED tests passam (mock DAP server)
- [ ] code-audit OK

#### DoD
- [ ] cargo test verde

---

## Phase 14: Live tool streaming UI (T2)

### T14.1 — `PartialToolResult` plumbing + TUI live render

#### Objective
Tools de longa duração (bash, plan_*, browser_*) emitem chunks; TUI renderiza com debounce.

#### Files to edit
```
crates/theo-tooling/src/plan/mod.rs — emitir partial em `plan_create` (parsing progress)
crates/theo-tooling/src/browser/mod.rs — emitir partial em `browser_eval`
apps/theo-cli/src/render/streaming.rs (NEW) — debounce + live render
apps/theo-cli/src/tui/* — consumir
```

#### Tasks
1. Tools chamam `ctx.partial_tx.send(PartialToolResult{...})`.
2. CLI/TUI listener com 50ms debounce.
3. Markdown live rendering via `pulldown-cmark`.

#### TDD
```
RED: t141_bash_emits_partial_per_line
RED: t141_browser_eval_emits_progress
RED: t141_tui_debounces_50ms
GREEN: implementar
VERIFY: cargo test render
```

#### Acceptance Criteria
- [ ] Latência percebida cai (medida manual)
- [ ] code-audit OK

#### DoD
- [ ] 3 RED tests passam

---

## Phase 15: External docs RAG (T2)

### T15.1 — Tool `docs_search` + index Tantivy local

#### Objective
Indexar docs.rs/MDN/npm sob demanda em `~/.cache/theo/docs/<lang>.tantivy`; tool busca cross-source.

#### Files to edit
```
crates/theo-tooling/src/docs_search/mod.rs (NEW)
crates/theo-tooling/src/docs_search/sources.rs (NEW) — fetchers crates.io, MDN, npm
crates/theo-tooling/src/docs_search/index.rs (NEW) — Tantivy wrapper
```

#### Tasks
1. Source adapters (3 inicial).
2. Tantivy schema doc-level.
3. Tool com `query, source?, top_k`.

#### TDD
```
RED: t151_index_crates_io_docs
RED: t151_search_returns_top_k
RED: t151_source_filter_works
GREEN: implementar
VERIFY: cargo test docs_search
```

#### Acceptance Criteria
- [ ] 3 RED tests passam (fixture HTML)
- [ ] code-audit OK

#### DoD
- [ ] cargo test verde

---

## Phase 16: RLHF feedback export (T2)

### T16.1 — Trajectory rating + export tool

#### Objective
Adicionar `rating: Option<i8>` em trajectory entries; CLI `theo trajectory export-rlhf <out.jsonl>`.

#### Files to edit
```
crates/theo-domain/src/event.rs — `EventType::TurnRated`
crates/theo-agent-runtime/src/observability/trajectories.rs — rating field
apps/theo-cli/src/trajectory.rs (NEW) — export command
```

#### Tasks
1. Schema rating.
2. CLI command `/rate +1` no TUI.
3. Export → DPO/PPO-ready JSONL.

#### TDD
```
RED: t161_trajectory_persists_rating
RED: t161_export_filters_by_rating
RED: t161_export_format_compatible_with_axolotl
GREEN: implementar
VERIFY: cargo test trajectory
```

#### Acceptance Criteria
- [ ] 3 RED tests passam
- [ ] Output valida contra axolotl loader
- [ ] code-audit OK

#### DoD
- [ ] cargo test verde

---

## Coverage Matrix

| # | Tier | Gap | Phase / Task | Resolution |
|---|------|-----|--------------|------------|
| 1 | T1 | Multimodal/vision | P0/T0.1 + P1/T1.1, T1.2 | `ContentBlock` enum + screenshot/read_image tools |
| 2 | T1 | Browser automation | P2/T2.1 | Playwright sidecar + 6 browser_* tools |
| 3 | T1 | LSP real | P3/T3.1 | Cliente LSP contra rust-analyzer/pyright/tsserver |
| 4 | T1 | Adaptive replanning | P6/T6.1 | Plan::apply_patch + replan tool + auto-trigger |
| 5 | T1 | Multi-agent paralelo | P7/T7.1 | claim_task CAS + run_parallel_from_plan |
| 6 | T1 | Computer Use | P4/T4.1 | Anthropic adapter + computer/* tools |
| 7 | T1 | Auto-test-gen | P5/T5.1, T5.2 | gen_property_test + gen_mutation_test |
| 8 | T2 | Cross-encoder reranker | P8/T8.1 | Always-on, config-gate |
| 9 | T2 | Skill marketplace | P9/T9.1 | skill_catalog wired + CLI subcommand |
| 10 | T2 | Cost routing | P10/T10.1 | AutomaticModelRouter cabo no AgentLoop |
| 11 | T2 | Compactação | P11/T11.1 | Prune + Compact stages on |
| 12 | T2 | Eval CI | P12/T12.1 | GitHub Actions workflow |
| 13 | T2 | DAP | P13/T13.1 | dap-client + debug_* tools |
| 14 | T2 | Streaming UI | P14/T14.1 | PartialToolResult plumbing |
| 15 | T2 | External docs RAG | P15/T15.1 | docs_search tool + Tantivy index |
| 16 | T2 | RLHF feedback | P16/T16.1 | rating + export-rlhf command |

**Coverage: 16/16 gaps cobertos (100%)**

## Global Definition of Done

- [ ] All 16 phases completed
- [ ] All RED tests passing (estimated total: 75+ new tests)
- [ ] cargo test --workspace green (excluding theo-desktop/marklive system-dep)
- [ ] cargo clippy --workspace -- -D warnings green
- [ ] Backward compatibility: state v1 plans/transcripts carregam sem erro
- [ ] code-audit checks (complexity, coverage, lint, size) verde em TODOS os crates modificados
- [ ] CHANGELOG.md atualizado com entrada `[Unreleased]/Added` por phase
- [ ] ADRs D1–D16 referenciados nos commits relevantes
- [ ] arch contract: 0 violations
- [ ] SWE-Bench-Verified ou terminal-bench reduced em CI mostra ≥10pt acima do baseline `37cb3b2`
- [ ] Cobertura de tier mensurável: T1 (7/7), T2 (9/9)
