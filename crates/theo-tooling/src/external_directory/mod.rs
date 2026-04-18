use std::path::Path;
use theo_domain::permission::{PermissionRequest, PermissionType};
use theo_domain::tool::PermissionCollector;

#[derive(Debug, Default)]
pub struct ExternalDirectoryOptions {
    pub kind: Option<PathKind>,
    pub bypass: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathKind {
    File,
    Directory,
}

/// Assert that a path is within the project directory, or record an external_directory
/// permission request if it's outside.
pub fn assert_external_directory(
    permissions: &mut PermissionCollector,
    project_dir: &Path,
    target: Option<&Path>,
    options: Option<ExternalDirectoryOptions>,
) {
    let opts = options.unwrap_or_default();

    if opts.bypass {
        return;
    }

    let target = match target {
        Some(t) => t,
        None => return,
    };

    if target.starts_with(project_dir) {
        return;
    }

    let dir = if opts.kind == Some(PathKind::Directory) {
        target.to_path_buf()
    } else {
        target.parent().unwrap_or(target).to_path_buf()
    };

    let pattern = format!("{}/*", dir.display()).replace('\\', "/");

    permissions.record(PermissionRequest {
        permission: PermissionType::ExternalDirectory,
        patterns: vec![pattern.clone()],
        always: vec![pattern],
        metadata: serde_json::json!({}),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_perms() -> PermissionCollector {
        PermissionCollector::new()
    }

    #[test]
    fn no_ops_for_empty_target() {
        let mut perms = new_perms();
        assert_external_directory(&mut perms, Path::new("/tmp"), None, None);
        assert_eq!(perms.requests.len(), 0);
    }

    #[test]
    fn no_ops_for_paths_inside_project() {
        let mut perms = new_perms();
        let project = Path::new("/tmp/project");
        let target = project.join("file.txt");
        assert_external_directory(&mut perms, project, Some(&target), None);
        assert_eq!(perms.requests.len(), 0);
    }

    #[test]
    fn asks_with_single_canonical_glob() {
        let mut perms = new_perms();
        let project = Path::new("/tmp/project");
        let target = Path::new("/tmp/outside/file.txt");
        let expected = "/tmp/outside/*";

        assert_external_directory(&mut perms, project, Some(target), None);

        let req = perms.find_by_type(&PermissionType::ExternalDirectory);
        assert!(req.is_some());
        assert_eq!(req.unwrap().patterns, vec![expected]);
        assert_eq!(req.unwrap().always, vec![expected]);
    }

    #[test]
    fn uses_target_directory_when_kind_is_directory() {
        let mut perms = new_perms();
        let project = Path::new("/tmp/project");
        let target = Path::new("/tmp/outside");
        let expected = "/tmp/outside/*";

        assert_external_directory(
            &mut perms,
            project,
            Some(target),
            Some(ExternalDirectoryOptions {
                kind: Some(PathKind::Directory),
                bypass: false,
            }),
        );

        let req = perms.find_by_type(&PermissionType::ExternalDirectory);
        assert!(req.is_some());
        assert_eq!(req.unwrap().patterns, vec![expected]);
        assert_eq!(req.unwrap().always, vec![expected]);
    }

    #[test]
    fn skips_prompting_when_bypass_true() {
        let mut perms = new_perms();
        assert_external_directory(
            &mut perms,
            Path::new("/tmp/project"),
            Some(Path::new("/tmp/outside/file.txt")),
            Some(ExternalDirectoryOptions {
                kind: None,
                bypass: true,
            }),
        );
        assert_eq!(perms.requests.len(), 0);
    }
}
