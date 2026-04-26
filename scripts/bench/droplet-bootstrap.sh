#!/usr/bin/env bash
#
# Bootstrap a fresh DigitalOcean droplet for benchmark execution.
# Run via: ssh -i .theo/secrets/bench-droplet-key root@<IP> 'bash -s' < scripts/bench/droplet-bootstrap.sh
#
# Installs:
#   - Docker (official repo, 24.0+)
#   - Rust toolchain (stable)
#   - System deps (git, build-essential, pkg-config, libssl-dev, python3-pip)
#   - terminal-bench (tb) Python package
#   - swebench Python package
#
# Idempotent: safe to re-run.

set -euo pipefail

echo "[bootstrap] $(date -u +%FT%TZ) starting on $(hostname)"

export DEBIAN_FRONTEND=noninteractive

# ── 1. System packages ───────────────────────────────────────────────
apt-get update -y >/dev/null
apt-get install -y --no-install-recommends \
  ca-certificates curl gnupg git build-essential pkg-config libssl-dev \
  python3-pip python3-venv jq tmux htop \
  >/dev/null
echo "[bootstrap] system packages OK"

# ── 2. Docker ────────────────────────────────────────────────────────
if ! command -v docker >/dev/null; then
  install -m 0755 -d /etc/apt/keyrings
  curl -fsSL https://download.docker.com/linux/ubuntu/gpg \
    | gpg --batch --yes --dearmor -o /etc/apt/keyrings/docker.gpg
  chmod a+r /etc/apt/keyrings/docker.gpg
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] \
    https://download.docker.com/linux/ubuntu $(. /etc/os-release && echo "$VERSION_CODENAME") stable" \
    > /etc/apt/sources.list.d/docker.list
  apt-get update -y >/dev/null
  apt-get install -y --no-install-recommends \
    docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin \
    >/dev/null
  systemctl enable --now docker
fi
docker --version
docker compose version
echo "[bootstrap] docker OK"

# ── 3. Rust toolchain ────────────────────────────────────────────────
if ! command -v cargo >/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable --profile minimal
fi
export PATH="$HOME/.cargo/bin:$PATH"
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> /root/.bashrc
cargo --version
echo "[bootstrap] rust OK"

# ── 4. Python tooling for benchmark harness (PEP 668 — Ubuntu 24.04) ─
# Use uv as the package installer to avoid externally-managed-environment.
if ! command -v uv >/dev/null; then
  curl -LsSf https://astral.sh/uv/install.sh | sh
fi
export PATH="$HOME/.local/bin:$PATH"
echo 'export PATH="$HOME/.local/bin:$PATH"' >> /root/.bashrc
uv --version

# Create a project venv for the bench tooling (isolated, no PEP 668 issue).
BENCH_VENV=/opt/theo-bench-venv
[ -d "$BENCH_VENV" ] || uv venv --python 3.12 "$BENCH_VENV"
source "$BENCH_VENV/bin/activate"

# terminal-bench: agent harness
uv pip install --quiet terminal-bench
uv pip install --quiet datasets pytest

# swebench grader (heavy — only install if requested)
if [ "${INSTALL_SWEBENCH:-0}" = "1" ]; then
  uv pip install --quiet swebench
fi
which tb >/dev/null && tb --help 2>&1 | head -3

# Make the venv available system-wide for non-interactive ssh sessions.
ln -sf "$BENCH_VENV/bin/tb" /usr/local/bin/tb 2>/dev/null || true
ln -sf "$BENCH_VENV/bin/python3" /usr/local/bin/bench-python 2>/dev/null || true
echo "BENCH_VENV=$BENCH_VENV" > /etc/profile.d/theo-bench.sh
echo 'source $BENCH_VENV/bin/activate' >> /etc/profile.d/theo-bench.sh
echo "[bootstrap] python tooling OK"

# ── 5. Workspace layout ──────────────────────────────────────────────
mkdir -p /opt/theo-code /var/log/theo-bench
echo "[bootstrap] workspace OK"

echo "[bootstrap] $(date -u +%FT%TZ) DONE"
