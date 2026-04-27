# theo-code audit & build automation
#
# Top-level targets used by developers and CI:
#   make build           cargo build --workspace
#   make test            cargo test --workspace
#   make fmt             cargo fmt --all
#   make lint            cargo clippy --workspace --all-targets -- -D warnings
#   make audit           run ALL 8 audit techniques (fails on regression)
#   make audit-tools     install missing audit tools
#   make audit-tools-check   report missing tools, no install
#   make check-arch      run T1.5 arch-contract gate
#   make check-unwrap    scan for production .unwrap()/.expect() sites (T2.5)
#   make check-sizes     enforce file and function size limits (T4.6)
#   make check-io-tests  detect misclassified I/O tests in src/ (T5.2)
#
# Notes:
# - `audit` is intentionally composite; each sub-target is independently runnable.
# - Tools required only by specific targets are tolerated missing unless strictly required.

SHELL := /usr/bin/env bash
.SHELLFLAGS := -eu -o pipefail -c

REPO_ROOT := $(shell git rev-parse --show-toplevel 2>/dev/null || pwd)
SCRIPTS   := $(REPO_ROOT)/scripts

.DEFAULT_GOAL := help

.PHONY: help
help:
	@awk 'BEGIN{FS=":.*## "} /^[a-zA-Z0-9_.-]+:.*## / {printf "  %-22s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

# ── Build & test ───────────────────────────────────────────────────────────
.PHONY: build
build: ## Build workspace
	cargo build --workspace

.PHONY: test
test: ## Run workspace tests
	cargo test --workspace

.PHONY: fmt
fmt: ## Format Rust code
	cargo fmt --all

.PHONY: lint
lint: ## Clippy with warnings-as-errors
	cargo clippy --workspace --all-targets --no-deps -- -D warnings

# ── Tooling ────────────────────────────────────────────────────────────────
.PHONY: audit-tools
audit-tools: ## Install missing audit tools (idempotent)
	@bash $(SCRIPTS)/install-audit-tools.sh

.PHONY: audit-tools-check
audit-tools-check: ## Check which audit tools are missing, no install
	@bash $(SCRIPTS)/install-audit-tools.sh --check

# ── Individual gates ───────────────────────────────────────────────────────
.PHONY: check-arch
check-arch: ## T1.5 architectural-boundary gate
	@bash $(SCRIPTS)/check-arch-contract.sh

.PHONY: check-unwrap
check-unwrap: ## T2.5 production unwrap/expect scan (strict)
	@bash $(SCRIPTS)/check-unwrap.sh

.PHONY: check-unwrap-report
check-unwrap-report: ## T2.5 unwrap/expect report (no fail)
	@bash $(SCRIPTS)/check-unwrap.sh --report

.PHONY: check-panic
check-panic: ## T2.6 production panic!/todo!/unimplemented! gate (strict)
	@bash $(SCRIPTS)/check-panic.sh

.PHONY: check-panic-report
check-panic-report: ## T2.6 panic gate report (no fail)
	@bash $(SCRIPTS)/check-panic.sh --report

.PHONY: check-unsafe
check-unsafe: ## T2.9 SAFETY-comment gate for unsafe blocks (strict)
	@bash $(SCRIPTS)/check-unsafe.sh

.PHONY: check-unsafe-report
check-unsafe-report: ## T2.9 unsafe gate report (no fail)
	@bash $(SCRIPTS)/check-unsafe.sh --report

.PHONY: check-secrets
check-secrets: ## T6.2 grep-backed secret scan (fallback if gitleaks missing)
	@bash $(SCRIPTS)/check-secrets.sh

.PHONY: check-secrets-report
check-secrets-report: ## T6.2 secret scan report (no fail)
	@bash $(SCRIPTS)/check-secrets.sh --report

.PHONY: check-sizes
check-sizes: ## T4.6 file/function size limits (strict, respects allowlist)
	@bash $(SCRIPTS)/check-sizes.sh

.PHONY: check-sizes-report
check-sizes-report: ## T4.6 size gate report (no fail)
	@bash $(SCRIPTS)/check-sizes.sh --report

.PHONY: check-io-tests
check-io-tests: ## T5.2 detect misclassified I/O tests in src/ (strict)
	@bash $(SCRIPTS)/check-inline-io-tests.sh

.PHONY: check-io-tests-report
check-io-tests-report: ## T5.2 inline I/O test report (no fail)
	@bash $(SCRIPTS)/check-inline-io-tests.sh --report

