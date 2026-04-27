//! Runtime-only persistence for plan findings (Manus principle: filesystem
//! as memory). Stays in `theo-agent-runtime` per meeting decision D7 — these
//! types are *not* part of the schema-validated plan model and may evolve
//! independently of `Plan`.
//!
//! Layout: `<project>/.theo/plans/findings.json`.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Format version for `findings.json`. Bump on incompatible changes.
pub const PLAN_FINDINGS_VERSION: u32 = 1;

/// Aggregated findings file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanFindings {
    pub version: u32,
    #[serde(default)]
    pub requirements: Vec<String>,
    #[serde(default)]
    pub research: Vec<PlanFinding>,
    #[serde(default)]
    pub resources: Vec<PlanResource>,
}

impl Default for PlanFindings {
    fn default() -> Self {
        Self {
            version: PLAN_FINDINGS_VERSION,
            requirements: Vec::new(),
            research: Vec::new(),
            resources: Vec::new(),
        }
    }
}

/// A single research note attached to a plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanFinding {
    pub summary: String,
    pub source: String,
    pub timestamp: u64,
}

/// External resource referenced by the plan (URL + title).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanResource {
    pub title: String,
    pub url: String,
}

/// Errors specific to findings I/O.
#[derive(Debug, thiserror::Error)]
pub enum PlanFindingsError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid findings format: {0}")]
    InvalidFormat(String),
    #[error("unsupported findings version: found {found}, max supported {max_supported}")]
    UnsupportedVersion { found: u32, max_supported: u32 },
}

/// Loads findings from disk; returns `Default` when the file is missing.
pub fn load_findings(path: &Path) -> Result<PlanFindings, PlanFindingsError> {
    if !path.exists() {
        return Ok(PlanFindings::default());
    }
    let content = std::fs::read_to_string(path)?;
    let findings: PlanFindings = serde_json::from_str(&content)
        .map_err(|e| PlanFindingsError::InvalidFormat(e.to_string()))?;
    if findings.version > PLAN_FINDINGS_VERSION {
        return Err(PlanFindingsError::UnsupportedVersion {
            found: findings.version,
            max_supported: PLAN_FINDINGS_VERSION,
        });
    }
    Ok(findings)
}

/// Saves findings atomically (write temp + rename).
pub fn save_findings(path: &Path, findings: &PlanFindings) -> Result<(), PlanFindingsError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(findings)
        .map_err(|e| PlanFindingsError::InvalidFormat(e.to_string()))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json.as_bytes())?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn findings_default_has_current_version() {
        let f = PlanFindings::default();
        assert_eq!(f.version, PLAN_FINDINGS_VERSION);
        assert!(f.requirements.is_empty());
    }

    #[test]
    fn findings_serde_roundtrip() {
        let f = PlanFindings {
            version: PLAN_FINDINGS_VERSION,
            requirements: vec!["Must support JSON".into()],
            research: vec![PlanFinding {
                summary: "Manus uses filesystem".into(),
                source: "manus.im".into(),
                timestamp: 42,
            }],
            resources: vec![PlanResource {
                title: "ADR-016".into(),
                url: "https://example.com/adr/016".into(),
            }],
        };
        let json = serde_json::to_string_pretty(&f).unwrap();
        let back: PlanFindings = serde_json::from_str(&json).unwrap();
        assert_eq!(f, back);
    }

    #[test]
    fn findings_load_returns_default_when_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.json");
        let f = load_findings(&path).unwrap();
        assert_eq!(f, PlanFindings::default());
    }

    #[test]
    fn findings_save_load_roundtrip_through_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("findings.json");
        let f = PlanFindings {
            version: PLAN_FINDINGS_VERSION,
            requirements: vec!["a".into()],
            research: vec![],
            resources: vec![],
        };
        save_findings(&path, &f).unwrap();
        let loaded = load_findings(&path).unwrap();
        assert_eq!(loaded, f);
    }

    #[test]
    fn findings_load_rejects_future_version() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("findings.json");
        let f = PlanFindings {
            version: 999,
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&f).unwrap()).unwrap();
        let err = load_findings(&path).unwrap_err();
        assert!(matches!(err, PlanFindingsError::UnsupportedVersion { .. }));
    }

    #[test]
    fn findings_load_rejects_invalid_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("findings.json");
        std::fs::write(&path, "{{ broken").unwrap();
        let err = load_findings(&path).unwrap_err();
        assert!(matches!(err, PlanFindingsError::InvalidFormat(_)));
    }

    #[test]
    fn findings_save_creates_missing_parent_dir() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("findings.json");
        let f = PlanFindings::default();
        save_findings(&path, &f).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn findings_optional_fields_default() {
        let json = r#"{"version": 1}"#;
        let f: PlanFindings = serde_json::from_str(json).unwrap();
        assert!(f.requirements.is_empty());
        assert!(f.research.is_empty());
        assert!(f.resources.is_empty());
    }
}
