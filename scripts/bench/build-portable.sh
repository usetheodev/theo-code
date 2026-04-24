#!/usr/bin/env bash
# Build a portable theo binary using cargo-zigbuild — cross-compiles
# against glibc 2.28 (RHEL 8 / Debian 10 baseline) so it runs on every
# modern container in tb tasks. FAANG standard portability approach.
#
# Run on the droplet:
#   bash scripts/bench/build-portable.sh
# Output: /opt/theo-bin/theo (replaces existing)

set -euo pipefail

REPO=/opt/theo-code
OUT_DIR=/opt/theo-bin
OUT="$OUT_DIR/theo"

source ~/.cargo/env

# 1. Install zig (small, ~50MB, no apt deps)
ZIG_VERSION="0.13.0"
ZIG_ROOT=/opt/zig
if [ ! -x "$ZIG_ROOT/zig" ]; then
  echo "[build-portable] installing zig $ZIG_VERSION..."
  cd /tmp
  curl -fsSL "https://ziglang.org/download/$ZIG_VERSION/zig-linux-x86_64-$ZIG_VERSION.tar.xz" \
    -o zig.tar.xz
  mkdir -p "$ZIG_ROOT"
  tar -xJf zig.tar.xz -C "$ZIG_ROOT" --strip-components=1
  rm zig.tar.xz
fi
export PATH="$ZIG_ROOT:$PATH"
zig version

# 2. Install cargo-zigbuild (cargo-installed, takes ~2min)
if ! command -v cargo-zigbuild >/dev/null; then
  echo "[build-portable] installing cargo-zigbuild..."
  cargo install --quiet cargo-zigbuild --locked
fi

# 3. Add the gnu target if missing
rustup target add x86_64-unknown-linux-gnu

# 4. Build, pinning glibc to 2.28 (RHEL8 / Debian 10 / Ubuntu 18.04 era)
echo "[build-portable] building theo for glibc 2.28..."
cd "$REPO"
CARGO_TARGET_DIR=/opt/theo-target-portable \
  cargo zigbuild --release --features otel \
    -p theo --bin theo \
    --target x86_64-unknown-linux-gnu.2.28

BUILT=/opt/theo-target-portable/x86_64-unknown-linux-gnu/release/theo
if [ ! -f "$BUILT" ]; then
  echo "[build-portable] FAIL: binary not produced at $BUILT"
  exit 1
fi

mkdir -p "$OUT_DIR"
cp "$BUILT" "$OUT"
chmod +x "$OUT"
echo "[build-portable] OK: $OUT ($(du -h "$OUT" | awk '{print $1}'))"
file "$OUT"
echo ""
echo "[build-portable] glibc version requirements:"
objdump -T "$OUT" 2>/dev/null | grep -oE 'GLIBC_[0-9.]+' | sort -V -u | tail -10
echo ""
"$OUT" --version