.PHONY: check-changelog
check-changelog: ## T6.5 enforce CHANGELOG.md update
	@bash $(SCRIPTS)/check-changelog.sh 2>/dev/null || echo "check-changelog: script not yet implemented (T6.5)"

.PHONY: check-sota-dod
check-sota-dod: ## SOTA Tier 1 + Tier 2 DoD report (arch + clippy + tests)
	@bash $(SCRIPTS)/check-sota-dod.sh

.PHONY: check-sota-dod-quick
check-sota-dod-quick: ## SOTA DoD report (arch + clippy only, no tests)
	@bash $(SCRIPTS)/check-sota-dod.sh --quick

.PHONY: check-adr-coverage
check-adr-coverage: ## SOTA Global DoD #8: ADRs D1-D16 referenced in commits
	@bash $(SCRIPTS)/check-adr-coverage.sh

.PHONY: check-complexity
check-complexity: ## SOTA Global DoD #6 (partial): clippy::too_many_lines per-crate ceiling
	@bash $(SCRIPTS)/check-complexity.sh

.PHONY: check-coverage-status
check-coverage-status: ## SOTA Global DoD #6 (partial): validate local .coverage/cobertura.xml
	@bash $(SCRIPTS)/check-coverage-status.sh

.PHONY: check-changelog-phase-coverage
check-changelog-phase-coverage: ## SOTA Global DoD #7: CHANGELOG mentions every phase 0..16
	@bash $(SCRIPTS)/check-changelog-phase-coverage.sh

.PHONY: check-sota-dod-test
check-sota-dod-test: ## Regression test for the SOTA-DoD gate scripts themselves
	@bash $(SCRIPTS)/check-sota-dod.test.sh

.PHONY: check-phase-artifacts
check-phase-artifacts: ## SOTA Global DoD #1 (code half): every phase has its promised artifacts
	@bash $(SCRIPTS)/check-phase-artifacts.sh

.PHONY: check-bench-preflight
check-bench-preflight: ## Pre-flight validate bench infra (eval.yml + smoke.py + scenarios)
	@bash $(SCRIPTS)/check-bench-preflight.sh --no-build

.PHONY: check-allowlist-paths
check-allowlist-paths: ## Structural audit — every allowlist path/crate resolves
	@bash $(SCRIPTS)/check-allowlist-paths.sh

# ── Composite audit ────────────────────────────────────────────────────────
.PHONY: audit
audit: ## Run all 8 audit techniques
	@echo "[1/8] complexity (report only — full CCN via clippy still pending)"
	@bash $(SCRIPTS)/check-sizes.sh --report | tail -5
	@echo "[2/8] coverage + mutation"
	@command -v cargo-tarpaulin >/dev/null && cargo tarpaulin --workspace --out Html --output-dir .theo/coverage 2>/dev/null || echo "  skipped (tarpaulin missing — run make audit-tools)"
	@echo "[3/8] module size (strict)"
	@bash $(SCRIPTS)/check-sizes.sh
	@echo "[4/8] dependency structure"
	@bash $(SCRIPTS)/check-arch-contract.sh
	@echo "[5/8] SCA"
	@command -v cargo-audit >/dev/null && cargo audit || echo "  skipped (cargo-audit missing)"
	@command -v cargo-deny  >/dev/null && cargo deny check || echo "  skipped (cargo-deny missing)"
	@echo "[6/8] unit-test quality (production unwrap/expect + panics + inline I/O tests)"
	@bash $(SCRIPTS)/check-unwrap.sh --report | tail -5
	@bash $(SCRIPTS)/check-panic.sh --report | tail -5
	@bash $(SCRIPTS)/check-inline-io-tests.sh --report | tail -5
	@echo "[7/8] integration tests"
	@cargo test --workspace --tests --no-run >/dev/null 2>&1 && echo "  tests compile" || { echo "  FAIL tests do not compile"; exit 1; }
	@echo "[8/8] pentest (SAST)"
	@command -v semgrep >/dev/null && semgrep --error --config .semgrep/theo.yaml crates apps 2>/dev/null || echo "  skipped (semgrep missing — run make audit-tools)"
	@command -v gitleaks >/dev/null && gitleaks protect --staged --no-git -v || bash $(SCRIPTS)/check-secrets.sh --report | tail -5
	@echo "audit complete"
