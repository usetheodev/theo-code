#!/usr/bin/env bash
# Helper library for droplet ssh/scp operations.
# Source this from other bench scripts.

DROPLET_META="${DROPLET_META:-.theo/secrets/droplet.json}"
SSH_KEY="${SSH_KEY:-.theo/secrets/bench-droplet-key}"
KNOWN_HOSTS="${KNOWN_HOSTS:-.theo/secrets/known_hosts}"

if [ ! -f "$DROPLET_META" ]; then
  echo "[lib-droplet] FATAL: $DROPLET_META not found" >&2
  return 1 2>/dev/null || exit 1
fi

DROPLET_IP="$(jq -r '.ip' "$DROPLET_META")"
DROPLET_ID="$(jq -r '.id' "$DROPLET_META")"
DROPLET_USER="${DROPLET_USER:-root}"

ssh_d() {
  ssh -i "$SSH_KEY" \
      -o UserKnownHostsFile="$KNOWN_HOSTS" \
      -o StrictHostKeyChecking=accept-new \
      -o ConnectTimeout=20 \
      -o ServerAliveInterval=30 \
      "$DROPLET_USER@$DROPLET_IP" "$@"
}

scp_to_d() {
  scp -i "$SSH_KEY" \
      -o UserKnownHostsFile="$KNOWN_HOSTS" \
      -o StrictHostKeyChecking=accept-new \
      -r "$@" "$DROPLET_USER@$DROPLET_IP:"
}

scp_from_d() {
  local remote="$1"
  local local_path="$2"
  scp -i "$SSH_KEY" \
      -o UserKnownHostsFile="$KNOWN_HOSTS" \
      -o StrictHostKeyChecking=accept-new \
      -r "$DROPLET_USER@$DROPLET_IP:$remote" "$local_path"
}
