# Benchmark Plan — Status Update (2026-04-15)

## Phase 0: Smoke Suite ✅ COMPLETE
- 15 scenarios, 14-15/15 pass rate (93-100%)
- avg 7.3 iterations, 290k tokens
- Categories: read, search, fix-bug(5), add-feature, refactor, explore(2), multi-file, plan-mode

## Phase 1: Terminal-Bench ⏳ BLOCKED (Docker)
- Harbor adapter written (tbench/agent.py + setup.sh)
- Needs Docker for container-based execution
- Ready to run: `harbor run -d terminal-bench/terminal-bench-2 -a theo-code`

## Phase 2: SWE-bench Lite 🔄 IN PROGRESS
- 26/26 patches generated (100% generation rate)
- ~38% estimated resolve rate (via gold-patch line overlap)
- By repo: Requests 83%, Django 40%, Flask 33%, Seaborn 25%, Pytest 20%, Pylint 0%
- **Blocker**: Rate limited on Codex OAuth — waiting for reset
- **Next**: Run 50+ more instances, validate with SWE-bench Docker grader

## Phase 3: Theo-Bench 📋 PLANNED
- Not started — waiting for Phase 2 baseline

## Evolve Loop ✅ OPERATIONAL
- Karpathy ratchet pattern: mutate → eval → accept/revert
- 5 mutations tested, 1 survived (think_skip)
- 3 pending hypotheses (edit_then_done, grep_before_read, direct_edit_simple)
- Tool hints added to edit/write tools

## Optimizations Applied
| Change | Impact | Status |
|---|---|---|
| Lean headless prompt | -27% tokens | Accepted |
| think_skip | -0.4 avg iterations | Accepted (evolve) |
| RPI sensor removal | -2 iter/edit | Accepted |
| FAIL_TO_PASS in SWE prompt | Better targeting | Accepted |
| Edit/write done hints | TBD | Pending validation |
| verify_done_combo | -2.4 score | Rejected (evolve) |
| batch_usage | -14.5 score | Rejected (evolve) |
| codebase_context_skip | -2.6 score | Rejected (evolve) |
| done_without_test | -1.5 score | Rejected (evolve) |
| SWE "be FAST" | -33% resolve | Rejected (manual) |

## Estimated Position
- SWE-bench Lite: ~4th-6th (behind Claude Code ~55%, Codex ~50%, Aider ~45%)
- Terminal-Bench: unknown (blocked on Docker)
- To reach top 3: need ~50% resolve rate (+12 pp from current 38%)
