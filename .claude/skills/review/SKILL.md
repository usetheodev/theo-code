---
name: review
description: Review recent code changes for quality and architectural compliance. Use before commits or PRs.
user-invocable: true
context: fork
agent: code-reviewer
argument-hint: "[staged|branch|file]"
---

Review code changes in the Theo Code workspace.

## What to review

- `staged` or no args: review `git diff --cached`
- `branch`: review all commits on current branch vs main
- File path: review specific file

## Provide the reviewer with

The git diff output:

```!
git diff --cached --stat
git diff --cached
```

Current branch context:

```!
git log --oneline -5
```

## TDD Compliance Check

The reviewer MUST verify:
1. Every new/changed function has a corresponding test
2. Tests were written BEFORE implementation (check commit order if visible)
3. All tests pass: `cargo test -p <affected-crate>`
4. No code change without test change = automatic flag

**Code without tests = REJECT. No exceptions.**
