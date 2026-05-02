---
id: 20260426-181501
date: 2026-04-26
topic: "SOTA Tool Search — Deferred Tool Discovery System"
verdict: REVISED
participants: 16
---

# Reuniao: SOTA Tool Search — Deferred Tool Discovery System

## Pauta

**Contexto:** O Theo envia 35 tool definitions por LLM turn (~6-8K tokens). Infraestrutura de deferral existe (should_defer, search_hint, tool_search meta-tool) mas esta idle — nenhuma tool real usa deferral. O tool_search retorna apenas (id, hint) sem schema completo. Claude Code mediu 85% de reducao de tokens e salto de accuracy de 49% para 74% com tool search habilitado.

**Questoes a decidir:**
1. Enriquecer tool_search para retornar schemas completos?
2. Quais tools deferir e com qual criterio?
3. Deferral contextual por modo do agente?
4. Scoring/ranking inteligente vs substring match?
5. Taxonomia: nova variante Deferred em ToolExposure?

**Restricoes:**
- Dependency contract inviolavel (theo-tooling -> theo-domain only)
- TDD obrigatorio (RED-GREEN-REFACTOR)
- Sem quebra de API existente sem migracao

## Posicoes por Agente

### Estrategia

| Agente | Posicao | Resumo |
|--------|---------|--------|
| chief-architect | APPROVE | Arquiteturalmente sound. Ativacao de capacidade ja projetada, nao arquitetura nova. Scoring em tool_bridge, nao em theo-tooling. Rollout incremental obrigatorio. |
| evolution-agent | APPROVE | Maior leverage disponivel agora. 4-5K tokens/turn salvos. Sequenciar: schemas primeiro, instrumentar, deferir, contextual por ultimo. Fallback auto-search para tool nao encontrada. |

### Conhecimento

| Agente | Posicao | Resumo |
|--------|---------|--------|
| knowledge-compiler | APPROVE | Wiki precisa de visibility_tier no frontmatter. Criar pagina conceito tool-search. Source of truth continua tool_manifest.rs. |
| ontology-manager | REJECT (parcial) | REJEITA Deferred como variante de ToolExposure. Deferral e decisao de runtime, nao de registro. Introduzir ToolSelectionPolicy separado. |
| data-ingestor | APPROVE | Tool metadata e ingestivel. Trigger por checksum de tool_manifest.rs. Docs sao read-only indexes. |
| wiki-expert | APPROVE | Deferred tools devem ter paginas wiki existentes com badge status:deferred. BM25 search precisa diferenciar visualmente. |

### Qualidade

| Agente | Posicao | Resumo |
|--------|---------|--------|
| validator | CONCERN | Degradacao silenciosa e o risco principal. Escape hatch obrigatorio (query "*" lista todos). Nao mudar Tool trait. Schema opt-in via include_schema param. |
| linter | CONCERN | CLAUDE.md diz "21 default registry" mas registry tem 27 — drift critico ja existente. Corrigir ANTES de prosseguir. Adicionar teste de contagem visivel. |
| retrieval-engineer | APPROVE | BM25 e overkill para 45 tools. Weighted token overlap suficiente (~60 linhas Rust). Campo weights: id 5x, hint 1x, prefix bonus +3. Top-K=5. Sem dependencia externa. |
| memory-synthesizer | APPROVE (cond) | Instrumentar primeiro (measure before schema). Emitir eventos para .theo/metrics/ como JSONL. Deferral estatico primeiro, frequency-driven depois. |

### Engineering

