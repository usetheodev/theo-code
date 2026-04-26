//! T3.1 — LSP server discovery.
//!
//! Probes the system PATH for known LSP servers and returns a list
//! of `DiscoveredServer` entries the caller can hand to
//! `LspClient::spawn`. Each entry maps a set of file extensions
//! (`.rs`, `.py`, `.ts`) to the program + args that drive that
//! language's server.
//!
//! Pure (no async, no tokio) so it's trivially testable with a
//! controlled PATH. The actual spawn happens in `LspClient`.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// One discovered LSP server entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredServer {
    /// Short identifier (`"rust-analyzer"`, `"pyright"`).
    pub name: &'static str,
    /// Absolute path to the binary.
    pub command: PathBuf,
    /// Args the server expects on its command line (`["--stdio"]` for
    /// pyright, empty for rust-analyzer).
    pub args: Vec<&'static str>,
    /// File extensions this server handles (without the dot).
    pub file_extensions: &'static [&'static str],
    /// Languages this server handles (lower-case, ASCII).
    pub languages: &'static [&'static str],
}

/// The catalogue of well-known LSP servers we know how to invoke.
/// Ordering reflects rough preference within a language (e.g. pyright
/// before pylsp because pyright is faster + better-tested in the
/// SOTA assistant tier).
struct ServerSpec {
    name: &'static str,
    binary: &'static str,
    args: &'static [&'static str],
    file_extensions: &'static [&'static str],
    languages: &'static [&'static str],
}

const KNOWN_SERVERS: &[ServerSpec] = &[
    ServerSpec {
        name: "rust-analyzer",
        binary: "rust-analyzer",
        args: &[],
        file_extensions: &["rs"],
        languages: &["rust"],
    },
    ServerSpec {
        name: "pyright",
        binary: "pyright-langserver",
        args: &["--stdio"],
        file_extensions: &["py", "pyi"],
        languages: &["python"],
    },
    ServerSpec {
        name: "pylsp",
        binary: "pylsp",
        args: &[],
        file_extensions: &["py", "pyi"],
        languages: &["python"],
    },
    ServerSpec {
        name: "typescript-language-server",
        binary: "typescript-language-server",
        args: &["--stdio"],
        file_extensions: &["ts", "tsx", "js", "jsx", "mjs", "cjs"],
        languages: &["typescript", "javascript"],
    },
    ServerSpec {
        name: "gopls",
        binary: "gopls",
        args: &[],
        file_extensions: &["go"],
        languages: &["go"],
    },
    ServerSpec {
        name: "clangd",
        binary: "clangd",
        args: &[],
        file_extensions: &["c", "cc", "cpp", "cxx", "h", "hh", "hpp", "hxx"],
        languages: &["c", "cpp"],
    },
    ServerSpec {
        name: "jdtls",
        binary: "jdtls",
        args: &[],
        file_extensions: &["java"],
        languages: &["java"],
    },
    ServerSpec {
        name: "lua-language-server",
        binary: "lua-language-server",
        args: &[],
        file_extensions: &["lua"],
        languages: &["lua"],
    },
    ServerSpec {
        name: "kotlin-language-server",
        binary: "kotlin-language-server",
        args: &[],
        file_extensions: &["kt", "kts"],
        languages: &["kotlin"],
    },
    ServerSpec {
        name: "ruby-lsp",
        binary: "ruby-lsp",
        args: &[],
        file_extensions: &["rb"],
        languages: &["ruby"],
    },
];

/// Probe the supplied PATH directories for each known LSP server.
/// Returns one entry per server whose binary is executable. Order
/// mirrors `KNOWN_SERVERS`. Pure: takes `PATH` as a param so tests
/// can drive it without mutating `std::env`.
pub fn discover_with_path(path_dirs: &[&Path]) -> Vec<DiscoveredServer> {
    let mut found = Vec::with_capacity(KNOWN_SERVERS.len());
    for spec in KNOWN_SERVERS {
        if let Some(command) = locate_executable(spec.binary, path_dirs) {
            found.push(DiscoveredServer {
                name: spec.name,
                command,
                args: spec.args.to_vec(),
                file_extensions: spec.file_extensions,
                languages: spec.languages,
            });
        }
    }
    found
}

/// Convenience: probe the current process `PATH` env var.
pub fn discover() -> Vec<DiscoveredServer> {
    let path = env::var_os("PATH").unwrap_or_default();
    let dirs: Vec<PathBuf> = env::split_paths(&path).collect();
    let refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
    discover_with_path(&refs)
}

/// Build an extension → server lookup. When two servers claim the
/// same extension (pyright + pylsp), the first one in `servers`
/// wins (since `KNOWN_SERVERS` ordering reflects preference).
pub fn server_for_extension(
    servers: &[DiscoveredServer],
) -> HashMap<&'static str, DiscoveredServer> {
    let mut map = HashMap::new();
    for srv in servers {
        for ext in srv.file_extensions {
            map.entry(*ext).or_insert_with(|| srv.clone());
        }
    }
    map
}

