# Evolution Research: Tool Design Best Practices

**Prompt:** Revisar tools baseados em Anthropic best practices
**Source:** https://www.anthropic.com/engineering/writing-tools-for-agents (Anthropic Engineering, 2025)
**Date:** 2026-04-19

## 12 Anthropic Principles (canonical checklist)

1. **Strategic tool selection** — build tools that match agent workflows, not API endpoints
2. **Consolidate multi-step operations** — `schedule_event` > `list_users + list_events + create_event`
3. **Distinct purposes** — overlapping tools confuse agents
4. **Clear namespacing** — consistent prefixes (`asana_projects_*`, `asana_users_*`)
5. **Unambiguous parameter names** — `user_id` not `user`
6. **Response format control** — optional enum `response_format: "concise" | "detailed"`
7. **Semantic identifiers** — prefer names over UUIDs
8. **Actionable error messages** — `"Expected user_id as number; you provided 'jane'"`
9. **Pagination + filtering defaults** — sensible limits to control token bloat
10. **Truncate with guidance** — tell the agent how to refine the query
11. **Descriptions as onboarding a colleague** — explicit context, terminology, relationships
12. **Strict schemas + concrete examples** — minimize context overhead, enable targeted follow-ups

## Reference Patterns (Rust-grounded)

### opendev-tools-core (primary Rust reference)

- **`llm_suffix` on ToolResult** (traits.rs:128-130, 173-176)
  `ToolResult` carries `llm_suffix: Option<String>` via `.with_llm_suffix(...)`. Suffix appended to model-facing output but hidden from UI. Encodes retry hints: _"Try `grep` with a narrower pattern"_.
  -> **Maps to Principle 8** (actionable errors), **10** (truncation guidance).

- **`truncation_rule()` on BaseTool trait** (traits.rs:534-542, sanitizer.rs:27-53)
  Each tool returns `Option<TruncationRule { max_chars, strategy: Head|Tail|HeadTail }>`. Sanitizer pipeline enforces per-tool. Bash -> `Tail(8000)`, Read -> `Head(15000)`. Appends: _"[truncated: N of M chars; use read_file with offset to see more]"_.
  -> **Maps to Principle 10** (truncation + guidance), **12** (strict output limits).

- **`format_validation_error()` on BaseTool trait** (traits.rs:444-447)
  Defaulted trait method. Registry calls it when schema validation fails. Tools override to emit: _"Missing 'query'. Provide the regex, e.g. grep(query='fn main')"_.
  -> **Maps to Principle 8** (actionable errors), **5** (unambiguous params).

- **`should_defer()` + `search_hint()`** (traits.rs:547-575)
  Tools marked `should_defer() -> true` omit their schemas from the system prompt; agents discover via a lazy `ToolSearch` tool using `search_hint()` for keyword matching.
  -> **Maps to Principle 12** (minimize context overhead).

- **camelCase-to-snake_case normalizer** (normalizer.rs:26-78)
  Static match block maps ~50 common LLM mistakes (`filePath` -> `file_path`). Applied pre-validation so the schema stays strict. Declarative `FILE_PATH_PARAMS` slice tags which params get path expansion.
  -> **Maps to Principle 5** (unambiguous params without breaking strict-schema contract).

### fff-mcp (MCP server in Rust)

- **Decision-tree descriptions with NOT-usage rules** (server.rs:388-502)
  `find_files` description: _"Use grep instead for searching code content. IMPORTANT: avoid X because Y."_ Teaches model what NOT to call, not just what the tool does.
  -> **Maps to Principle 11** (onboarding-style descriptions).

- **Cursor-based pagination with opaque IDs** (server.rs:161-168, 398-402)
  `FindFilesParams.cursor: Option<String>` backed by `CursorStore`. Safer than numeric offsets (model cannot fabricate valid cursors).
  -> **Maps to Principle 9** (pagination defaults).

## Gap Analysis: theo-tooling current state

