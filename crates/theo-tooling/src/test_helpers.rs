use std::path::Path;
use tempfile::TempDir;
use theo_domain::permission::{PermissionRequest, PermissionType};
use theo_domain::tool::{PermissionCollector, ToolContext};

/// Create a temporary directory with optional git initialization
pub struct TestDir {
    pub dir: TempDir,
}

impl TestDir {
    pub fn new() -> Self {
        Self {
            dir: TempDir::new().expect("Failed to create temp dir"),
        }
    }

    pub fn with_git() -> Self {
        let td = Self::new();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(td.path())
            .output()
            .expect("Failed to git init");
        std::process::Command::new("git")
            .args(["config", "user.email", "test@opencode.test"])
            .current_dir(td.path())
            .output()
            .ok();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(td.path())
            .output()
            .ok();
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-m", "root commit"])
            .current_dir(td.path())
            .output()
            .ok();
        td
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn write_file(&self, relative_path: &str, content: &str) {
        let full_path = self.path().join(relative_path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).expect("Failed to create parent directories");
        }
        std::fs::write(&full_path, content).expect("Failed to write file");
    }

    pub fn read_file(&self, relative_path: &str) -> String {
        let full_path = self.path().join(relative_path);
        std::fs::read_to_string(&full_path).expect("Failed to read file")
    }

    pub fn file_exists(&self, relative_path: &str) -> bool {
        self.path().join(relative_path).exists()
    }
}

pub fn test_context(project_dir: &Path) -> ToolContext {
    ToolContext::test_context(project_dir.to_path_buf())
}

pub fn find_permission<'a>(
    collector: &'a PermissionCollector,
    ptype: &PermissionType,
) -> Option<&'a PermissionRequest> {
    collector.find_by_type(ptype)
}