/// Look up a discovered server by language name (lower-case ASCII).
pub fn server_for_language<'a>(
    servers: &'a [DiscoveredServer],
    language: &str,
) -> Option<&'a DiscoveredServer> {
    let needle = language.to_ascii_lowercase();
    servers
        .iter()
        .find(|s| s.languages.contains(&needle.as_str()))
}

/// Locate `binary` inside any of `dirs`. Returns the first absolute
/// path that exists and is regular (not a directory). Cross-platform
/// note: on Linux/macOS we don't append `.exe`. Callers that need
/// Windows support should pass `binary` with the right extension.
fn locate_executable(binary: &str, dirs: &[&Path]) -> Option<PathBuf> {
    for dir in dirs {
        let candidate = dir.join(binary);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Returns true when the path exists, is a regular file, and (on
/// Unix) has at least one execute bit set.
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

/// Helper for test ergonomics: build a temp dir and seed it with
/// fake executables. Returned `_guard` keeps the dir alive.
#[cfg(test)]
mod test_helpers {
    use super::*;
    use std::io::Write;

    pub struct FakeBinDir {
        pub dir: tempfile::TempDir,
    }

    impl FakeBinDir {
        pub fn new() -> Self {
            Self {
                dir: tempfile::tempdir().expect("tempdir"),
            }
        }

        /// Drop a fake executable script that exits 0. Marks it
        /// executable on Unix.
        pub fn add(&self, name: &str) -> PathBuf {
            let path = self.dir.path().join(name);
            let mut f = fs::File::create(&path).expect("create fake bin");
            writeln!(f, "#!/bin/sh\nexit 0").expect("write fake bin");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&path).expect("meta").permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&path, perms).expect("chmod");
            }
            path
        }

        /// Path that callers pass to `discover_with_path`.
        pub fn path(&self) -> &Path {
            self.dir.path()
        }
    }

    /// Add a non-executable file that shouldn't be discovered.
    pub fn add_non_executable(dir: &FakeBinDir, name: &str) -> PathBuf {
        let path = dir.path().join(name);
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "not a script").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o644); // no execute bit
            fs::set_permissions(&path, perms).unwrap();
        }
        path
    }
}

#[cfg(test)]
mod tests {
    use super::test_helpers::*;
    use super::*;

    #[test]
    fn t31disc_empty_path_returns_no_servers() {
        let dirs: Vec<&Path> = Vec::new();
        let servers = discover_with_path(&dirs);
        assert!(servers.is_empty());
    }

    #[test]
    fn t31disc_path_with_no_known_binaries_returns_empty() {
        let bin = FakeBinDir::new();
        bin.add("totally-unrelated-binary");
        bin.add("another-random-tool");
        let dirs = [bin.path()];
        let servers = discover_with_path(&dirs);
        assert!(servers.is_empty());
    }

    #[test]
    fn t31disc_finds_rust_analyzer_when_present() {
        let bin = FakeBinDir::new();
        bin.add("rust-analyzer");
        let dirs = [bin.path()];
        let servers = discover_with_path(&dirs);
        assert_eq!(servers.len(), 1);
        let ra = &servers[0];
        assert_eq!(ra.name, "rust-analyzer");
        assert!(ra.file_extensions.contains(&"rs"));
        assert!(ra.languages.contains(&"rust"));
        assert!(ra.command.is_absolute());
    }

