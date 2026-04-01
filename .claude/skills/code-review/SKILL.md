---
name: code-review
description: Use when reviewing pull requests, examining code changes, or providing feedback on code quality. Covers Rust safety, React patterns, architecture boundaries, security, and test coverage for the Theo Code workspace.
---

# Code Review

Review code changes for the Theo Code workspace. Applies to Rust crates, React/TypeScript frontend, and Tauri bridge.

## Review Checklist

### 1. Runtime Safety (Rust)
- `unwrap()` in production code (outside `#[cfg(test)]`)
- Missing error propagation (`?` vs silent ignore)
- Potential panics: `unreachable!()`, array indexing without bounds check
- Async: missing `.await`, unbounded channel sends, no timeout on external calls

### 2. Architecture Boundaries
- `theo-domain` depends on zero other crates?
- Apps import only `theo-application` and `theo-api-contracts`?
- No engine crate depends on another engine crate (except retrieval → graph)?
- Governance on the critical path, not bypassed?

### 3. Security
- No secrets in code (API keys, tokens, passwords)
- Command injection in `theo-tooling` bash tool?
- Path traversal in file operations?
- Input validation at system boundaries (CLI args, Tauri commands, LLM responses)?

### 4. Performance
- Unbounded allocations (Vec growing without limit)?
- Blocking I/O on async runtime (no `tokio::fs` instead of `std::fs`)?
- Unnecessary `.clone()` where borrow suffices?
- N+1 patterns in graph traversal?

### 5. Test Coverage
- New public functions have tests?
- Error paths tested (not just happy path)?
- Tests follow AAA pattern?
- Tests are deterministic (no timing dependencies)?

### 6. Frontend (React/TypeScript)
- No `any` types
- Proper loading/error states
- Radix UI used correctly (accessible names, keyboard support)
- Tauri IPC has proper error handling
- No inline styles when Tailwind class exists

### 7. Agent Runtime
- State machine transitions are valid?
- Promise gate cannot be bypassed?
- Context loops emit correctly?
- done() requires real evidence?

## Output Format

For each finding:
```
[SEVERITY] file:line — description
  Suggestion: how to fix
```

Severities: CRITICAL (must fix), HIGH (should fix), MEDIUM (improve), LOW (nitpick)

Argument: $ARGUMENTS
