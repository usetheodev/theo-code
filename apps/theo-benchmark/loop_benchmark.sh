#!/bin/bash
# Optimization loop: build → benchmark → analyze → repeat
# Usage: ./benchmark/loop_benchmark.sh <vllm_url>

set -e

VLLM_URL=${1:-"http://localhost:8000"}
REPO_PATH="."
BIN="./target/release/theo-code"
ITER=1

echo "========================================"
echo "GRAPHCTX Optimization Loop"
echo "API: $VLLM_URL"
echo "========================================"

# Build release
echo "[iter $ITER] Building release..."
cargo build --release 2>&1 | grep -E "Finished|error" | tail -1

# Verify API
echo "[iter $ITER] Checking API..."
curl -s --connect-timeout 5 "$VLLM_URL/v1/models" | python3 -c "import sys,json; print('Model:', json.load(sys.stdin)['data'][0]['id'])" 2>/dev/null || {
    echo "ERROR: API not reachable at $VLLM_URL"
    exit 1
}

# Run benchmark
echo "[iter $ITER] Running benchmark..."
VLLM_URL=$VLLM_URL REPO_PATH=$REPO_PATH THEO_CODE_BIN=$BIN python3 benchmark/run_benchmark.py 2>&1

# Analyze
echo ""
echo "[iter $ITER] Analysis:"
python3 -c "
import json
results = json.load(open('benchmark/results.json'))
with_results = [r for r in results if r['mode'] == 'with_graphctx']
without_results = [r for r in results if r['mode'] == 'without_graphctx']

with_success = sum(1 for r in with_results if r['success'])
without_success = sum(1 for r in without_results if r['success'])
with_tokens = sum(r['total_tokens'] for r in with_results)
without_tokens = sum(r['total_tokens'] for r in without_results)
with_inter = sum(r['interactions'] for r in with_results)

print(f'Success: {with_success}/5 (baseline: {without_success}/5)')
print(f'Tokens:  {with_tokens:,} (baseline: {without_tokens:,}) = {(1-with_tokens/max(without_tokens,1))*100:.0f}% reduction')
print(f'Avg interactions: {with_inter/5:.1f}')
print()

# Per-task breakdown
for r in with_results:
    status = 'PASS' if r['success'] else 'FAIL'
    print(f'  [{status}] {r[\"task_id\"]:20s} int={r[\"interactions\"]:2d} tok={r[\"total_tokens\"]:6d}')

# Identify failures
failures = [r for r in with_results if not r['success']]
if failures:
    print()
    print('FAILURES to investigate:')
    for f in failures:
        print(f'  {f[\"task_id\"]}: {f[\"interactions\"]} interactions, {f[\"total_tokens\"]} tokens')
"

echo ""
echo "Results saved to benchmark/results.json"
echo "Iteration $ITER complete."
