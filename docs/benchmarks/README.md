# Theo Code — Benchmark Results

This directory holds public benchmark validation reports per the
plan in `docs/plans/benchmark-validation-plan.md`. Each report is a
dated subdirectory with:

- `manifest.json` — exact provenance (theo SHA, model, host, date)
- `comparison.md` — high-level summary table + cost breakdown
- `<benchmark>/` — per-benchmark raw + analyzed records
- `submission.md` — what was sent to public leaderboards

## Latest

See the most recent dated directory.

## Reproduction

To reproduce a result, check out the `theo_sha` from the manifest and run:

```bash
# On a Docker-enabled host (DigitalOcean droplet, GCP VM, etc.)
git clone --branch develop https://github.com/usetheodev/theo-code /opt/theo-code
cd /opt/theo-code
bash scripts/bench/droplet-bootstrap.sh   # install Docker, Rust, tb, uv
bash scripts/bench/build-portable.sh      # produce glibc-2.34-pinned binary
bash scripts/bench/run-all.sh             # smoke gate → tbench → swe → pro
```

Total wall-clock: ~4-7 hours for the full sweep.
Total LLM cost: ~$300-900 depending on model + retries.