    #[test]
    fn t31disc_finds_pyright_with_correct_args() {
        let bin = FakeBinDir::new();
        bin.add("pyright-langserver");
        let dirs = [bin.path()];
        let servers = discover_with_path(&dirs);
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "pyright");
        // Pyright requires --stdio flag — must be in args.
        assert_eq!(servers[0].args, vec!["--stdio"]);
    }

    #[test]
    fn t31disc_finds_typescript_language_server_with_correct_args() {
        let bin = FakeBinDir::new();
        bin.add("typescript-language-server");
        let dirs = [bin.path()];
        let servers = discover_with_path(&dirs);
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "typescript-language-server");
        assert_eq!(servers[0].args, vec!["--stdio"]);
        // Covers .ts, .tsx, .js, .jsx, .mjs, .cjs.
        assert!(servers[0].file_extensions.contains(&"ts"));
        assert!(servers[0].file_extensions.contains(&"tsx"));
        assert!(servers[0].file_extensions.contains(&"js"));
    }

    #[test]
    fn t31disc_finds_multiple_servers_in_path() {
        let bin = FakeBinDir::new();
        bin.add("rust-analyzer");
        bin.add("gopls");
        bin.add("clangd");
        let dirs = [bin.path()];
        let servers = discover_with_path(&dirs);
        assert_eq!(servers.len(), 3);
        let names: Vec<_> = servers.iter().map(|s| s.name).collect();
        assert!(names.contains(&"rust-analyzer"));
        assert!(names.contains(&"gopls"));
        assert!(names.contains(&"clangd"));
    }

    #[test]
    fn t31disc_skips_non_executable_files() {
        // A file named `rust-analyzer` but without execute bit must
        // NOT be discovered — otherwise we'd try to spawn a text
        // file as a binary.
        let bin = FakeBinDir::new();
        add_non_executable(&bin, "rust-analyzer");
        let dirs = [bin.path()];
        let servers = discover_with_path(&dirs);
        assert!(
            servers.is_empty(),
            "non-executable rust-analyzer should not be discovered"
        );
    }

    #[test]
    fn t31disc_first_path_dir_wins_for_duplicate_binaries() {
        // If both dirs have `rust-analyzer`, the one earlier in
        // PATH must be returned (matches shell semantics).
        let bin1 = FakeBinDir::new();
        let bin2 = FakeBinDir::new();
        let _expected_path = bin1.add("rust-analyzer");
        let _shadowed_path = bin2.add("rust-analyzer");
        let dirs = [bin1.path(), bin2.path()];
        let servers = discover_with_path(&dirs);
        assert_eq!(servers.len(), 1);
        assert!(
            servers[0].command.starts_with(bin1.path()),
            "expected first dir to win, got {:?}",
            servers[0].command
        );
    }

    #[test]
    fn t31disc_server_for_extension_maps_each_known_extension() {
        let bin = FakeBinDir::new();
        bin.add("rust-analyzer");
        bin.add("gopls");
        let servers = discover_with_path(&[bin.path()]);
        let by_ext = server_for_extension(&servers);
        assert_eq!(by_ext.get("rs").unwrap().name, "rust-analyzer");
        assert_eq!(by_ext.get("go").unwrap().name, "gopls");
        // Unknown extension → None.
        assert!(by_ext.get("xyz").is_none());
    }

    #[test]
    fn t31disc_server_for_extension_first_server_wins_on_conflict() {
        // Both pyright and pylsp claim `.py`. Pyright is first in
        // KNOWN_SERVERS, so it wins — even when both are installed.
        let bin = FakeBinDir::new();
        bin.add("pyright-langserver");
        bin.add("pylsp");
        let servers = discover_with_path(&[bin.path()]);
        assert_eq!(servers.len(), 2);
        let by_ext = server_for_extension(&servers);
        assert_eq!(
            by_ext.get("py").unwrap().name,
            "pyright",
            "preference order: pyright > pylsp"
        );
    }

    #[test]
    fn t31disc_server_for_language_finds_by_language_name() {
        let bin = FakeBinDir::new();
        bin.add("rust-analyzer");
        bin.add("typescript-language-server");
        let servers = discover_with_path(&[bin.path()]);
        let rust = server_for_language(&servers, "rust");
        assert!(rust.is_some());
        assert_eq!(rust.unwrap().name, "rust-analyzer");
        let ts = server_for_language(&servers, "typescript");
        assert!(ts.is_some());
        assert_eq!(ts.unwrap().name, "typescript-language-server");
        let js = server_for_language(&servers, "javascript");
        assert!(js.is_some(), "ts-language-server also serves javascript");
    }

    #[test]
    fn t31disc_server_for_language_is_case_insensitive() {
        let bin = FakeBinDir::new();
        bin.add("rust-analyzer");
        let servers = discover_with_path(&[bin.path()]);
        assert!(server_for_language(&servers, "Rust").is_some());
        assert!(server_for_language(&servers, "RUST").is_some());
        assert!(server_for_language(&servers, "rust").is_some());
    }

    #[test]
    fn t31disc_server_for_language_returns_none_for_unknown() {
        let bin = FakeBinDir::new();
        bin.add("rust-analyzer");
        let servers = discover_with_path(&[bin.path()]);
        assert!(server_for_language(&servers, "haskell").is_none());
        assert!(server_for_language(&servers, "").is_none());
    }

    #[test]
    fn t31disc_discover_uses_real_path_env_var() {
        // Smoke test: discover() should not panic even when PATH is
        // weird. We don't assert what's found because CI varies.
        let _servers = discover();
    }

    #[test]
    fn t31disc_locate_executable_returns_none_for_directory() {
        // A directory named "rust-analyzer" (uncommon but possible)
        // must not be returned as a binary.
        let bin = FakeBinDir::new();
        let dir = bin.path().join("rust-analyzer");
        std::fs::create_dir(&dir).unwrap();
        let dirs = [bin.path()];
        let servers = discover_with_path(&dirs);
        assert!(servers.is_empty());
    }
}

