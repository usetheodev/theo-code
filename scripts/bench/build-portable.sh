#!/usr/bin/env bash
# Build a portable theo binary linked against Debian 11 (glibc 2.31)
# so it runs on every modern Debian/Ubuntu container in tb tasks.
#
# Run on the droplet:
#   bash scripts/bench/build-portable.sh
# Output: /opt/theo-bin/theo (replaces existing)

set -euo pipefail

REPO=/opt/theo-code
OUT=/opt/theo-bin/theo

echo "[build-portable] starting build inside debian:11 container..."
docker run --rm \
  -v "$REPO:/repo" \
  -v "/opt/theo-target-portable:/repo/target" \
  -w /repo \
  rust:1.83-slim-bullseye \
  bash -c "
    apt-get update -qq && apt-get install -y -qq pkg-config libssl-dev cmake build-essential >/dev/null
    cargo build --release --features otel -p theo --bin theo
  "

if [ ! -f /opt/theo-target-portable/release/theo ]; then
  echo "[build-portable] FAIL: binary not produced"
  exit 1
fi

mkdir -p /opt/theo-bin
cp /opt/theo-target-portable/release/theo "$OUT"
chmod +x "$OUT"
echo "[build-portable] OK: $OUT"
ldd "$OUT" | head -10
"$OUT" --version
