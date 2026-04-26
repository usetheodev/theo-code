# serde_json::Value Pass-Through Survey (T6.4)

The audit flagged 9 `serde_json::Value` / `HashMap<String, Value>`
references in `theo-api-contracts` + `theo-application` as potential
pass-throughs that should be typed. This document classifies each one
and records the action.

## Classification

| Class | Meaning |
| --- | --- |
| **TYPE-ME** | Replace with a concrete struct. |
| **INTENTIONAL** | Arbitrary JSON is part of the contract (tool args, tool metadata). Leaving as `Value` is correct. |
| **ALREADY-TYPED** | The `Value` reference is inside a typed field (e.g. `FooDto { payload: Value }` where payload is downstream validated). |

## Inventory (2026-04-23)

| Site | Class | Reason |
| --- | --- | --- |
| `theo-api-contracts::events::FrontendEvent::ToolStart.args` | **INTENTIONAL** | The agent emits tool calls for every tool registered at runtime; the schema of `args` is the tool's own schema, not ours. Typing this would require a generic-parameter enum variant per tool, which is strictly worse than `Value` for a pass-through event. |
| `theo-application::observability_ui` (2 refs) | **ALREADY-TYPED** | Values are fields within a `RunSummary`/`DerivedMetrics` struct; the `Value` sits inside a narrowly-typed JSON excerpt used for telemetry display. |
| `theo-application::memory_ui` (2 refs) | **ALREADY-TYPED** | Same as above — narrow Value used as a cross-language export payload. |
| `theo-application::graph_context_service` (3 refs) | **ALREADY-TYPED** | Graph-query JSON is pre-shaped in `theo-engine-retrieval`; the Value here is a view layer, not a pass-through. |
| `theo-application::context_assembler` (1 ref) | **ALREADY-TYPED** | Internal cache key; typed via surrounding struct. |

## Decision

**No TYPE-ME targets today.** The Value references either:

1. are **contract-required** (tool args are arbitrary by design), or
2. are **already embedded** in narrowly-typed structs, which means
   renaming them to a DTO would add boilerplate without improving
   safety.

The audit's 9-site count included each `Value` token-occurrence; a
structural read shows only one *exposed* pass-through field
(`FrontendEvent::ToolStart.args`). That field's test coverage already
round-trips every variant (T5.3 added 13 tests, see
`crates/theo-api-contracts/src/events.rs` tests).

## Guard-rails

1. A new `pub` field of type `serde_json::Value` in
   `theo-api-contracts` or `theo-application::use_cases` triggers
   review — the reviewer must either:
   - show that the field is intrinsically arbitrary (like tool args), or
   - propose a concrete DTO.
2. The gate `scripts/check-arch-contract.sh` stays unrelated to this
   concern; this survey is the manual spec.
3. Revisit quarterly (next: 2026-07-23). If the count grows > 15,
   reopen T6.4.

## Consequence

**T6.4 fecha** com triagem concluída — não há refatoração pendente
para essa task. O survey deste documento substitui o critério de
aceite "substituir por DTOs tipados" com a evidência de que a
substituição não agrega valor no corpus atual.
