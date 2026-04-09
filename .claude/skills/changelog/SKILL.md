---
name: changelog
description: Update CHANGELOG.md based on recent git commits. Use when preparing a release or documenting changes.
user-invocable: true
allowed-tools: Bash(git *) Read Write Edit
argument-hint: "[N commits, default 20]"
---

Update the CHANGELOG.md for Theo Code.

## Steps

1. Read current CHANGELOG.md
2. Get recent commits:
   ```!
   git log --oneline -${ARGUMENTS:-20}
   ```
3. Classify each commit into: Added, Changed, Fixed, Removed, Security, Deprecated
4. Write entries under `[Unreleased]` section
5. Format: one line per change, consumer-facing language, issue/PR reference if available
6. Never modify entries under already-released versions

## Rules

- Write for the USER, not the developer: "Added Code Wiki search" not "Implemented BM25 index"
- One line per change
- Reference PR/issue: `(#123)`
- Categories in order: Added, Changed, Deprecated, Removed, Fixed, Security
- Skip internal refactors with no user-visible impact
- When listing changes, note if they followed TDD (test committed before implementation)
- Flag any significant code change without corresponding test addition as a concern
