# 06 — Tooling & Sandbox (`theo-tooling`)

Every tool the agent can use, the sandbox that constrains bash execution, and the tool registry that dispatches calls. The crate currently ships **~28 built-in tools** (each under its own module in `crates/theo-tooling/src/`: `read/`, `write/`, `edit/`, `multiedit/`, `apply_patch/`, `ls/`, `grep/`, `glob/`, `codesearch/`, `codebase_context/`, `bash/`, `git/`, `http_client/` (exposes `http_get` + `http_post`), `webfetch/`, `websearch/`, `think/`, `reflect/`, `memory/`, `task/`, `todo/`, `skill/`, `question/`, `plan/`, `env_info/`, `invalid/`, `lsp/`, `wiki_tool/`, `batch/`, `external_directory/`) plus the plugin mechanism in `shell_tool.rs`.

From a harness perspective, tools are not "capabilities the agent has" — they are the **legibility surface** of the environment. Each tool exposes one slice of the project, the runtime, or a feedback channel in a form the agent can repeatedly inspect and act on. A missing tool is an invisible part of the environment.

### Tool Taxonomy (guide vs sensor)

| Category | Direction | Execution | Examples |
|---|---|---|---|
| File/search read | Guide (feedforward) | Computational | `read`, `grep`, `glob`, `ls`, `codesearch`, `codebase_context` |
| File write | Mutation + triggers sensors | Computational | `write`, `edit`, `multiedit`, `apply_patch` |
| Execution | Mutation + triggers sensors | Computational | `bash` (sandboxed) |
| Verification | Sensor (feedback) | Computational | `bash` running tests / linters, `lsp` diagnostics |
| Knowledge | Guide | Computational | `memory`, `wiki_tool`, `env_info`, `git` (read ops) |
| Cognitive | Internal (no external effect) | Inferential | `think`, `reflect` |
| Orchestration | Meta | — | `task`, `todo`, `question`, `plan_exit` |
| Packaged skills (bifunctional) | Guide *or* Sensor, depending on payload | Inferential | `skill` — see note below |
| Web | Guide (external knowledge) | Computational | `http_get`, `http_post`, `webfetch`, `websearch` |

Write tools automatically trigger the post-edit sensor hook (`.theo/hooks/edit.verify.sh`) — every mutation has a paired feedback channel. This is the **"no blind edits"** invariant.

> **Skills are bifunctional.** Böckeler `§Examples` shows "coding conventions" (a **feedforward guide**) and "instructions how to review" (a **feedback sensor**) both implemented as skills. A skill that front-loads conventions before an edit acts as a guide; a skill invoked after a change to score it acts as a sensor. The tool dispatch is the same; the category depends on *when* in the loop the skill is triggered. This is the only tool in the registry whose category is not fixed.

### LLM-Friendly Error Messages

From `docs/pesquisas/harness-engineering-openai.md`: OpenAI's custom linters write error messages that *"inject remediation instructions into agent context"* — a deliberate form of positive prompt injection. Theo's tool errors follow the same pattern: every failure returns structured output with (1) what failed, (2) why, (3) what the agent should do next. Examples:

- `edit` — when `old_string` is ambiguous, the error includes surrounding context and the suggested fix: disambiguate with more lines.
- `bash` — when sandbox blocks a command, the violation reason is included and alternative approaches suggested.
- `write` — when parent dir is missing, the tool auto-creates rather than failing.

Cheap errors that the agent can parse and recover from turn a feedback loop from one-shot to iterative.

Dependencies: `theo-domain`, `tokio`, `serde`, `regex`, `reqwest`, `similar`, `walkdir`, `ignore`, `glob`, `landlock`, `libc`.

## Tool Registry

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn register(&mut self, tool: Box<dyn Tool>);
    pub fn get(&self, id: &str) -> Option<&dyn Tool>;
    pub fn definitions(&self) -> Vec<ToolDefinition>;
}

