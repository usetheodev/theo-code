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
