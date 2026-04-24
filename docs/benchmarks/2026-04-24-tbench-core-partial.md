# Theo Code — Terminal-Bench Core (partial, mid-run)

## Run manifest

| Field | Value |
|---|---|
| Date (UTC) | 2026-04-24T13:00:31Z |
| Theo SHA | `9f750f9` (develop) |
| Model | `gpt-5.4` (OAuth Codex via ChatGPT) |
| Dataset | `terminal-bench-core==0.1.1` |
| Harness | `terminal-bench` (via `tb run`, official) |
| Concurrency | 4 |
| Max iter (theo) | 20 |
| Host | DigitalOcean s-8vcpu-16gb (NYC1) |
| Status at snapshot | **mid-run** — 39/~80 trials complete |

## Headline numbers (n=39)

| Metric | Value |
|---|---:|
| **Pass rate** | **8/39 = 20.5%** |
| Total cost | $31.89 (avg $0.89/trial) |
| Total tokens | 5,994,802 |
| Total LLM calls | 577 (avg 17.5/trial) |
| Total tool calls | 951 (avg 28.8/trial) |
| Tool dispatch success rate | 97.3% |
| Avg agent duration | ~3 min/trial |
| Completion rate | 0.78 tasks/min |

## Failure root-cause taxonomy (the actionable view)

| Cause | Count | % | What it means |
|---|---:|---:|---|
| **iter_limit** | 18 | 50% | theo hit max-iter=20 still working |
| **tests_disagree** | 8 | 22% | theo `success: true` but tb tests fail |
| **ok** (resolved) | 7 | 19% | passed |
| **no_sidecar** | 3 | 8% | telemetry capture failed |
| **timeout** | 1 | 3% | tb killed agent |
| **early_giveup** | 1 | 3% | theo declared impossible (build-linux-kernel) |

## Key insight: `tests_disagree` (8 trials) decomposes into 2 patterns

### Pattern A — theo overestimates own success (3 trials)
Theo's `summary` explicitly says "blocked / couldn't complete" but `success: true`:
- `git-multibranch` — 20 iters, theo: "sandbox blocks /git, /var/www writes"
- `eval-mteb` — 20 iters, theo: "blockers hit while trying mteb==1.36.8"
- `security-vulhub-minio` — 1 iter, theo refused: "I can't help retrieve credentials"

**Bug**: theo's `success` flag means "agent completed run" not "task verified done". `done` meta-tool needs verification gate.

### Pattern B — theo's solution passes most tests but fails edge cases (5 trials)
- `fibonacci-server` — 5/6 tests passed, fails on negative number handling
- `intrusion-detection` — 4/7 tests passed, missed rules-file usage + error handling
- `download-youtube` — theo says successful download, test verifies content/timing
- `blind-maze-explorer-algorithm.easy` — theo built solver, test failed on specific case
- `get-bitcoin-nodes` — theo built service, test failed on validation

**Bug**: theo doesn't run tests itself before declaring done. Most scaffolds (Codex, ForgeCode) execute the test script as part of the loop.

## Iterations — max_iter=20 is bound for 64% of trials

```
Iter buckets (n=33 with sidecar):
  1-3   ▏ 1 trial  (instant refusal)
  6-10  ▎ 2 trials (genuine quick wins)
  14-19 ▌ 9 trials (mix of resolved + ran out)
  20-21 ███████████ 21 trials (iter limit binding)
```

**Decision**: bumping `max_iter=30` would likely lift pass rate by 3-7pp at cost of ~$0.30/trial more. Not yet — see "Why we don't tune yet" below.

## Tool dispatch — solid foundation

- 925 / 951 = **97.3% success rate**
- 28.8 tool calls per trial average
- No catastrophic tool failure clusters

This is theo's strongest layer. Loop control + dispatch work well.

## OTLP / observability — wired but blocked

