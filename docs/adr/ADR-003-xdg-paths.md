# ADR-003: XDG Base Directory Compliance

- **Status**: Accepted
- **Date**: 2026-04-11
- **Deciders**: Meeting 20260411-103954
- **Context**: CLI Professionalization Plan, promoted from T4.5 to Fase 0

## Context

The current theo-cli hardcodes paths to `~/.config/theo/`:

- `apps/theo-cli/src/repl.rs` L284-289 — sessions
- `apps/theo-cli/src/commands.rs` L123-128 — skills
- `apps/theo-cli/src/commands.rs` L186-193 — memory

This ignores the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html) and breaks for users who relocate their config/cache/data directories.

## Decision

Adopt XDG Base Directory spec from F0. All paths resolved via the `dirs` crate with the following mapping:

| Data type | XDG var | `dirs` function | Fallback (Linux) | Theo subdir |
|---|---|---|---|---|
| Config (theme, model, keybindings) | `$XDG_CONFIG_HOME` | `config_dir()` | `~/.config` | `theo/` |
| Session history, memory store | `$XDG_DATA_HOME` | `data_dir()` | `~/.local/share` | `theo/` |
| Cached indices, syntect themes | `$XDG_CACHE_HOME` | `cache_dir()` | `~/.cache` | `theo/` |
| User skills | `$XDG_DATA_HOME` | `data_dir()` | `~/.local/share` | `theo/skills/` |

Environment override `THEO_HOME` forces all directories under a single root (useful for tests, CI, Docker).

## Implementation

```rust
// apps/theo-cli/src/config/paths.rs
use std::path::PathBuf;

pub struct TheoPaths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
}

impl TheoPaths {
    pub fn resolve() -> Self {
        if let Ok(home) = std::env::var("THEO_HOME") {
            let root = PathBuf::from(home);
            return Self {
                config: root.join("config"),
                data: root.join("data"),
                cache: root.join("cache"),
            };
        }
        Self {
            config: dirs::config_dir().unwrap_or_default().join("theo"),
            data: dirs::data_dir().unwrap_or_default().join("theo"),
            cache: dirs::cache_dir().unwrap_or_default().join("theo"),
        }
    }

    pub fn sessions(&self) -> PathBuf { self.data.join("sessions") }
    pub fn memory(&self) -> PathBuf { self.data.join("memory") }
    pub fn skills(&self) -> PathBuf { self.data.join("skills") }
    pub fn config_file(&self) -> PathBuf { self.config.join("config.toml") }
    pub fn syntect_cache(&self) -> PathBuf { self.cache.join("syntect") }
}
```

## Migration

Old path `~/.config/theo/sessions/` → new path `$XDG_DATA_HOME/theo/sessions/`.

**Strategy**: On first run, if new path does not exist AND old path exists, **copy** (not move) to new location. Log migration event. Do not delete old data.

## Alternatives Considered

### Alternative 1: Keep `~/.config/theo/` hardcoded
- **Rejected**: Violates spec, blocks sandboxing, breaks for relocated homes.

### Alternative 2: Single `~/.theo/` root (dotfile)
- **Rejected**: Pollutes home directory, old-style, no separation of data/cache/config lifetimes.

### Alternative 3: Require explicit `--config-dir` flag
- **Rejected**: Friction for default usage; XDG solves this automatically.

## Consequences

### Positive
- Compliant with Linux desktop standards
- Cache can be cleared independently of config
- Data backup strategies work correctly (no cache pollution)
- Test isolation via `THEO_HOME=$TMPDIR/theo`
- Compatible with immutable home directories (ChromeOS-style)

### Negative
- One-time migration code for existing users
- macOS uses `~/Library/Application Support/` via `dirs` — different from Linux but handled by crate
- Windows uses `%APPDATA%` — also handled by crate

### Test Strategy

- Mock `dirs::*` calls via wrapper function
- Set `THEO_HOME=$TMPDIR/test-xdg` in all tests
- Assert path resolution with env var present/absent
- Migration test: create old path, run resolver, assert copy happened

## References

- Plan: `docs/roadmap/cli-professionalization.md` (T0.4, T4.5 — promoted)
- Spec: https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html
- Crate: https://docs.rs/dirs
- Meeting: `.claude/meetings/20260411-103954-cli-professionalization.md`
