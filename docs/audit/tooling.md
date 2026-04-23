# Audit Tooling

Reference for every CLI the code-audit skill depends on. The install script
`scripts/install-audit-tools.sh` is idempotent and safe to re-run.

## One-shot install

```bash
make audit-tools              # installs everything missing
make audit-tools-check        # reports missing, no install
```

## By section

### Rust (via `cargo install`)

| Tool            | Purpose                                            |
| --------------- | -------------------------------------------------- |
| cargo-audit     | CVE scan of `Cargo.lock` via RustSec advisory DB   |
| cargo-deny      | License + bans + advisories policy (see `deny.toml`) |
| cargo-outdated  | Flags deps that have newer major versions          |
| cargo-tarpaulin | Line + branch coverage                             |
| cargo-mutants   | Mutation testing                                   |
| cargo-modules   | Visualise crate module graph                       |
| cargo-geiger    | Counts `unsafe` usage across dep tree              |

Install manually:

```bash
cargo install cargo-audit cargo-deny cargo-outdated \
              cargo-tarpaulin cargo-mutants cargo-modules cargo-geiger
```

### Python (via `pipx` / `pip`)

| Tool     | Purpose                                  |
| -------- | ---------------------------------------- |
| semgrep  | SAST rules for Rust, TS, generic patterns |

```bash
pipx install semgrep
```

### Binary (manual install)

`gitleaks` and `osv-scanner` have no universal installer. Pick whichever matches your environment:

**gitleaks**
- macOS: `brew install gitleaks`
- Linux: download from <https://github.com/gitleaks/gitleaks/releases>
- Go users: `go install github.com/gitleaks/gitleaks/v8@latest`

**osv-scanner**
- macOS: `brew install osv-scanner`
- Linux: download from <https://github.com/google/osv-scanner/releases>
- Go users: `go install github.com/google/osv-scanner/v2/cmd/osv-scanner@latest`

### Node (via `npm` inside `apps/theo-ui`)

Pending T0.2. Devs should add to `apps/theo-ui/package.json`:

```json
"devDependencies": {
  "@stryker-mutator/core": "^8",
  "@stryker-mutator/vitest-runner": "^8",
  "madge": "^6",
  "license-checker": "^25"
}
```

Then `npm install` from `apps/theo-ui/`.

## Tooling inventory

After each install run, the script should update `.theo/tooling-inventory.md`
(T0.1 DoD). This file records the installed version per host and is committed
so the CI environment is reproducible.
