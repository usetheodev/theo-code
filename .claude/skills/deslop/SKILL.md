---
name: deslop
description: Use when asked to "deslop", "clean up", "simplify code", or after making changes that need refinement. Covers Rust and TypeScript/React code in the Theo Code workspace.
---

# Deslop — Code Simplification

You are an expert code simplification specialist. Refine recently modified code while preserving exact functionality.

## Rules

1. **Never change what the code does** — only how it does it
2. **Only touch recently modified code** — unless explicitly told to review broader scope
3. **Language-specific standards apply**:

### Rust
- Replace verbose match arms with `?` operator where possible
- Replace `if let Some(x) = ... { x } else { return Err(...) }` with `?.ok_or(...)?`
- Remove unnecessary `.clone()` — check if borrow works
- Replace `vec.iter().map(...).collect::<Vec<_>>()` with more idiomatic alternatives when simpler
- Consolidate related `use` statements
- Remove `#[allow(dead_code)]` if the code is actually used
- Replace `String::from("...")` with `"...".to_string()` or `.into()` consistently
- Simplify error types — if a variant is never constructed, remove it

### TypeScript/React
- Replace nested ternaries with early returns or switch
- Replace `space-y-*` with `flex flex-col gap-*`
- Replace `w-X h-X` with `size-X` when equal
- Use `cn()` for conditional classes instead of template literals
- Remove unused imports and props
- Replace `useEffect` + `useState` with derived state where possible
- Remove unnecessary `React.Fragment` wrapping single children

## Process

1. Identify recently modified files via `git diff --name-only`
2. For each file, read and identify simplification opportunities
3. Apply changes preserving all behavior
4. Verify: `cargo check` for Rust, type correctness for TypeScript
5. Report what was simplified and why

Argument: $ARGUMENTS
