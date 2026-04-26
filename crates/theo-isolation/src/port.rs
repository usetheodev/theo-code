//! Deterministic port allocation per worktree path.
//!
//! Archon pattern (CLAUDE.md "Port Allocation"):
//! - Range: 3190-4089 (900 ports)
//! - port = 3190 + (sha256(worktree_path) % 900)
//! - Same worktree always gets same port (idempotent for testing)

use sha2::{Digest, Sha256};

/// Lower bound of the auto-allocated port range (inclusive).
pub const PORT_BASE: u16 = 3190;
/// Width of the port range.
pub const PORT_WIDTH: u16 = 900;

/// Deterministically allocate a port for a worktree path.
/// Same path → same port; different paths → likely different ports.
pub fn allocate_port(worktree_path: &str) -> u16 {
    let mut hasher = Sha256::new();
    hasher.update(worktree_path.as_bytes());
    let bytes = hasher.finalize();
    // Use first 4 bytes as u32, mod into the range
    let n = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    PORT_BASE + (n % PORT_WIDTH as u32) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_port_in_range() {
        let p = allocate_port("/some/path");
        assert!(p >= PORT_BASE);
        assert!(p < PORT_BASE + PORT_WIDTH);
    }

    #[test]
    fn allocate_port_deterministic_per_path() {
        assert_eq!(allocate_port("/a"), allocate_port("/a"));
        assert_eq!(allocate_port("/foo/bar"), allocate_port("/foo/bar"));
    }

    #[test]
    fn allocate_port_differs_per_path() {
        // Stochastic — could collide but probability ~1/900
        let mut seen = std::collections::HashSet::new();
        for i in 0..50 {
            seen.insert(allocate_port(&format!("/path/{}", i)));
        }
        // Expect at least 30 distinct ports out of 50 (very likely)
        assert!(
            seen.len() > 30,
            "expected diverse port allocation, got {} distinct ports",
            seen.len()
        );
    }

    #[test]
    fn port_base_and_width_match_archon_convention() {
        assert_eq!(PORT_BASE, 3190);
        assert_eq!(PORT_WIDTH, 900);
    }
}