pub fn create_default_registry() -> ToolRegistry;  // registers all ~28 built-in tools
pub fn register_plugin_tools(registry, plugins);   // adds shell-script plugin tools
```

## Built-in Tools

### File Operations

| Tool | Purpose | Key Behavior |
|---|---|---|
| `read` | Read file contents | Line range support, binary detection, 2000-line default limit |
| `write` | Create/overwrite file | Creates parent dirs, UTF-8 validated |
| `edit` | String replacement in file | `old_string` must be unique match; supports `replace_all` |
| `multiedit` | Multiple edits in one call | Atomic: all-or-nothing application |
| `apply_patch` | Unified diff patch | Fuzzy matching for context lines |
| `ls` | List directory | Depth-limited, respects `.gitignore` |

### Search

| Tool | Purpose | Key Behavior |
|---|---|---|
| `grep` | Content search (ripgrep-style) | Regex, glob filter, context lines, output modes |
| `glob` | File pattern matching | Returns paths sorted by modification time |
| `codesearch` | Semantic code search | Combines grep + symbol-aware filtering |
| `codebase_context` | GRAPHCTX query | Invokes `GraphContextProvider::query_context()` |

The search stack is intentionally redundant: cheap lexical search, structural retrieval, and semantic helpers exist together so the runtime can trade cost, precision, and recall per turn instead of depending on a single retrieval mechanism.

### Execution

| Tool | Purpose | Key Behavior |
|---|---|---|
| `bash` | Shell command execution | **Sandboxed** (bwrap > landlock > noop cascade), timeout, output truncation |

### Web

| Tool | Purpose | Key Behavior |
|---|---|---|
| `http_get` / `http_post` | HTTP client | URL validation, response truncation |
| `webfetch` | Fetch web page content | HTML → text extraction |
| `websearch` | Web search | Provider-agnostic search API |

### Cognitive

| Tool | Purpose | Key Behavior |
|---|---|---|
| `think` | Extended reasoning | No-op tool — output goes to context, agent sees its own thinking |
| `reflect` | Self-reflection | Similar to think, positioned for post-action analysis |
| `memory` | Cross-session memory | Read/write to `~/.config/theo/memory/` |

### Orchestration

| Tool | Purpose | Key Behavior |
|---|---|---|
| `task` | Task management | Create/update/list tasks with status tracking |
| `todo` | `task_create` / `task_update` | Alias for task management |
| `skill` | Invoke packaged skill | Loads skill definition, executes InContext or as SubAgent |
| `question` | Ask user a question | Blocks until user responds |
| `plan_exit` | Exit plan mode | Transitions from Plan → Agent mode |

### Utility

| Tool | Purpose |
|---|---|
| `env_info` | System environment information |
| `git` | Git operations (status, diff, log, commit) |
| `invalid` | Error response for malformed tool calls |
| `lsp` | Language Server Protocol queries |
| `wiki_tool` | Wiki query/generate/ingest |

## Sandbox Architecture

Defense-in-depth for `bash` tool execution. Three-tier cascade:

```
┌─────────────────────────────────┐
│  bubblewrap (bwrap)             │  Linux namespaces (user, mount, net, pid)
│  Strongest isolation            │  Mount overlays, /proc filtering
│  Requires: bwrap binary         │  Network namespace isolation
├─────────────────────────────────┤
│  landlock                       │  Linux Security Module (5.13+)
│  Filesystem access control      │  No root/caps needed
│  Requires: kernel support       │  Fine-grained path rules
├─────────────────────────────────┤
│  macOS sandbox-exec             │  macOS sandbox profiles
│  Process-level sandboxing       │  Filesystem + network rules
├─────────────────────────────────┤
│  noop (fallback)                │  No isolation
│  Used when nothing available    │  Logs warning
└─────────────────────────────────┘
```

`create_executor()` probes system capabilities at startup and selects the strongest available.

This is a **guide** in the harness-engineering sense: it shapes what the agent can do before a risky action happens, instead of depending only on after-the-fact review.

### Sandbox Modules

| Module | Purpose |
|---|---|
| `bwrap.rs` | bubblewrap namespace executor |
| `command_validator.rs` | Command allowlist/denylist validation |
| `denied_paths.rs` | Path denylist (`/etc/passwd`, `~/.ssh`, `~/.aws/credentials`) |
| `env_sanitizer.rs` | Strip sensitive env vars (`AWS_*`, `GITHUB_TOKEN`, etc.) |
| `executor.rs` | `create_executor()` — probe + cascade |
| `macos.rs` | macOS sandbox-exec profile generation |
| `network.rs` | Network namespace isolation |
| `probe.rs` | Runtime capability detection |
| `rlimits.rs` | `rlimit` resource limits (process count, memory, CPU) |

### Command Validation

`command_validator` applies a denylist of dangerous patterns before execution:
- `rm -rf /`, `mkfs`, `dd if=`, `:(){:|:&};:`
- `chmod 777`, `chown root`
- Sensitive file access patterns

### Edit Tool — Diff Algorithm

The `edit` tool uses the `similar` crate for string matching. `old_string` must match exactly one location in the file. If ambiguous, returns an error with context to help the LLM disambiguate. `prepare_arguments()` normalizes `filePath` → `file_path` for backwards compatibility.

### Plugin System (shell_tool.rs)

`ShellTool` wraps any shell script as a tool. Plugin manifest in TOML:

```toml
[tool]
name = "my_tool"
description = "Does something"
command = "./scripts/my_tool.sh"

[[tool.params]]
name = "input"
type = "string"
description = "Input data"
required = true
```

Discovered from `.theo/plugins/` and `~/.config/theo/plugins/`.

Plugins are the extension point where project-specific harnesses get built. A team that wants a domain-specific sensor (custom linter, structural test, mutation test) wraps it as a plugin tool, and the agent picks it up automatically. This is the same mechanism OpenAI uses for their custom-linter / structural-test enforcement — just exposed as a tool rather than a CI hook.
