//! T13.1 — DAP server discovery.
//!
//! Probes the system PATH for known Debug Adapter Protocol servers
//! and returns one entry per detected adapter, ready for
//! `DapClient::spawn`. Mirrors `lsp::discovery` in shape; the
//! catalogue is smaller because DAP adapters are language-specific
//! and most languages have one canonical adapter.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// One discovered DAP adapter entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredAdapter {
    /// Short identifier (`"lldb-vscode"`, `"debugpy"`).
    pub name: &'static str,
    /// Absolute path to the binary.
    pub command: PathBuf,
    /// Args needed to enter DAP mode (`["-m", "debugpy.adapter"]`
    /// for the python `debugpy` adapter).
    pub args: Vec<&'static str>,
    /// Languages handled by this adapter (lower-case ASCII).
    pub languages: &'static [&'static str],
    /// File extensions handled (without dot).
    pub file_extensions: &'static [&'static str],
}

struct AdapterSpec {
    name: &'static str,
    binary: &'static str,
    args: &'static [&'static str],
    languages: &'static [&'static str],
    file_extensions: &'static [&'static str],
}

/// Catalogue. Order reflects rough preference within a language.
const KNOWN_ADAPTERS: &[AdapterSpec] = &[
    AdapterSpec {
        name: "lldb-vscode",
        binary: "lldb-vscode",
        args: &[],
        languages: &["c", "cpp", "rust", "swift", "objective-c"],
        file_extensions: &["c", "cc", "cpp", "cxx", "h", "hpp", "rs", "swift", "m"],
    },
    AdapterSpec {
        name: "lldb-dap",
        binary: "lldb-dap",
        args: &[],
        languages: &["c", "cpp", "rust", "swift"],
        file_extensions: &["c", "cc", "cpp", "cxx", "h", "hpp", "rs", "swift"],
    },
    AdapterSpec {
        name: "codelldb",
        binary: "codelldb",
        args: &["--port", "0"],
        languages: &["c", "cpp", "rust"],
        file_extensions: &["c", "cc", "cpp", "rs"],
    },
    AdapterSpec {
        // `python -m debugpy.adapter` is the canonical entry; we
        // still discover by the `debugpy-adapter` shim that ships
        // with recent debugpy releases.
        name: "debugpy",
        binary: "debugpy-adapter",
        args: &[],
        languages: &["python"],
        file_extensions: &["py", "pyi"],
    },
    AdapterSpec {
        name: "delve-dap",
        binary: "dlv",
        args: &["dap"],
        languages: &["go"],
        file_extensions: &["go"],
    },
    AdapterSpec {
        name: "vscode-js-debug",
        binary: "js-debug-adapter",
        args: &[],
        languages: &["javascript", "typescript"],
        file_extensions: &["js", "jsx", "ts", "tsx", "mjs", "cjs"],
    },
    AdapterSpec {
        name: "java-debug",
        binary: "java-debug-server",
        args: &[],
        languages: &["java"],
        file_extensions: &["java"],
    },
];

/// Probe `path_dirs` for each known DAP adapter. Pure: tests pass
/// fake bin dirs without mutating env vars.
pub fn discover_with_path(path_dirs: &[&Path]) -> Vec<DiscoveredAdapter> {
    let mut found = Vec::with_capacity(KNOWN_ADAPTERS.len());
    for spec in KNOWN_ADAPTERS {
        if let Some(command) = locate_executable(spec.binary, path_dirs) {
            found.push(DiscoveredAdapter {
                name: spec.name,
                command,
                args: spec.args.to_vec(),
                languages: spec.languages,
                file_extensions: spec.file_extensions,
            });
        }
    }
    found
}

/// Convenience: probe the current process `PATH`.
pub fn discover() -> Vec<DiscoveredAdapter> {
    let path = env::var_os("PATH").unwrap_or_default();
    let dirs: Vec<PathBuf> = env::split_paths(&path).collect();
    let refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
    discover_with_path(&refs)
}

/// Build a language → adapter lookup. First adapter in `KNOWN_ADAPTERS`
/// order wins on conflict (e.g. lldb-vscode > codelldb for `rust`).
pub fn adapter_for_language(
    adapters: &[DiscoveredAdapter],
) -> HashMap<&'static str, DiscoveredAdapter> {
    let mut map = HashMap::new();
    for ad in adapters {
        for lang in ad.languages {
            map.entry(*lang).or_insert_with(|| ad.clone());
        }
    }
    map
}

/// Find an adapter for a specific language (case-insensitive).
pub fn adapter_for<'a>(
    adapters: &'a [DiscoveredAdapter],
    language: &str,
) -> Option<&'a DiscoveredAdapter> {
    let needle = language.to_ascii_lowercase();
    adapters
        .iter()
        .find(|a| a.languages.contains(&needle.as_str()))
}

