#!/usr/bin/env bash
# Pull EVERY artifact from the droplet bench run for offline analysis.
#
# Run locally:
#   bash scripts/bench/collect-everything.sh <date>
# Defaults to .theo/secrets/current-bench-date if no arg given.
#
# Output layout (local):
#   .theo/bench-data/<date>/
#     reports/                   ← rsync of /opt/theo-code/.../reports/<date>
#     otlp/spans.jsonl           ← collector file exporter
#     docker-logs/{collector,jaeger}.log
#     bench-state.json           ← snapshot: tb pid, monitor pid, theo SHA, etc.
#     manifest.json              ← provenance pin

set -uo pipefail

DATE="${1:-$(cat .theo/secrets/current-bench-date 2>/dev/null)}"
[ -z "$DATE" ] && { echo "no date — pass as arg or set .theo/secrets/current-bench-date"; exit 1; }

OUT=".theo/bench-data/$DATE"
mkdir -p "$OUT"/{reports,otlp,docker-logs}

source scripts/bench/lib-droplet.sh

echo "[collect] date=$DATE  out=$OUT"

# 1. Rsync reports (text-only, fast)
echo "[collect] 1/5  rsync reports/$DATE..."
rsync -az --info=stats1 \
  -e "ssh -i $SSH_KEY -o UserKnownHostsFile=$KNOWN_HOSTS" \
  "root@$DROPLET_IP:/opt/theo-code/apps/theo-benchmark/reports/$DATE/" \
  "$OUT/reports/" 2>&1 | tail -5

# 2. OTLP spans (file exporter inside collector container)
echo "[collect] 2/5  pulling OTLP spans..."
ssh_d "docker cp otlp-otel-collector-1:/var/log/otel/spans.jsonl /tmp/spans.jsonl 2>/dev/null && wc -l /tmp/spans.jsonl" || echo "  (no spans file)"
scp -i "$SSH_KEY" -o UserKnownHostsFile="$KNOWN_HOSTS" \
  "root@$DROPLET_IP:/tmp/spans.jsonl" "$OUT/otlp/spans.jsonl" 2>/dev/null \
  || echo "  (skipped — file not pulled)"

# 3. Docker logs
echo "[collect] 3/5  collector + jaeger docker logs..."
ssh_d "docker logs otlp-otel-collector-1 2>&1" > "$OUT/docker-logs/collector.log" 2>/dev/null
ssh_d "docker logs otlp-jaeger-1 2>&1" > "$OUT/docker-logs/jaeger.log" 2>/dev/null
echo "  collector lines: $(wc -l < $OUT/docker-logs/collector.log)"
echo "  jaeger lines: $(wc -l < $OUT/docker-logs/jaeger.log)"

# 4. Bench state snapshot
echo "[collect] 4/5  state snapshot..."
ssh_d "echo '{
  \"date\": \"$DATE\",
  \"droplet_ip\": \"$DROPLET_IP\",
  \"droplet_id\": $DROPLET_ID,
  \"theo_sha\": \"\$(cd /opt/theo-code && git rev-parse HEAD)\",
  \"theo_short_sha\": \"\$(cd /opt/theo-code && git rev-parse --short HEAD)\",
  \"theo_branch\": \"develop\",
  \"tb_pids_running\": [\$(pgrep -f 'tb run' | tr '\n' ',' | sed 's/,\$//')]\\,
  \"monitor_pids_running\": [\$(pgrep -f 'monitor.py' | tr '\n' ',' | sed 's/,\$//')]\\,
  \"docker_containers_active\": \$(docker ps -q | wc -l),
  \"hostname\": \"\$(hostname)\",
  \"kernel\": \"\$(uname -r)\",
  \"cores\": \$(nproc),
  \"mem_total_mb\": \$(awk '/MemTotal/ {printf \"%d\", \$2/1024}' /proc/meminfo),
  \"mem_avail_mb\": \$(awk '/MemAvailable/ {printf \"%d\", \$2/1024}' /proc/meminfo),
  \"disk_pct\": \$(df / | awk 'NR==2 {gsub(\"%\",\"\",\$5); print \$5}')
}'" > "$OUT/bench-state.json"
cat "$OUT/bench-state.json" | python3 -m json.tool 2>/dev/null | head

# 5. Quick stats
echo "[collect] 5/5  inventory..."
TRIALS=$(find "$OUT/reports" -name "results.json" -path "*/raw/*/*/results.json" | wc -l)
SIDECARS=$(find "$OUT/reports" -name "theo-headless.json" | wc -l)
SNAPSHOTS=$(wc -l < "$OUT/reports/tbench-core/monitor.jsonl" 2>/dev/null || echo 0)
SPANS=$(wc -l < "$OUT/otlp/spans.jsonl" 2>/dev/null || echo 0)
TOTAL_BYTES=$(du -sh "$OUT" | awk '{print $1}')

cat > "$OUT/INVENTORY.md" <<EOF
# Bench Data Collection — $DATE

| Artifact | Count |
|---|---|
| Per-trial results.json | $TRIALS |
| Per-trial theo-headless.json (sidecar) | $SIDECARS |
| Monitor snapshots | $SNAPSHOTS |
| OTLP spans (lines) | $SPANS |
| Total size | $TOTAL_BYTES |

Collected at: $(date -u +%FT%TZ)

## Files
\`\`\`
$(find "$OUT" -maxdepth 4 -type d | sort)
\`\`\`
EOF
cat "$OUT/INVENTORY.md"