File-by-file audit (`crates/theo-domain/src/tool.rs`, `crates/theo-tooling/src/`):

| Gap | Principle | Current state |
|---|---|---|
| `ToolOutput` has no `llm_suffix` field | 8 | `ToolOutput { title, output, metadata, attachments }` (tool.rs:11-18) |
| `Tool` trait has no `truncation_rule()` | 10, 12 | Tools handle truncation ad-hoc in `execute()` |
| `Tool` trait has no `format_validation_error()` | 8 | Generic `ToolError::InvalidArgs(String)` via `require_string` |
| `Tool` trait has no `should_defer()` / `search_hint()` | 12 | All 21+ tool schemas always in prompt |
| `ToolParam` lacks enum/minLength/maxItems constraints | 12 | Flat `{name, param_type: String, description, required}` — `to_json_schema()` emits minimal schema |
| One-liner descriptions (e.g., `"Read a file or directory"`) | 11 | No decision tree, no NOT-usage, no examples |
| No `response_format` parameter on any tool | 6 | All tools emit verbose default output |
| `prepare_arguments` is identity by default | 5 | camelCase tolerance is per-tool, duplicated |

## Top 5 Highest-Impact Changes (ordered by ROI)

### [P1] Add `llm_suffix` to `ToolOutput` — enable per-tool error coaching
- **Files:** `theo-domain/src/tool.rs` (+field), `theo-agent-runtime` serializer, 4-5 high-error tools (bash/edit/write/grep/read)
- **LOC:** ~80
- **Risk:** Low (additive, `#[serde(skip_serializing_if = "Option::is_none")]`)
- **Principles:** 8, 10
- **Test plan:** Unit test: `ToolOutput::with_llm_suffix(s)` roundtrips; integration test: edit tool failure produces suffix visible in model channel

### [P2] Add `truncation_rule()` + sanitizer — bound tool output token cost
- **Files:** `theo-domain/src/tool.rs` (+trait method, +2 types), `theo-tooling/src/sanitizer.rs` (new, ~80 LOC), `theo-agent-runtime` execution loop
- **LOC:** ~150
- **Risk:** Low-medium — touches execution pipeline
- **Principles:** 10, 12
- **Test plan:** `TruncationStrategy::{Head,Tail,HeadTail}` each have unit test; bash output > 8k bytes gets tail-truncated with guidance suffix

### [P3] Decision-tree descriptions + NOT-usage rules on top-5 tools
- **Files:** `theo-tooling/src/{read,grep,glob,bash,edit}/mod.rs` — `description()` methods
- **LOC:** ~200 (prompt content only)
- **Risk:** Zero code risk — prompt-quality only
- **Principles:** 3, 11
- **Test plan:** Each description includes at least one "Use X instead when Y" and a concrete example; snapshot test against regression

### [P4] Add `format_validation_error()` on `Tool` trait + structured `ValidationError`
- **Files:** `theo-domain/src/tool.rs` (+struct, +defaulted method), `theo-tooling/src/registry` (invoke on validation failure), 5-6 most-called tools override
- **LOC:** ~100
- **Risk:** Low (additive)
- **Principles:** 5, 8
- **Test plan:** Tool with missing required field emits message with concrete example; test covers override chain

### [P5] Add `should_defer()` + `search_hint()` — lazy schemas for rare tools
- **Files:** `theo-domain/src/tool.rs` (+2 defaulted methods), `theo-application` registry init, tools: wiki, skill, lsp, codesearch override `should_defer() -> true`
- **LOC:** ~60
- **Risk:** Medium — requires tool-search discovery path to exist/be added
- **Principles:** 12
- **Test plan:** Deferred tools absent from `registry.definitions()` by default; `registry.search(hint) -> matches` covers each hint

## Execution Order

P1 -> P3 -> P2 -> P4 -> P5 (each cycle: RED test -> GREEN min impl -> REFACTOR -> hygiene).
P3 can land alongside P1 since it's pure text.
