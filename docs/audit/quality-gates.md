# Quality Gates — Coverage & Mutation Targets (T5.1)

Targets defined in `docs/audit/remediation-plan.md` §DoD:

- **Branch coverage (tarpaulin):** ≥ 85% workspace-wide.
- **Mutation kill rate (cargo-mutants):** ≥ 60% on core crates
  (`theo-domain`, `theo-application`, `theo-agent-runtime`, `theo-tooling`,
  `theo-engine-retrieval`).

## How to run

```bash
# Per-crate (fast iteration)
cargo tarpaulin -p theo-tooling --lib \
  --exclude-files 'referencias/*' \
  --exclude-files 'apps/*' \
  --exclude-files '.theo/*'

# Workspace (CI)
cargo tarpaulin --workspace --lib \
  --exclude-files 'referencias/*' \
  --exclude-files '.theo/*' \
  --out Html --output-dir .theo/coverage

# Mutation — scoped to a single crate (slow)
cargo mutants --package theo-tooling --test-timeout 60
```

## 2026-04-23 Baseline

Generated with the exclusions above. Every crate in the table below has the
JSON artifact at `.theo/coverage/tarpaulin-report.json` (per-crate).

| Crate | Line coverage | Lines (covered / total) | Target delta |
| --- | --- | --- | --- |
| theo-tooling | **45.92 %** | 2 049 / 4 462 | −39.08 pp |
| theo-domain | **59.30 %** | 1 183 / 1 995 | −25.70 pp |
| theo-api-contracts | *(reporting artefact)* | 0 / 720 | — (13 unit tests; tarpaulin double-counts compile deps for tiny crates) |

Other crates have not yet been run — T5.1 execution is ongoing; each
per-crate run takes 30–120 s compile + test. Next iterations will extend
this table. The workspace-wide tarpaulin + Stryker runs are earmarked for a
CI pipeline because they can exceed 10 min.

## Migration posture

1. **Current (soft-fail):** `make audit` prints coverage but does not fail
   CI. Gives the team two weeks to close the gap without blocking merges.
2. **Hard-fail:** when the workspace baseline crosses 70 % we flip to
   `fail-on-delta`: any PR that lowers coverage for the crates it touches
   must justify in the PR description or is blocked.
3. **Target:** 85 % / 60 % (branch / mutation). Missing-coverage per crate
   tracked via dedicated issues that reference T5.1.

## Known coverage blind spots

- `crates/theo-tooling/src/wiki_tool/mod.rs` (0 / 91 lines): wiki-tool
  scaffolding waiting for the wiki-render E2E test; ship in Phase 5.
- `crates/theo-tooling/src/sandbox/macos.rs`: gated on macOS; skipped on
  Linux CI. Not a blind spot, but a target-gated exclusion.
- `crates/theo-tooling/src/lsp/*`: currently no integration test binding
  a real LSP client; tracked under T2.4 follow-up.

## Related

- **T5.1**: this document.
- **T5.2**: inline I/O tests gate (`scripts/check-inline-io-tests.sh`) keeps
  deterministic unit tests separate from integration-grade I/O tests so
  tarpaulin runs fast in CI.
- **T5.3**: theo-api-contracts unit tests (13 tests) — every `FrontendEvent`
  variant round-trips through serde JSON.
