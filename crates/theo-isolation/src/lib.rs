//! `theo-isolation` — sub-agent isolation primitives (worktree, port allocation, safety rules).
//!
//! Track D — Phase 11.
//!
//! Provides:
//! - `WorktreeProvider`: wrapper around `git worktree` for per-agent CWD isolation
//! - Port auto-allocation: deterministic hash-based port assignment per worktree
//! - `safety_rules()`: Pi-Mono-aligned text injected into sub-agent system prompts
//!   when worktree mode is active
//!
//! Reference:
//! - `referencias/Archon/packages/isolation/src/providers/worktree.ts`
//! - `referencias/pi-mono/AGENTS.md:194-233` (parallel-agent git safety rules)

pub mod port;
pub mod safety;
pub mod worktree;

pub use port::allocate_port;
pub use safety::{safety_rules, IsolationMode};
pub use worktree::{IsolationError, WorktreeHandle, WorktreeProvider};
