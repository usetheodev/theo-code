# Mutation Testing — Test Quality Validation

Use when asked to validate test quality, check if tests are meaningful, or run mutation testing.

## Trigger
- User says "mutants", "mutation testing", "test quality", "are tests meaningful"
- After writing significant new tests
- To validate that tests actually catch bugs

## Prerequisites
```bash
cargo install cargo-mutants  # One-time setup
```

## Usage

Argument: `$ARGUMENTS`

### If argument is a crate name:
```bash
cargo mutants -p $ARGUMENTS --timeout 60 -- --lib 2>&1 | tail -40
```

### If argument is empty (default: critical crates):
Run on the most critical crates in order:
```bash
cargo mutants -p theo-agent-runtime --timeout 60 -- --lib 2>&1 | tail -40
cargo mutants -p theo-governance --timeout 60 -- --lib 2>&1 | tail -40
cargo mutants -p theo-engine-graph --timeout 60 -- --lib 2>&1 | tail -40
```

### If argument is "report":
Summarize the last mutation testing results.

## Interpreting Results

- **Killed**: Mutant was detected by tests. Good.
- **Survived**: Mutant was NOT detected. Test gap — the code can be changed without any test failing.
- **Timeout**: Mutant caused infinite loop or very slow test. Usually OK.
- **Unviable**: Mutant didn't compile. Irrelevant.

Focus on **survived** mutants — these reveal where tests are weak.

## Report Format
```markdown
## Mutation Testing Report — [crate]

### Summary
- Mutants tested: N
- Killed: N (X%)
- Survived: N (Y%) ← THESE NEED ATTENTION
- Timeout: N

### Survived Mutants (test gaps)
- `src/foo.rs:42` — replaced `>` with `>=` in `check_threshold` → no test caught it
- `src/bar.rs:87` — removed `return Err(...)` → no test for error path

### Recommendations
- [ ] Add test for threshold boundary in foo.rs
- [ ] Add error path test for bar.rs
```

## Important
- Mutation testing is SLOW (minutes per crate). Don't run on entire workspace.
- Focus on critical crates: agent-runtime, governance, engine-graph.
- Survived mutants are NOT bugs — they are test coverage gaps.