fn locate_executable(binary: &str, dirs: &[&Path]) -> Option<PathBuf> {
    for dir in dirs {
        let candidate = dir.join(binary);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode();
        mode & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Local copy of the test helper. We don't reuse lsp::discovery's
    /// because crate-private items aren't reachable across modules in
    /// `#[cfg(test)]`-only modules without cfg gymnastics.
    struct FakeBinDir {
        dir: tempfile::TempDir,
    }

    impl FakeBinDir {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().unwrap(),
            }
        }
        fn add(&self, name: &str) -> PathBuf {
            let path = self.dir.path().join(name);
            let mut f = fs::File::create(&path).unwrap();
            writeln!(f, "#!/bin/sh\nexit 0").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&path).unwrap().permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&path, perms).unwrap();
            }
            path
        }
        fn path(&self) -> &Path {
            self.dir.path()
        }
    }

    #[test]
    fn t131disc_empty_path_returns_no_adapters() {
        let dirs: Vec<&Path> = Vec::new();
        assert!(discover_with_path(&dirs).is_empty());
    }

    #[test]
    fn t131disc_finds_lldb_vscode_when_present() {
        let bin = FakeBinDir::new();
        bin.add("lldb-vscode");
        let adapters = discover_with_path(&[bin.path()]);
        assert_eq!(adapters.len(), 1);
        assert_eq!(adapters[0].name, "lldb-vscode");
        assert!(adapters[0].languages.contains(&"rust"));
        assert!(adapters[0].command.is_absolute());
    }

    #[test]
    fn t131disc_finds_debugpy_for_python() {
        let bin = FakeBinDir::new();
        bin.add("debugpy-adapter");
        let adapters = discover_with_path(&[bin.path()]);
        assert_eq!(adapters.len(), 1);
        assert_eq!(adapters[0].name, "debugpy");
        assert_eq!(adapters[0].languages, &["python"]);
    }

    #[test]
    fn t131disc_finds_dlv_with_dap_arg_for_go() {
        let bin = FakeBinDir::new();
        bin.add("dlv");
        let adapters = discover_with_path(&[bin.path()]);
        assert_eq!(adapters.len(), 1);
        assert_eq!(adapters[0].name, "delve-dap");
        // `dlv` enters DAP mode only when invoked as `dlv dap`.
        assert_eq!(adapters[0].args, vec!["dap"]);
    }

    #[test]
    fn t131disc_finds_multiple_adapters_in_path() {
        let bin = FakeBinDir::new();
        bin.add("lldb-vscode");
        bin.add("debugpy-adapter");
        bin.add("dlv");
        let adapters = discover_with_path(&[bin.path()]);
        assert_eq!(adapters.len(), 3);
    }

    #[test]
    fn t131disc_adapter_for_language_returns_first_match() {
        let bin = FakeBinDir::new();
        bin.add("lldb-vscode");
        bin.add("codelldb");
        let adapters = discover_with_path(&[bin.path()]);
        assert_eq!(adapters.len(), 2);
        // Both claim rust — lldb-vscode is first in KNOWN_ADAPTERS,
        // so it wins.
        let rust = adapter_for(&adapters, "rust").unwrap();
        assert_eq!(rust.name, "lldb-vscode");
    }

    #[test]
    fn t131disc_adapter_for_language_is_case_insensitive() {
        let bin = FakeBinDir::new();
        bin.add("debugpy-adapter");
        let adapters = discover_with_path(&[bin.path()]);
        assert!(adapter_for(&adapters, "Python").is_some());
        assert!(adapter_for(&adapters, "PYTHON").is_some());
    }

    #[test]
    fn t131disc_adapter_for_language_returns_none_for_unknown() {
        let bin = FakeBinDir::new();
        bin.add("lldb-vscode");
        let adapters = discover_with_path(&[bin.path()]);
        assert!(adapter_for(&adapters, "cobol").is_none());
    }

    #[test]
    fn t131disc_adapter_for_language_lookup_table_first_wins() {
        let bin = FakeBinDir::new();
        bin.add("lldb-vscode");
        bin.add("codelldb");
        let adapters = discover_with_path(&[bin.path()]);
        let map = adapter_for_language(&adapters);
        assert_eq!(map.get("rust").unwrap().name, "lldb-vscode");
    }

    #[test]
    fn t131disc_skips_directory_with_same_name_as_binary() {
        let bin = FakeBinDir::new();
        std::fs::create_dir(bin.path().join("lldb-vscode")).unwrap();
        let adapters = discover_with_path(&[bin.path()]);
        assert!(adapters.is_empty());
    }

    #[test]
    fn t131disc_discover_real_path_does_not_panic() {
        let _ = discover();
    }
}
