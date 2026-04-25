# Coverage baselines — REMEDIATION_PLAN T0.3

This directory tracks line-rate baselines for `theo-agent-runtime`.
The check is intentionally per-module so that a regression in one
file (e.g. `pilot.rs` dropping from 31% to 10%) trips the gate even
when the workspace average looks fine.

## Files

- `baseline-<git-short-sha>.tsv` — frozen per-module line-rates from
  the commit `<sha>`. Format is `<module-path>\t<line-rate>` (TSV).
  The current canonical baseline is the `tsv` file with the most
  recent SHA — at the time of writing, `baseline-a51f58f.tsv`.
- `cobertura.xml` (gitignored) — raw tarpaulin output, regenerated
  on every run of `scripts/check-coverage.sh`.

## Regenerating the baseline

```bash
cargo tarpaulin -p theo-agent-runtime --out Xml \
    --output-dir .coverage --skip-clean --timeout 300 --no-fail-fast
tr '<' '\n<' < .coverage/cobertura.xml \
    | grep 'package name="crates/theo-agent-runtime' \
    | sed 's/.*name="\(crates\/theo-agent-runtime[^"]*\)" line-rate="\([^"]*\)".*/\1\t\2/' \
    > ".coverage/baseline-$(git rev-parse --short HEAD).tsv"
```

The `--no-fail-fast` flag is required because there is one
pre-existing flaky env-mutating test (`project_config::tests::env_override_does_not_affect_unset_fields`)
that fails under tarpaulin's serial scheduling. Coverage data is still
emitted for all other tests.

## CI gate

`scripts/check-coverage.sh` re-runs tarpaulin, compares the per-module
line-rates against the canonical baseline, and exits non-zero if any
module drops by more than **2 percentage points**. New modules with
no baseline entry are accepted as-is on first appearance.

## Acceptance threshold

`MAX_DROP_PP=2.0` — a 2-percentage-point drop is the noise floor for
day-to-day runs. Larger drops require either an explicit baseline
update (commit a new `baseline-<sha>.tsv`) or a justification in the
PR description.