| Agente | Posicao | Resumo |
|--------|---------|--------|
| code-reviewer | CONCERN | 4 issues: (1) return type de search_deferred e breaking change, (2) Tool trait change para contextual deferral e invasivo demais, (3) scoring precisa de teste oracle, (4) CLAUDE.md invariant desatualizado. DeferralPolicy via OCP, nao mutacao de trait. |
| graphctx-expert | APPROVE | Scoring DEVE ser self-contained em theo-tooling. Nao puxar theo-engine-retrieval (violaria contrato). BM25 hand-rolled ~30 linhas. Trait ToolRanker em theo-domain para extensibilidade futura. |
| arch-validator | APPROVE (cond) | Boundaries respeitadas. AgentMode como parametro, nao estado. Scoring local em theo-tooling. Cap resultados. check-arch-contract.sh deve passar. |
| test-runner | APPROVE | TDD viavel. 5 novos testes, 4 existentes atualizados. Fases: RED (score struct), GREEN (scoring logic), REFACTOR (schema field), ACCEPT (defer 15). |
| frontend-dev | APPROVE (cond) | Precisa de evento ToolDiscovered no SSE stream. tool_search renderizado como meta-step. Tool palette read-only no sidebar (v2). |

### Pesquisa

| Agente | Posicao | Resumo |
|--------|---------|--------|
| research-agent | APPROVE | Anthropic mediu 85% token reduction, 49%->74% accuracy. Claude Code v2.1.69 defere ALL built-in tools. OpenDev: 40%->5% startup context. Sequencia: instrumentar -> deferir -> scoring -> contextual. |

## Conflitos

### Conflito 1: ToolExposure::Deferred vs ToolSelectionPolicy separado

**ontology-manager** REJEITA adicionar `Deferred` a `ToolExposure`. Argumenta que deferral e decisao de runtime, nao de registro estatico. Propoe `ToolSelectionPolicy` separado.

**test-runner** e **code-reviewer** tambem recomendam nao mudar o Tool trait.

**Resolucao:** ACEITO. `ToolExposure` fica inalterado. Deferral e controlado por `should_defer()` (ja existe no trait, sem mudanca) + `DeferralPolicy` na camada de runtime. Nao ha variante nova em ToolExposure.

### Conflito 2: Onde vive o scoring — theo-tooling vs theo-agent-runtime

**chief-architect** diz scoring em tool_bridge (theo-agent-runtime).
**retrieval-engineer** e **graphctx-expert** dizem scoring em theo-tooling.

**Resolucao:** Scoring basico (weighted token overlap) vive em `theo-tooling/registry` porque opera apenas sobre dados ja disponiveis no registry (id, hint, description). Nao precisa de nenhum import externo. Se futuramente precisar de scoring semantico, um `ToolRanker` trait em theo-domain permite injecao via DIP. Ambos os lados ficam satisfeitos.

### Conflito 3: Contextual deferral — mudar Tool trait vs DeferralPolicy

**validator** e **code-reviewer** dizem: nao mudar `Tool::should_defer(&self)` para aceitar contexto.
**arch-validator** propoe `visible_definitions_for_mode(mode)`.

**Resolucao:** `should_defer()` permanece `&self -> bool` (estatico). Contextual deferral via `DeferralPolicy` consultada em `registry_to_definitions()`. O registry permanece stateless. Fase 4 do rollout — nao bloqueia fases anteriores.

### Conflito 4: Schema completo por default vs opt-in

**validator** quer `include_schema: true` como opt-in.
**evolution-agent** e **research-agent** querem schema por default (Claude Code faz assim).

**Resolucao:** Schema retornado por default (alinhado com Claude Code e Anthropic best practices), mas limitado a top-3 resultados. Isso limita o pior caso a ~1500 tokens por tool_search call. Se token budget for problema, adicionar opt-out depois.

## Decisoes

1. **NAO adicionar variante Deferred a ToolExposure.** Deferral controlado por `should_defer()` existente + `DeferralPolicy` em runtime.

2. **Scoring em theo-tooling.** Weighted token overlap (~60 linhas), campo weights id:5x hint:1x description:0.5x, prefix bonus. Top-K=3 com schemas, top-K=5 sem schemas.

3. **tool_search retorna schemas completos por default**, limitado a top-3. Retorno muda de `Vec<(String, String)>` para `Vec<ToolSearchResult>` (novo struct).

