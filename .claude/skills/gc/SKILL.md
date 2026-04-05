# Garbage Collection — Codebase Cleanup

Use when asked to clean up, find dead code, detect drift, or run a "garbage collection" pass on the codebase.

## Trigger
- User says "gc", "garbage collection", "cleanup", "dead code", "drift"
- After a large feature is merged and code needs tidying
- As a recurring task via `/loop 30m /gc`

## Steps

### 1. Cargo Clippy (warnings scan)
```bash
cargo clippy --workspace --all-targets -- -W clippy::all 2>&1 | head -100
```
Report: number of warnings, group by category (dead_code, unused_imports, etc).

### 2. Dead Code Detection
```bash
grep -rn '#\[allow(dead_code)\]' crates/ apps/ --include='*.rs' | head -30
```
Each `#[allow(dead_code)]` is a candidate for removal. Check if the code is actually used.

### 3. Unused Dependencies
```bash
cargo +nightly udeps --workspace 2>&1 | head -50
```
If `cargo-udeps` is not installed, skip with note. Report crates with unused deps.

### 4. Large Files (SRP candidates)
```bash
find crates/ apps/ -name '*.rs' ! -name '*test*' ! -path '*/test/*' -exec wc -l {} + | sort -rn | head -15
```
Files > 500 lines are candidates for splitting.

### 5. Report
Produce a structured report:

```markdown
## GC Report — [date]

### Clippy Warnings: N
- [list top 5 by category]

### Dead Code Annotations: N
- [list files with #[allow(dead_code)]]

### Unused Dependencies: N
- [list if cargo-udeps available]

### Large Files (>500 lines): N
- [list top 5]

### Suggested Actions
- [ ] Remove dead_code annotation in X (code is unused)
- [ ] Split large file Y into modules
- [ ] Remove unused dep Z from crate W
```

## Important
- This skill DOES NOT make changes. It reports findings.
- Use `/deslop` or manual edits to fix issues found.
- Integrável com `/loop` para monitoramento contínuo.
