---
active: true
prompt: "Anthropic Tool Calling 2.0 — programmatic tool calling, dynamic filtering, deferred loading, input examples"
current_phase: 1
phase_name: "research"
phase_iteration: 1
global_iteration: 1
max_global_iterations: 15
completion_promise: "optimize"
started_at: "2026-04-20T14:00:00Z"
theo_code_dir: "/home/paulo/theo-code"
output_dir: "/home/paulo/theo-code/evolution-output"
plugin_root: "/home/paulo/autoloop/theocode-loop"
baseline_score: "72.300"
baseline_l1: "94.1"
baseline_l2: "50.5"
current_score: "72.300"
sota_average: "0.0"
iteration_cycle: 0
---

# Theocode Evolution Loop — Tool Calling 2.0

## Target Features (from Anthropic "Tool Calling 2.0")

1. **Programmatic Tool Calling** — LLM emits code that orchestrates multiple tool calls in a sandbox
2. **Dynamic Filtering (Web Fetch)** — filter HTML through code-exec layer before insertion into context
3. **Tool Search (Deferred Loading)** — already landed last cycle (should_defer + visible_definitions + tool_search)
4. **Tool Use Examples** — input_examples on ToolSchema to coach complex parameter usage

## Pre-existing state (from prior evolution cycle)

- `Tool::should_defer()` + `Tool::search_hint()` on trait — DONE
- `ToolRegistry::visible_definitions()` + `search_deferred()` — DONE
- `tool_search` meta-tool in tool_bridge — DONE
- `Tool::truncation_rule()` + sanitizer (P2 last cycle) — DONE
- `Tool::format_validation_error()` (P4 last cycle) — DONE
- `ToolOutput::llm_suffix` (P1 last cycle) — DONE
- Decision-tree descriptions on top-5 tools (P3 last cycle) — DONE

## Delta for this cycle

**P1 — input_examples on ToolParam/ToolSchema (Anthropic Tool Use Examples)**
  - Add `input_examples: Vec<serde_json::Value>` field to `ToolSchema`
  - Emit into JSON Schema output as top-level `examples` key (OpenAI/Anthropic compatible)
  - Populate examples on top-5 complex tools (edit, apply_patch, read, grep, bash)
  - Target: 72% → 90% param accuracy on complex tools

**P2 — Dynamic filtering wired into webfetch (Anthropic Dynamic Filtering)**
  - After reqwest returns HTML, run a deterministic reducer that strips nav/header/footer/script/style
  - Emit filtered text + an llm_suffix telling the model which sections were dropped
  - Target: ~24% token reduction on webfetch results

**P3 — Programmatic tool-calling scaffolding (smaller than full Code Mode)**
  - Add a `batch_execute` meta-tool that accepts an ordered list of tool calls with variable bindings
  - Enables "for loop over URLs" patterns without a full code sandbox
  - Not a full code interpreter (that's too large for one cycle) — just deterministic batch semantics
  - Defer the Python/JS interpreter path as a future iteration

## Guardrails

- Hygiene floor: `cargo check --workspace --tests` must stay at 0 warnings, 0 errors
- Every behavior change starts with a failing test
- Pre-commit hook must pass without --no-verify
- Max 200 lines per commit; decompose larger changes
- `theo-domain` stays dependency-free

## Done Definition

- `cargo test --workspace` passes (currently 2556 tests)
- At least 3 tools declare non-empty `input_examples`
- Webfetch filters HTML deterministically and cites what was dropped
- `batch_execute` meta-tool exists with integration test covering 2+ sequential calls