4. **should_defer() NAO muda.** Contextual deferral via DeferralPolicy na camada runtime (Fase 4).

5. **Corrigir CLAUDE.md ANTES de implementar** — contagem de tools esta desatualizada.

6. **Escape hatch obrigatorio:** `tool_search({query: "*"})` lista todos os deferred tools.

7. **Fallback auto-search:** Se o agente tenta chamar uma tool desconhecida que existe como deferred, auto-trigger tool_search antes de falhar.

8. **Rollout em 5 fases sequenciais**, cada uma com RED-GREEN-REFACTOR e gate de qualidade.

## Plano de Implementacao — 5 Fases

### Fase 0: Fundacao (pre-requisito)
- Corrigir CLAUDE.md (contagem de tools)
- Adicionar teste `visible_registry_has_expected_count`
- CHANGELOG entry

### Fase 1: Enriquecer tool_search com schemas (puro ganho, zero risco)
**RED:**
```rust
#[test]
fn tool_search_returns_full_schema_for_matched_tools() {
    // Registra DeferredStub com schema nao-vazio
    // Chama search_deferred("wiki")
    // Asserta que resultado inclui ToolDefinition com schema completo
    // FALHA: search_deferred retorna (String, String), nao ToolSearchResult
}
```
**GREEN:** Criar `ToolSearchResult { id, hint, score, definition }`. Mudar `search_deferred()` para retornar `Vec<ToolSearchResult>`. Atualizar `handle_tool_search` para serializar schemas.
**REFACTOR:** Atualizar 4 testes existentes para novo tipo de retorno.
**VERIFY:** `cargo test -p theo-tooling && cargo test -p theo-agent-runtime`

### Fase 2: Scoring inteligente
**RED:**
```rust
#[test]
fn search_deferred_ranks_exact_id_match_above_hint_match() {
    // tool A: id="git_log", hint="show commit history"
    // tool B: id="git_status", hint="git log viewer"
    // query: "git_log"
    // EXPECT: A.score > B.score
}

#[test]
fn search_deferred_ranks_prefix_match_above_substring() {
    // query: "git" -> git_status, git_log, git_diff ranked above webfetch
}
```
**GREEN:** Implementar weighted token overlap: tokenize query, score cada tool por (id_matches * 5.0 + hint_matches * 1.0 + desc_matches * 0.5) / query_tokens. Prefix bonus +3.0.
**REFACTOR:** Extrair tokenizer para funcao utilitaria. Limpar sort.
**VERIFY:** `cargo test -p theo-tooling`

### Fase 3: Marcar tools como deferred (rollout incremental)
**Batch 1 (5 tools, menor risco):** `http_get`, `http_post`, `reflect`, `task_create`, `task_update`
**Batch 2 (6 tools, planning):** `plan_create`, `plan_summary`, `plan_advance_phase`, `plan_log`, `plan_update_task`, `plan_next_task`
**Batch 3 (4 tools, git):** `git_status`, `git_diff`, `git_log`, `git_commit`

**RED (por batch):**
```rust
#[test]
fn batch_N_tools_are_deferred_and_discoverable() {
    let registry = create_default_registry();
    for id in BATCH_N_IDS {
        let tool = registry.get(id).unwrap();
        assert!(tool.should_defer());
        assert!(tool.search_hint().is_some());
        let hits = registry.search_deferred(id);
        assert!(!hits.is_empty());
    }
}
```
**GREEN:** Override `should_defer() -> true` e `search_hint()` em cada tool.
**REFACTOR:** Atualizar tool_manifest.rs notes, CLAUDE.md contagens.
**VERIFY:** `cargo test` (workspace inteiro) + benchmark no vast.ai
**GATE:** Se pass rate do benchmark cair >2%, reverter batch e investigar.

