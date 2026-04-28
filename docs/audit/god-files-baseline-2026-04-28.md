# God-Files Baseline — 2026-04-28

Snapshot of `.claude/rules/size-allowlist.txt` per-entry LOC at the start of
the god-files-2026-07-23 plan. Frozen reference for measuring progress.

Reproduce: `bash scripts/check-allowlist-progress.sh --baseline`

## Per-entry table

| # | Path | Ceiling | Current LOC | Headroom |
|---:|---|---:|---:|---:|
| 1 | `crates/theo-agent-runtime/src/run_engine/mod.rs` | 2600 | 1668 | 932 |
| 2 | `crates/theo-engine-retrieval/src/wiki/generator.rs` | 2100 | 2018 | 82 |
| 3 | `crates/theo-engine-parser/src/extractors/language_behavior.rs` | 1800 | 1757 | 43 |
| 4 | `crates/theo-application/src/use_cases/graph_context_service.rs` | 1980 | 1959 | 21 |
| 5 | `crates/theo-engine-parser/src/types.rs` | 1700 | 1665 | 35 |
| 6 | `crates/theo-engine-retrieval/src/assembly.rs` | 1700 | 1612 | 88 |
| 7 | `crates/theo-engine-graph/src/cluster.rs` | 1650 | 1585 | 65 |
| 8 | `crates/theo-engine-retrieval/src/file_retriever.rs` | 1500 | 1418 | 82 |
| 9 | `crates/theo-engine-parser/src/extractors/symbols.rs` | 1450 | 1393 | 57 |
| 10 | `crates/theo-engine-parser/src/symbol_table.rs` | 1400 | 1349 | 51 |
| 11 | `crates/theo-domain/src/episode.rs` | 1400 | 1324 | 76 |
| 12 | `apps/theo-cli/src/tui/app.rs` | 1300 | 1270 | 30 |
| 13 | `crates/theo-engine-retrieval/src/search.rs` | 1300 | 1257 | 43 |
| 14 | `crates/theo-engine-parser/src/import_resolver.rs` | 1300 | 1243 | 57 |
| 15 | `crates/theo-agent-runtime/src/pilot/mod.rs` | 1450 | 1414 | 36 |
| 16 | `crates/theo-engine-parser/src/extractors/data_models.rs` | 1200 | 1185 | 15 |
| 17 | `crates/theo-engine-parser/src/extractors/csharp.rs` | 1200 | 1158 | 42 |
| 18 | `crates/theo-engine-retrieval/src/wiki/runtime.rs` | 1100 | 1087 | 13 |
| 19 | `crates/theo-infra-llm/src/providers/anthropic.rs` | 1100 | 1074 | 26 |
| 20 | `crates/theo-domain/src/tool.rs` | 1100 | 1090 | 10 |
| 21 | `crates/theo-agent-runtime/src/tool_bridge/mod.rs` | 1100 | 923 | 177 |
| 22 | `apps/theo-cli/src/main.rs` | 1500 | 1480 | 20 |
| 23 | `crates/theo-application/src/use_cases/pipeline.rs` | 1000 | 989 | 11 |
| 24 | `crates/theo-engine-parser/src/extractors/python.rs` | 1000 | 970 | 30 |
| 25 | `crates/theo-infra-llm/src/providers/openai.rs` | 1000 | 964 | 36 |
| 26 | `crates/theo-engine-parser/src/extractors/php.rs` | 1000 | 944 | 56 |
| 27 | `crates/theo-application/src/use_cases/context_assembler.rs` | 950 | 949 | 1 |
| 28 | `crates/theo-engine-retrieval/src/tantivy_search.rs` | 950 | 937 | 13 |
| 29 | `crates/theo-tooling/src/read/mod.rs` | 970 | 955 | 15 |
| 30 | `crates/theo-engine-parser/src/extractors/typescript.rs` | 950 | 921 | 29 |
| 31 | `crates/theo-agent-runtime/src/session_tree/mod.rs` | 950 | 753 | 197 |
| 32 | `crates/theo-engine-graph/src/bridge.rs` | 900 | 884 | 16 |
| 33 | `crates/theo-engine-parser/src/tree_sitter.rs` | 900 | 876 | 24 |
| 34 | `crates/theo-engine-graph/src/git.rs` | 900 | 861 | 39 |
| 35 | `crates/theo-engine-retrieval/src/wiki/model.rs` | 900 | 856 | 44 |
| 36 | `crates/theo-tooling/src/apply_patch/mod.rs` | 920 | 908 | 12 |
| 37 | `crates/theo-infra-memory/src/security.rs` | 850 | 816 | 34 |
| 38 | `crates/theo-tooling/src/dap/tool.rs` | 2500 | 1783 | 717 |
| 39 | `crates/theo-tooling/src/dap/tool_tests.rs` | 1300 | 1259 | 41 |
| 40 | `crates/theo-tooling/src/plan/mod.rs` | 2400 | 2356 | 44 |
| 41 | `crates/theo-domain/src/plan.rs` | 1900 | 1867 | 33 |
| 42 | `crates/theo-tooling/src/lsp/tool.rs` | 1000 | 974 | 26 |
| 43 | `crates/theo-tooling/src/lsp/tool_tests.rs` | 850 | 804 | 46 |
| 44 | `crates/theo-tooling/src/browser/tool.rs` | 900 | 868 | 32 |
| 45 | `crates/theo-tooling/src/registry/mod.rs` | 1550 | 1380 | 170 |
| 46 | `crates/theo-agent-runtime/src/subagent/mod.rs` | 1500 | 1492 | 8 |
| 47 | `crates/theo-agent-runtime/src/subagent/resume.rs` | 1200 | 1144 | 56 |
| 48 | `crates/theo-agent-runtime/src/compaction_stages.rs` | 960 | 934 | 26 |
| 49 | `crates/theo-agent-runtime/src/lifecycle_hooks.rs` | 850 | 837 | 13 |
| 50 | `crates/theo-agent-runtime/src/config/mod.rs` | 920 | 905 | 15 |
| 51 | `crates/theo-domain/src/event.rs` | 820 | 803 | 17 |
| 52 | `crates/theo-infra-mcp/src/discovery.rs` | 940 | 933 | 7 |
| 53 | `apps/theo-ui/src/components/ui/sidebar.tsx` | 800 | 771 | 29 |

## Rollups

- **Total entries:** 53
- **Total LOC above default 800-LOC ceiling:** sum of (current - 800) per entry; see baseline script
- **Sunset:** 2026-07-23 (every entry)
- **Default crate ceiling:** 800 LOC
- **Default UI ceiling:** 400 LOC

## Distribution by ceiling tier

| Tier | Count |
|---|---:|
| 2000+ LOC | 4 |
| 1500-1999 LOC | 10 |
| 1000-1499 LOC | 20 |
| 800-999 LOC | 19 |

## Distribution by crate

- `crates/theo-engine-parser` — 11 entries
- `crates/theo-tooling` — 9 entries
- `crates/theo-agent-runtime` — 9 entries
- `crates/theo-engine-retrieval` — 7 entries
- `crates/theo-domain` — 4 entries
- `crates/theo-engine-graph` — 3 entries
- `crates/theo-application` — 3 entries
- `crates/theo-infra-llm` — 2 entries
- `apps/theo-cli` — 2 entries
- `crates/theo-infra-memory` — 1 entries
- `crates/theo-infra-mcp` — 1 entries
- `apps/theo-ui` — 1 entries
