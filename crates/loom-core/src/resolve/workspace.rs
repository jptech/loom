use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::LoomError;
use crate::manifest::{
    load_component_manifest, load_project_manifest, load_workspace_manifest, ComponentManifest,
    ProjectManifest, WorkspaceManifest,
};
use crate::util::clean_path;

pub struct DiscoveredWorkspace {
    pub root: PathBuf,
    pub manifest: WorkspaceManifest,
    pub member_paths: Vec<MemberPath>,
}

pub struct MemberPath {
    pub path: PathBuf,
    pub kind: MemberKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MemberKind {
    Component,
    Project,
    Platform,
    Unknown,
}

/// Walk up from `start` to find a directory containing `workspace.toml`.
pub fn find_workspace_root(start: &Path) -> Result<(PathBuf, WorkspaceManifest), LoomError> {
    let start = clean_path(start.canonicalize().map_err(|e| LoomError::Io {
        path: start.to_owned(),
        source: e,
    })?);

    let mut current = start.as_path();
    loop {
        let candidate = current.join("workspace.toml");
        if candidate.exists() {
            let manifest = load_workspace_manifest(&candidate)?;
            return Ok((current.to_owned(), manifest));
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => {
                return Err(LoomError::NoWorkspace { start });
            }
        }
    }
}

/// Expand workspace member globs relative to `workspace_root`.
pub fn discover_members(
    workspace_root: &Path,
    manifest: &WorkspaceManifest,
) -> Result<Vec<MemberPath>, LoomError> {
    let mut members = Vec::new();
    let mut seen = HashSet::new();

    for glob_pattern in &manifest.workspace.members {
        let abs_pattern = workspace_root
            .join(glob_pattern)
            .to_string_lossy()
            .to_string();

        let paths = glob::glob(&abs_pattern).map_err(|e| LoomError::GlobPattern {
            pattern: glob_pattern.clone(),
            message: e.to_string(),
        })?;

        for entry in paths {
            let path = entry.map_err(|e| LoomError::GlobError {
                message: e.to_string(),
            })?;

            if !path.is_dir() {
                continue;
            }

            let canonical = clean_path(path.canonicalize().map_err(|e| LoomError::Io {
                path: path.clone(),
                source: e,
            })?);
            if !seen.insert(canonical.clone()) {
                continue;
            }

            let kind = classify_member(&canonical);
            members.push(MemberPath {
                path: canonical,
                kind,
            });
        }
    }

    Ok(members)
}

fn classify_member(path: &Path) -> MemberKind {
    if path.join("component.toml").exists() {
        MemberKind::Component
    } else if path.join("project.toml").exists() {
        MemberKind::Project
    } else if path.join("platform.toml").exists() {
        MemberKind::Platform
    } else {
        MemberKind::Unknown
    }
}

/// Load all component manifests from the workspace members.
pub fn load_all_components(
    members: &[MemberPath],
) -> Result<Vec<(PathBuf, ComponentManifest)>, LoomError> {
    members
        .iter()
        .filter(|m| m.kind == MemberKind::Component)
        .map(|m| {
            let manifest_path = m.path.join("component.toml");
            let manifest = load_component_manifest(&manifest_path)?;
            Ok((m.path.clone(), manifest))
        })
        .collect()
}

/// Find a project by name. If `project_name` is None, returns the only project
/// (errors if zero or multiple projects exist).
pub fn find_project(
    members: &[MemberPath],
    project_name: Option<&str>,
) -> Result<(PathBuf, ProjectManifest), LoomError> {
    let project_members: Vec<_> = members
        .iter()
        .filter(|m| m.kind == MemberKind::Project)
        .collect();

    match project_name {
        Some(name) => {
            for member in &project_members {
                let manifest_path = member.path.join("project.toml");
                let manifest = load_project_manifest(&manifest_path)?;
                if manifest.project.name == name {
                    return Ok((member.path.clone(), manifest));
                }
            }
            Err(LoomError::ProjectNotFound {
                name: name.to_owned(),
            })
        }
        None => {
            if project_members.len() == 1 {
                let member = project_members[0];
                let manifest_path = member.path.join("project.toml");
                let manifest = load_project_manifest(&manifest_path)?;
                Ok((member.path.clone(), manifest))
            } else if project_members.is_empty() {
                Err(LoomError::ProjectNotFound {
                    name: "<any>".to_owned(),
                })
            } else {
                Err(LoomError::Internal(format!(
                    "Multiple projects found in workspace. Specify one with --project. Found: {:?}",
                    project_members
                        .iter()
                        .map(|m| m.path.display().to_string())
                        .collect::<Vec<_>>()
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_path(name: &str) -> PathBuf {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        manifest_dir.join("../../tests/fixtures").join(name)
    }

    #[test]
    fn test_find_workspace_root() {
        let fixture = fixture_path("simple_project");
        let start = fixture.join("projects/my_design");
        let (root, manifest) = find_workspace_root(&start).unwrap();
        assert_eq!(
            root.canonicalize().unwrap(),
            fixture.canonicalize().unwrap()
        );
        assert_eq!(manifest.workspace.name, "test_workspace");
    }

    #[test]
    fn test_discover_members() {
        let fixture = fixture_path("simple_project");
        let (root, manifest) = find_workspace_root(&fixture).unwrap();
        let members = discover_members(&root, &manifest).unwrap();

        let component_count = members
            .iter()
            .filter(|m| m.kind == MemberKind::Component)
            .count();
        let project_count = members
            .iter()
            .filter(|m| m.kind == MemberKind::Project)
            .count();

        assert_eq!(component_count, 1);
        assert_eq!(project_count, 1);
    }

    #[test]
    fn test_find_project_by_name() {
        let fixture = fixture_path("simple_project");
        let (root, manifest) = find_workspace_root(&fixture).unwrap();
        let members = discover_members(&root, &manifest).unwrap();
        let (_, project) = find_project(&members, Some("my_design")).unwrap();
        assert_eq!(project.project.name, "my_design");
    }

    #[test]
    fn test_find_project_auto() {
        let fixture = fixture_path("simple_project");
        let (root, manifest) = find_workspace_root(&fixture).unwrap();
        let members = discover_members(&root, &manifest).unwrap();
        let (_, project) = find_project(&members, None).unwrap();
        assert_eq!(project.project.name, "my_design");
    }

    #[test]
    fn test_load_all_components() {
        let fixture = fixture_path("simple_project");
        let (root, manifest) = find_workspace_root(&fixture).unwrap();
        let members = discover_members(&root, &manifest).unwrap();
        let components = load_all_components(&members).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].1.component.name, "testorg/axi_common");
    }

    #[test]
    fn test_multi_component_discovery() {
        let fixture = fixture_path("multi_component");
        let (root, manifest) = find_workspace_root(&fixture).unwrap();
        let members = discover_members(&root, &manifest).unwrap();
        let components = load_all_components(&members).unwrap();
        assert_eq!(components.len(), 2);
    }
}
