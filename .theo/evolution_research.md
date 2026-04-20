# Evolution Research: Anthropic Tool Calling 2.0

**Prompt:** Migrate tools to Anthropic's "Tool Calling 2.0" mental model — programmatic tool calling, dynamic filtering, deferred loading, input examples.
**Source:** https://www.youtube.com/watch?v=3wglqgskzjQ + Anthropic API docs
**Date:** 2026-04-20
**Baseline:** 588d0b6 (score 72.3, 2556 tests)

## Status of the 4 new features

| Feature | Theo state entering this cycle | Delta needed |
|---|---|---|
| 1. Programmatic Tool Calling | None | Add `batch_execute` meta-tool as a Rust-safe intermediate (not a full code sandbox) |
| 2. Dynamic Filtering (web fetch) | webfetch returns raw response body | Add HTML reducer + llm_suffix |
| 3. Tool Search (deferred loading) | **DONE** last cycle: `should_defer`, `visible_definitions`, `tool_search` meta-tool, 4 tests | Nothing |
| 4. Tool Use Examples | `ToolSchema` has only name/type/description/required | Add `input_examples: Vec<Value>` and emit in JSON Schema |

## Patterns to Apply (refs: opendev, fff-mcp, Anthropic docs)

### P1 — input_examples (Anthropic "Tool Use Examples")
- Field on `ToolSchema` (not `ToolParam`) because Anthropic emits `examples: [...]` at schema top level
- Each example is a full valid invocation JSON showing how to fill correlated fields
- Populate on `edit` (filePath+oldString+newString+replaceAll combinations), `apply_patch` (V4A format), `read` (offset/limit), `grep` (pattern+path+include), `bash` (command+description)
- Accuracy: Anthropic reports 72%→90% on complex schemas (Tool Use Examples section of the transcript)

### P2 — Dynamic HTML filtering (Anthropic "Dynamic Filtering")
- Reducer removes `<script>`, `<style>`, `<nav>`, `<header>`, `<footer>`, inline event handlers, and base64 image payloads
- Operates deterministically, so no code-exec sandbox is needed (a simpler approach than Anthropic's implementation, but matches the "strip noise before context" goal)
- Surface "removed N chars of HTML noise" in the `llm_suffix` so the model knows it got a digest

### P3 — batch_execute meta-tool (minimum-viable programmatic tool calling)
- Accepts `{calls: [{tool, args, bind_as?}, ...]}` with optional variable binding between steps
- Serial execution with early-exit on first failure; failure detail is surfaced in the combined result
- Meta-tool dispatched in `tool_bridge::execute_tool_call` alongside `tool_search`
- Does NOT include a JS/Python interpreter — that is a multi-cycle effort and outside this pass
- Still delivers the "for-loop over URLs" pattern Anthropic highlights as 30-50% token win

## Gap Analysis: ToolSchema today

```rust
pub struct ToolSchema { pub params: Vec<ToolParam> }

pub struct ToolParam {
    pub name: String,
    pub param_type: String,
    pub description: String,
    pub required: bool,
}
```

Missing for Anthropic parity:
- No `examples` top-level array
- No per-param `enum` / `minLength` / `maxItems` (noted last cycle, still deferred — not in the Tool Calling 2.0 scope)

## Top 3 Changes ordered by ROI

| # | Change | Est. LOC | Risk | Impact |
|---|---|---|---|---|
| P1 | `input_examples` on ToolSchema + 3-5 tool adoptions | ~150 | Low (additive, serde default) | High — directly raises param-accuracy baseline |
| P2 | Webfetch HTML reducer | ~120 | Low | Medium — webfetch is not the hot path today but win is real |
| P3 | `batch_execute` meta-tool | ~180 | Medium (touches dispatch) | High — unlocks parallel workflows |

## Execution order

P1 → P2 → P3. Each cycle: RED test → GREEN impl → refactor → hygiene → evaluate.
