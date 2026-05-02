---
name: kanban
description: Transform a plan from docs/plans/ into a Kanban board with cards per task. Tracks status, dependencies, and links back to the source plan. Use when user wants to visualize or track plan execution.
user-invocable: true
argument-hint: "[plan-slug|plan-file-path] [--update|--status|--move T1.2 doing|--done T1.2]"
---

Transforms an implementation plan (`docs/plans/*.md`) into a Kanban board saved to `docs/kanban/`. Each task in the plan becomes a card with status tracking, dependency awareness, and direct references back to the source plan.

## Arguments

| Argument | Behavior |
|---|---|
| `{plan-slug}` | Create/update board from `docs/plans/{slug}-plan.md` |
| `{path}` | Create/update board from explicit file path |
| `--status` | Show current board status summary (no file arg needed if only one board exists) |
| `--move T{N}.{M} {column}` | Move a card to a new column |
| `--done T{N}.{M}` | Mark a card as done (shortcut for `--move T{N}.{M} done`) |
| `--update` | Re-sync board from source plan (adds new tasks, preserves statuses) |

## Columns

Every board has exactly 5 columns:

| Column | Meaning | Entry criteria |
|---|---|---|
| **backlog** | Not started, dependencies may not be met | Default for all new cards |
| **ready** | All dependencies met, can start immediately | All blocking cards are `done` |
| **doing** | Actively being worked on | Moved manually or by `/ralph-loop` |
| **review** | Implementation complete, awaiting verification | Code written, tests passing, needs review |
| **done** | Verified and complete | All acceptance criteria met, DoD satisfied |

## Process

### Step 1 — Parse the Plan

Read the source plan and extract:

1. **Plan metadata:** title, version, date, objective
2. **Phases:** phase number, title, objective
3. **Tasks:** for each `### T{N}.{M}` section, extract:
   - `id`: `T{N}.{M}` (e.g., `T1.2`)
   - `title`: task title after the `—`
   - `phase`: parent phase number
   - `files`: from "Files to edit" section
   - `tests`: count of RED tests from TDD section
   - `acceptance_criteria`: count from acceptance criteria section
   - `dependencies`: inferred from dependency graph + phase ordering
   - `estimated_complexity`: S/M/L based on number of files + tests + criteria

4. **Dependency graph:** parse the ASCII dependency diagram to determine:
   - Which phases block which
   - Which phases can parallelize
   - Task-level dependencies within phases (T{N}.1 before T{N}.2 unless stated otherwise)

### Step 2 — Generate Cards

For each task, create a card:

```markdown
### T{N}.{M} — {Title}

| Field | Value |
|---|---|
| **Phase** | {N}: {phase_title} |
| **Status** | {column} |
| **Complexity** | {S/M/L} |
| **Dependencies** | {list of T{X}.{Y} IDs or "none"} |
| **Blocks** | {list of T{X}.{Y} IDs that depend on this} |
| **Files** | {count} files |
| **Tests** | {count} RED tests |
| **Acceptance Criteria** | {count} criteria |
| **Plan ref** | [T{N}.{M}]({relative_path_to_plan}#t{n}{m}--{slug}) |

**Objective:** {one-line from task objective}

**Key deliverables:**
- {file1} — {what changes}
- {file2} — {what changes}
```

### Step 3 — Compute Ready State

After generating all cards, automatically move cards from `backlog` to `ready` when:
- ALL dependency cards are in `done` column
- OR the card has no dependencies (Phase 0 tasks, independent tasks)

### Step 4 — Output

Save the board to `docs/kanban/{plan-slug}-board.md`.

## Board Template

```markdown
# Kanban — {Plan Title}

**Source:** [{plan-file}](../plans/{plan-file})
**Created:** {date}
**Last updated:** {date}

## Progress

```
[===========·················] 38% (11/29 done)
```

| Column | Count | Cards |
|---|---|---|
| backlog | {n} | {T-ids} |
| ready | {n} | {T-ids} |
| doing | {n} | {T-ids} |
| review | {n} | {T-ids} |
| done | {n} | {T-ids} |

## Phase Summary

| Phase | Title | Total | Done | Progress |
|---|---|---|---|---|
| 0 | {title} | {n} | {n} | {pct}% |
| 1 | {title} | {n} | {n} | {pct}% |
| ... | | | | |

## Dependency Graph (Live)

```
T0.1 [done] ──▶ T1.1 [doing] ──▶ T2.1 [ready]
                    │                    │
                    ▼                    ▼
               T1.2 [doing]        T2.2 [backlog]
                    │
                    ▼
               T4.1 [backlog]
```

Status annotations: `[done]`, `[review]`, `[doing]`, `[ready]`, `[backlog]`

---

## Backlog

{cards with status=backlog, grouped by phase}

## Ready

{cards with status=ready, grouped by phase}

## Doing

{cards with status=doing, grouped by phase}

## Review

{cards with status=review, grouped by phase}

## Done

{cards with status=done, grouped by phase, most recent first}

---

## History

| Date | Card | From | To | Note |
|---|---|---|---|---|
| {date} | T1.2 | doing | review | tests passing |
| {date} | T1.1 | ready | doing | — |
| {date} | — | — | — | Board created from {plan-file} |
```

## Operations

### Create (`/kanban benchmark-sota-metrics`)

1. Parse `docs/plans/benchmark-sota-metrics-plan.md`
2. Generate all cards in `backlog`
3. Compute `ready` state
4. Save board to `docs/kanban/benchmark-sota-metrics-board.md`
5. Print summary to conversation

### Status (`/kanban --status`)

Read the most recent board (or specified board) and print:
- Progress bar
- Column counts
- Cards in `doing` (what's active now)
- Cards in `ready` (what can start next)
- Blocked cards (dependencies not met)

### Move (`/kanban --move T1.2 doing`)

1. Read board
2. Validate the move is legal:
   - `backlog → ready`: all dependencies must be `done`
   - `ready → doing`: no constraint
   - `doing → review`: no constraint
   - `review → done`: no constraint
   - `any → backlog`: allowed (revert/re-scope)
   - Skip columns (e.g., `ready → done`): allowed with warning
3. Update card status
4. Re-compute `ready` state for all cards (cascading unblocks)
5. Add history entry
6. Save board
7. Print what changed

### Done (`/kanban --done T1.2`)

Shortcut for `--move T1.2 done`. Also:
- Re-compute ready state (may unblock dependent cards)
- Print newly unblocked cards

### Update (`/kanban --update benchmark-sota-metrics`)

Re-read the source plan and sync:
- New tasks in plan → add as `backlog` cards
- Removed tasks → mark as `cancelled` in history (don't delete)
- Changed task content → update card fields, preserve status
- Print diff of what changed

## Complexity Estimation

Cards are sized automatically based on plan content:

| Size | Criteria |
|---|---|
| **S** (small) | <= 2 files, <= 3 tests, <= 3 acceptance criteria |
| **M** (medium) | <= 5 files, <= 8 tests, <= 6 acceptance criteria |
| **L** (large) | > 5 files OR > 8 tests OR > 6 acceptance criteria |

## Integration with Other Skills

- `/ralph-loop` can read the kanban board to pick the next `ready` card
- `/review` can reference card IDs in findings
- `/to-plan` generates plans that this skill consumes
- `/show-domain` results can inform which cards to prioritize (worst health → fix first)

## Notes

- Boards are plain Markdown — viewable on GitHub, editable by hand
- History is append-only (never delete entries)
- One board per plan (1:1 mapping)
- Cards reference the source plan via relative links — if the plan updates, cards stay linked
- The dependency graph in the board is LIVE — reflects current status, not static from the plan
