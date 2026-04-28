# theo-compat-harness — orphaned satellite

> **Status:** does not compile. Preserved in tree for git-history reference.
> Excluded from the Cargo workspace via `Cargo.toml [workspace.exclude]`.

This crate was added in commits `914534d` and `3140ce8` ("CLI professionalization
review and Rust terminal ecosystem") as a **compatibility harness** intended to
parse upstream TypeScript sources (`src/commands.ts`, `src/tools.ts`,
`src/entrypoints/cli.tsx`) and validate that the Rust port stays in sync with
some external upstream.

It was never wired up. Its `Cargo.toml` declares dependencies on three sibling
crates that **do not exist anywhere in this repository**:

```
[dependencies]
commands = { path = "../commands" }
tools    = { path = "../tools" }
runtime  = { path = "../runtime" }
```

As a result, `cargo build -p compat-harness` cannot succeed in any working tree.
The `lib.rs` references `commands::CommandRegistry`, `runtime::BootstrapPlan`,
and `tools::ToolRegistry` types that have no producer in this repo.

## Why is it still here?

CLEAN-F1 in `docs/plans/cleanup-2026-04-28.md` evaluated three options:

- **(a) Integrate** — would require recreating `commands` / `tools` / `runtime`
  in `crates/`. Out of scope; these names also collide with vendored TS source.
- **(b) Move to `referencias/`** — `referencias/` is gitignored, so the move
  would lose the source from the repository. The git history would still hold
  it, but the working tree would not.
- **(c) Document as satellite** — keep in tree, add this README, exclude from
  workspace.

Chose **(c)** to preserve the source visibly until someone with context
decides whether to revive or delete the experiment.

## What to do

- **Don't try to compile this crate.** It will fail with "no matching package"
  for `commands`, `tools`, `runtime`.
- **Don't add it to `[workspace.members]`** — `Cargo.toml` explicitly excludes it.
- **If you want to revive it**, you'll need to provide the three missing crates
  AND decide what upstream the harness should validate against (the TS sources
  it parses are also not committed).
- **If you want to delete it**, the git history preserves the implementation;
  no information is lost. `git rm -rf crates/theo-compat-harness/` plus removal
  of the `[workspace.exclude]` entry is sufficient.

## See also

- `docs/plans/cleanup-2026-04-28.md` — CLEAN-F1 task that produced this README.
- Commits `914534d` and `3140ce8` — origin of the harness.
