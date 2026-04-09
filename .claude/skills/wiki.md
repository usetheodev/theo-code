# /wiki — Generate or Update Code Wiki

Generate or update the project's code wiki. Creates if not exists, incremental update if exists.

## When to use

- First time in a project: `/wiki` to generate initial documentation
- After code changes: `/wiki` to update the wiki with new changes
- When you need to understand the codebase structure

## What it does

1. Parses all source files (Rust, Python, Go, TypeScript, etc.)
2. Builds a code graph (symbols, calls, imports, dependencies)
3. Detects module communities via Leiden clustering
4. Generates wiki pages with:
   - Module descriptions (from Cargo.toml, //! docs, README.md)
   - Entry points and public API (grouped by type)
   - Cross-module dependencies with [[wiki-links]]
   - Test coverage statistics
   - Runtime notes (if execution data was ingested)
5. Writes to `.theo/wiki/` as markdown files

## Action

Call the `wiki_generate` tool with no arguments. It operates on the current project directory.

If the wiki already exists and the code hasn't changed, it will skip generation (cache is fresh).

## Output

Reports: pages generated, pages updated, pages skipped, duration.