### Fase 4: Contextual deferral + DeferralPolicy
**RED:**
```rust
#[test]
fn plan_tools_visible_in_plan_mode() {
    let policy = DeferralPolicy::for_mode(AgentMode::Plan);
    let registry = create_default_registry();
    let visible = registry.visible_definitions_with_policy(&policy);
    assert!(visible.iter().any(|d| d.id == "plan_create"));
}

#[test]
fn plan_tools_deferred_in_agent_mode() {
    let policy = DeferralPolicy::for_mode(AgentMode::Agent);
    let registry = create_default_registry();
    let visible = registry.visible_definitions_with_policy(&policy);
    assert!(!visible.iter().any(|d| d.id == "plan_create"));
}
```
**GREEN:** Criar `DeferralPolicy` em theo-domain. `visible_definitions_with_policy()` em theo-tooling. tool_bridge passa policy baseado no modo corrente.
**REFACTOR:** Limpar, documentar.
**VERIFY:** `cargo test` + `bash scripts/check-arch-contract.sh`

### Fase 5: Fallback auto-search + UI events
**RED:**
```rust
#[test]
fn unknown_tool_call_triggers_auto_search_if_deferred() {
    // Agent chama "git_status" que esta deferred
    // Runtime intercepta, roda tool_search("git_status")
    // Encontra e executa em vez de falhar
}
```
**GREEN:** Em execute_tool_call, se tool nao esta no registry visible E existe como deferred, auto-execute.
**UI:** Emitir evento `ToolDiscovered` no SSE stream. Renderizar tool_search como meta-step.
**VERIFY:** `cargo test -p theo-agent-runtime`

## Action Items

- [ ] **Paulo** — Fase 0: Corrigir CLAUDE.md contagem de tools — antes de qualquer codigo
- [ ] **Paulo** — Fase 1: RED tests para ToolSearchResult + schema — TDD primeiro
- [ ] **Paulo** — Fase 1: GREEN implementacao + REFACTOR — apos RED
- [ ] **Paulo** — Fase 2: RED tests para scoring — apos Fase 1 merge
- [ ] **Paulo** — Fase 2: GREEN weighted token overlap — apos RED
- [ ] **Paulo** — Fase 3: Batch 1 (5 tools) deferral + benchmark gate — apos Fase 2
- [ ] **Paulo** — Fase 3: Batch 2-3 se benchmark gate passa — incremental
- [ ] **Paulo** — Fase 4: DeferralPolicy + contextual deferral — apos Fase 3 estabilizar
- [ ] **Paulo** — Fase 5: Fallback auto-search + ToolDiscovered event — por ultimo

## Plano TDD

Para cada fase:
1. **RED**: Escrever teste que FALHA (prova que comportamento nao existe)
2. **GREEN**: Escrever MINIMO de codigo para teste passar
3. **REFACTOR**: Limpar mantendo testes verdes
4. **VERIFY**: `cargo test -p <crate>` + `cargo test` workspace
5. **GATE**: Benchmark no vast.ai para fases que mudam visibilidade de tools

## Metricas de Sucesso

| Metrica | Antes | Alvo | Como medir |
|---------|-------|------|------------|
| Tools no system prompt | 35 | 12-15 | Contagem em registry_to_definitions() |
| Tokens/turn em tools | ~6-8K | ~2-3K | Instrumentar token count |
| tool_search recall | N/A | >95% | Fixture: 15 deferred tools, queries representativas |
| Benchmark pass rate | 50% | >=48% | SWE-bench no vast.ai |
| Latencia por turn | baseline | <+200ms | tool_search e microsegundos para 45 tools |

## Veredito Final

**REVISED**: Aprovado com modificacoes. O core da proposta (schemas completos, scoring, deferral) e unanimemente apoiado. Revisoes: (1) NAO adicionar variante Deferred a ToolExposure — usar should_defer() existente + DeferralPolicy, (2) scoring self-contained em theo-tooling com weighted token overlap, (3) rollout em 5 fases sequenciais com gate de benchmark, (4) corrigir documentacao ANTES de implementar, (5) fallback auto-search para eliminar risco de degradacao silenciosa. Referencia externa: Anthropic mediu 85% token reduction e 49%->74% accuracy com tool search.
