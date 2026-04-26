#!/usr/bin/env bash
# install-audit-tools.sh
#
# Idempotent installer for every CLI the code-audit skill depends on.
# Covers:
#   - Rust: cargo-audit, cargo-deny, cargo-outdated, cargo-tarpaulin,
#           cargo-mutants, cargo-modules, cargo-geiger
#   - Python: semgrep
#   - Binary: gitleaks, osv-scanner (best-effort via package manager)
#   - Node: handled by `npm install` inside apps/theo-ui
#
# Safe to re-run: each step skips if the tool is already present.
#
# Usage:
#   scripts/install-audit-tools.sh              # install everything
#   scripts/install-audit-tools.sh --rust       # Rust only
#   scripts/install-audit-tools.sh --check      # no-install, report status
#
# Exit codes:
#   0  success (or all already installed)
#   1  one or more installs failed
#   2  --check mode found missing tools

set -euo pipefail

MODE="install"
SECTION="all"

for arg in "$@"; do
    case "$arg" in
        --check)   MODE="check" ;;
        --rust)    SECTION="rust" ;;
        --python)  SECTION="python" ;;
        --binary)  SECTION="binary" ;;
        --help|-h)
            sed -n '2,30p' "$0"
            exit 0
            ;;
        *) echo "unknown flag: $arg" >&2; exit 64 ;;
    esac
done

missing=()
installed=()
skipped=()

log()  { printf '  [%s] %s\n' "$1" "$2"; }
have() { command -v "$1" >/dev/null 2>&1; }

ensure_cargo_bin() {
    local name="$1" crate="${2:-$1}"
    if have "$name"; then
        skipped+=("$name")
        log OK "$name already installed ($(command -v "$name"))"
        return 0
    fi
    if [[ "$MODE" == "check" ]]; then
        missing+=("$name")
        log MISS "$name (install: cargo install $crate)"
        return 0
    fi
    log INSTALL "$name via cargo install $crate"
    if cargo install --locked "$crate" >/dev/null 2>&1; then
        installed+=("$name")
    else
        # retry without --locked (some crates ship without Cargo.lock)
        if cargo install "$crate" >/dev/null 2>&1; then
            installed+=("$name")
        else
            missing+=("$name")
            log FAIL "cargo install $crate failed"
        fi
    fi
}

ensure_pip_tool() {
    local name="$1" pkg="${2:-$1}"
    if have "$name"; then
        skipped+=("$name")
        log OK "$name already installed"
        return 0
    fi
    if [[ "$MODE" == "check" ]]; then
        missing+=("$name")
        log MISS "$name (install: pipx install $pkg)"
        return 0
    fi
    if have pipx; then
        log INSTALL "$name via pipx"
        pipx install "$pkg" >/dev/null 2>&1 && installed+=("$name") || missing+=("$name")
    elif have pip3; then
        log INSTALL "$name via pip3 --user"
        pip3 install --user "$pkg" >/dev/null 2>&1 && installed+=("$name") || missing+=("$name")
    else
        missing+=("$name")
        log FAIL "no pipx/pip3 available for $name"
    fi
}

ensure_binary_tool() {
    local name="$1" hint="$2"
    if have "$name"; then
        skipped+=("$name")
        log OK "$name already installed"
        return 0
    fi
    missing+=("$name")
    if [[ "$MODE" == "check" ]]; then
        log MISS "$name ($hint)"
    else
        log SKIP "$name not auto-installable — run: $hint"
    fi
}

printf 'theo-code audit tooling — mode=%s section=%s\n\n' "$MODE" "$SECTION"

if [[ "$SECTION" == "all" || "$SECTION" == "rust" ]]; then
    echo "[rust]"
    ensure_cargo_bin cargo-audit
    ensure_cargo_bin cargo-deny
    ensure_cargo_bin cargo-outdated
    ensure_cargo_bin cargo-tarpaulin
    ensure_cargo_bin cargo-mutants
    ensure_cargo_bin cargo-modules
    ensure_cargo_bin cargo-geiger
    echo
fi

if [[ "$SECTION" == "all" || "$SECTION" == "python" ]]; then
    echo "[python]"
    ensure_pip_tool semgrep
    echo
fi

if [[ "$SECTION" == "all" || "$SECTION" == "binary" ]]; then
    echo "[binary]"
    ensure_binary_tool gitleaks    "see https://github.com/gitleaks/gitleaks/releases or: go install github.com/gitleaks/gitleaks/v8@latest"
    ensure_binary_tool osv-scanner "see https://github.com/google/osv-scanner or: go install github.com/google/osv-scanner/v2/cmd/osv-scanner@latest"
    echo
fi

echo "[summary]"
printf '  installed: %s\n' "${#installed[@]}"
printf '  skipped (already present): %s\n' "${#skipped[@]}"
printf '  missing/failed: %s\n' "${#missing[@]}"

if (( ${#missing[@]} > 0 )); then
    printf '\nMissing tools:\n'
    for m in "${missing[@]}"; do printf '  - %s\n' "$m"; done
    if [[ "$MODE" == "check" ]]; then exit 2; else exit 1; fi
fi
exit 0
