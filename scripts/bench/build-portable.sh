#!/usr/bin/env bash
# Build a portable theo binary inside rust:1.95-slim-bookworm (Debian 12,
# glibc 2.36) — this matches the tb container baseline so the binary
# works on every tb task image. FAANG standard: build in the lowest
# common denominator runtime.
#
# Run on the droplet:
#   bash scripts/bench/build-portable.sh
# Output: /opt/theo-bin/theo

set -euo pipefail

REPO=/opt/theo-code
OUT_DIR=/opt/theo-bin
OUT="$OUT_DIR/theo"
TARGET_DIR=/opt/theo-target-portable

mkdir -p "$OUT_DIR" "$TARGET_DIR"

echo "[build-portable] building inside rust:1.95-slim-bookworm (glibc 2.36)..."
docker run --rm \
  -v "$REPO:/repo" \
  -v "$TARGET_DIR:/repo/target" \
  -e CARGO_HOME=/repo/target/.cargo \
  -w /repo \
  rust:1.95-slim-bookworm \
  bash -c '
    set -e
    apt-get update -qq >/dev/null
    apt-get install -y -qq --no-install-recommends \
      pkg-config libssl-dev cmake build-essential ca-certificates >/dev/null
    cargo build --release --features otel -p theo --bin theo
  '

BUILT=/opt/theo-target-portable/release/theo
if [ ! -f "$BUILT" ]; then
  echo "[build-portable] FAIL: binary not produced at $BUILT"
  exit 1
fi

cp "$BUILT" "$OUT"
chmod +x "$OUT"
echo "[build-portable] OK: $OUT ($(du -h "$OUT" | awk '{print $1}'))"
file "$OUT"
echo ""
echo "[build-portable] glibc version requirements:"
objdump -T "$OUT" 2>/dev/null | grep -oE 'GLIBC_[0-9.]+' | sort -V -u | tail -10
echo ""
"$OUT" --version