- **1989 spans were exported by theo to the collector** ✓ (the `--features otel` build works, `OTLP_ENDPOINT` env reaches container)
- File exporter blocked: `permission denied: /var/log/otel/spans.jsonl` (Docker volume mount perm bug)
- Jaeger gRPC reject: `unknown service opentelemetry.proto.collector.trace.v1.TraceService` (Jaeger 1.62 doesn't accept OTLP gRPC on 14250 — needs different port/protocol)

**Action**: fix collector volume permissions + use Jaeger's HTTP OTLP receiver. Re-run will populate spans.jsonl for percentile latency analysis.

## Resolved trials (8)

| Task | Iters | Tools | Cost | Pattern |
|---|---:|---:|---:|---|
| swe-bench-fsspec | 21 | 30 | $1.40 | Hard, used full budget |
| fix-pandas-version | 20 | 57 | $0.83 | Heavy tool-use, just fit |
| grid-pattern-transform | 6 | 8 | $0.18 | Genuine quick win |
| blind-maze-explorer-5x5 | 15 | 38 | $0.68 | Game/maze, multi-iter |
| organization-json-generator | 14 | 23 | $0.50 | CSV/JSON manipulation |
| blind-maze-explorer-algorithm | 21 | 40 | $1.47 | Hard, used full budget |
| password-recovery | 17 | 46 | $0.80 | Forensics, multi-iter |
| (8th from current run, > snapshot) | — | — | — | — |

## Concrete bug list (high → low priority)

1. **theo `success` flag overstates completion** (Pattern A above) — *theo runtime bug*
2. **theo doesn't run task tests before `done`** (Pattern B above) — *theo prompt/loop design gap*
3. **theo refuses benign security tasks** (`security-vulhub-minio`) — *safety alignment too strict*
4. **theo gives up early on env-restricted tasks** (`build-linux-kernel-qemu`) — *design choice; arguably correct*
5. **OTLP file exporter perm bug** in `apps/theo-benchmark/otlp/docker-compose.yml` — *bench infra bug*
6. **Jaeger gRPC port wrong** — *bench infra bug*
7. **`--max-iter 20` is binding for 64% of trials** — likely under-budgeted but defer fix

## Why we don't tune yet

We have **39 of ~80 trials** = 49% complete. Tuning `max_iter` now risks invalidating comparison across the run. Continue current run to completion, then re-run with fixes 1-2 (the high-value theo bugs) for a v2 baseline.

## Per-trial cost outliers

### Most expensive (5)
- `run-pdp11-code` — 21 iters, 333K tokens, **$1.94**
- `count-dataset-tokens` — 21 iters, 363K tokens, **$1.91**
- `jupyter-notebook-server` — 21 iters, 313K tokens, **$1.66**
- `super-benchmark-upet` — 21 iters, 317K tokens, **$1.64**
- `blind-maze-explorer-algorithm` — 21 iters, 281K tokens, **$1.47** *(resolved!)*

Pattern: high-iter trials = 280-360K tokens = $1.50-$2/each. At 80 tasks total cost = ~$70.

### Cheapest (5)
- `security-vulhub-minio` — 1 iter, 5K tokens, **$0.025** *(theo refused)*
- `grid-pattern-transform` — 6 iters, 34K tokens, **$0.18** *(resolved)*
- `fibonacci-server` — 8 iters, 43K tokens, **$0.23**
- `organization-json-generator` — 14 iters, 94K tokens, **$0.50** *(resolved)*
- `build-linux-kernel-qemu` — 14 iters, 98K tokens, **$0.51** *(theo gave up)*

## Next data collection priorities (when run completes)

1. **Per-tool breakdown** — agent.log doesn't contain raw tool dispatch JSON; need to wire OTLP file exporter properly to get `tool.call` span names
2. **LLM call latency p50/p95** — same dependency on OTLP fix
3. **Token distribution per tool type** (read vs bash vs edit) — needs OTLP `gen_ai.usage.*` per call
4. **Compare across 2 runs** to estimate variance (pass rate stability)

## Reproduction

```bash
# On droplet
cd /opt/theo-code && git checkout 9f750f9
bash scripts/bench/build-portable.sh   # produces /opt/theo-bin/theo glibc-2.34
docker compose -f apps/theo-benchmark/otlp/docker-compose.yml up -d
THEO_SKIP_ONBOARDING=1 THEO_MODEL=gpt-5.4 THEO_MAX_ITER=20 \
  OTLP_ENDPOINT=http://172.17.0.1:4317 \
  /opt/theo-bench-venv/bin/tb run \
    --dataset terminal-bench-core==0.1.1 \
    --agent-import-path tbench.agent:TheoAgent \
    --n-concurrent 4 --output-path <out>
```
