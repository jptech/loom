# Task 04: Workspace Discovery

**Prerequisites:** Task 03 complete
**Goal:** Given a starting path, find the workspace root (`workspace.toml`), discover all member paths via glob expansion, and categorize them as components/projects.

## Spec Reference
`system_plan.md` §11.1 (Workspace Layout), §11.2 (Workspace Manifest), §3.5 (Resolution Sources)

## File to Implement
`crates/loom-core/src/resolve/workspace.rs`

## Types

```rust
use std::path::{Path, PathBuf};
use crate::manifest::{WorkspaceManifest, ComponentManifest, ProjectManifest};
use crate::error::LoomError;

/// Discovered workspace with all member paths resolved
pub struct DiscoveredWorkspace {
    pub root: PathBuf,
    pub manifest: WorkspaceManifest,
    pub member_paths: Vec<MemberPath>,
}

/// A member discovered via glob expansion
pub struct MemberPath {
    pub path: PathBuf,              // absolute path to member directory
    pub kind: MemberKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MemberKind {
    Component,   // contains component.toml
    Project,     // contains project.toml
    Platform,    // contains platform.toml (Phase 3)
    Unknown,     // directory matched glob but no recognized manifest
}
```

## Functions to Implement

### 1. Find workspace root

```rust
/// Walk up from `start` to find a directory containing `workspace.toml`.
/// Returns the workspace root path and the loaded manifest.
/// Error if no workspace.toml found walking up to filesystem root.
pub fn find_workspace_root(start: &Path) -> Result<(PathBuf, WorkspaceManifest), LoomError> {
    let start = start.canonicalize()
        .map_err(|e| LoomError::Io { path: start.to_owned(), source: e })?;

    let mut current = start.as_path();
    loop {
        let candidate = current.join("workspace.toml");
        if candidate.exists() {
            let manifest = crate::manifest::load_workspace_manifest(&candidate)?;
            return Ok((current.to_owned(), manifest));
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return Err(LoomError::NoWorkspace { start: start }),
        }
    }
}
```

### 2. Discover all members

```rust
/// Expand workspace member globs relative to `workspace_root`.
/// Returns all matching directories classified by their manifest type.
pub fn discover_members(
    workspace_root: &Path,
    manifest: &WorkspaceManifest,
) -> Result<Vec<MemberPath>, LoomError> {
    let mut members = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for glob_pattern in &manifest.workspace.members {
        // Glob is relative to workspace root
        let abs_pattern = workspace_root
            .join(glob_pattern)
            .to_string_lossy()
            .to_string();

        let paths = glob::glob(&abs_pattern)
            .map_err(|e| LoomError::GlobPattern {
                pattern: glob_pattern.clone(),
                message: e.to_string(),
            })?;

        for entry in paths {
            let path = entry.map_err(|e| LoomError::GlobError {
                message: e.to_string(),
            })?;

            if !path.is_dir() { continue; }

            // Deduplicate
            let canonical = path.canonicalize()
                .map_err(|e| LoomError::Io { path: path.clone(), source: e })?;
            if !seen.insert(canonical.clone()) { continue; }

            let kind = classify_member(&canonical);
            members.push(MemberPath { path: canonical, kind });
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
```

### 3. Load all discovered components and projects

```rust
/// Load all component manifests from the workspace members.
pub fn load_all_components(
    members: &[MemberPath],
) -> Result<Vec<(PathBuf, ComponentManifest)>, LoomError> {
    members.iter()
        .filter(|m| m.kind == MemberKind::Component)
        .map(|m| {
            let manifest_path = m.path.join("component.toml");
            let manifest = crate::manifest::load_component_manifest(&manifest_path)?;
            Ok((m.path.clone(), manifest))
        })
        .collect()
}

/// Load a specific project manifest by name (matches project.name field).
pub fn find_project(
    members: &[MemberPath],
    project_name: &str,
) -> Result<(PathBuf, ProjectManifest), LoomError> {
    for member in members.iter().filter(|m| m.kind == MemberKind::Project) {
        let manifest_path = member.path.join("project.toml");
        let manifest = crate::manifest::load_project_manifest(&manifest_path)?;
        if manifest.project.name == project_name {
            return Ok((member.path.clone(), manifest));
        }
    }
    Err(LoomError::ProjectNotFound { name: project_name.to_owned() })
}
```

## Public API (exported from `resolve/mod.rs`)

```rust
pub mod workspace;
pub mod resolver;
pub mod lockfile;
pub mod graph;

pub use workspace::{
    find_workspace_root, discover_members, load_all_components, find_project,
    DiscoveredWorkspace, MemberPath, MemberKind,
};
```

## Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_path(name: &str) -> PathBuf {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        // Go up to workspace root, then into tests/fixtures
        manifest_dir.join("../../tests/fixtures").join(name)
    }

    #[test]
    fn test_find_workspace_root() {
        let fixture = fixture_path("simple_project");
        let start = fixture.join("projects/my_design");  // start inside a project dir
        let (root, manifest) = find_workspace_root(&start).unwrap();
        assert_eq!(root.canonicalize().unwrap(), fixture.canonicalize().unwrap());
        assert_eq!(manifest.workspace.name, "test_workspace");
    }

    #[test]
    fn test_discover_members() {
        let fixture = fixture_path("simple_project");
        let (root, manifest) = find_workspace_root(&fixture).unwrap();
        let members = discover_members(&root, &manifest).unwrap();

        let component_count = members.iter().filter(|m| m.kind == MemberKind::Component).count();
        let project_count = members.iter().filter(|m| m.kind == MemberKind::Project).count();

        assert_eq!(component_count, 1);  // axi_common
        assert_eq!(project_count, 1);    // my_design
    }

    #[test]
    fn test_no_workspace_error() {
        let result = find_workspace_root(Path::new("/tmp"));
        // /tmp has no workspace.toml, so this should error
        // (assuming /tmp doesn't have one — safe assumption for tests)
        assert!(result.is_err());
    }
}
```

## Error Variants to Add

Add to `LoomError` in `error.rs`:
```rust
NoWorkspace { start: PathBuf },
ProjectNotFound { name: String },
GlobPattern { pattern: String, message: String },
GlobError { message: String },
```

## Done When

- `cargo test -p loom-core` passes
- `find_workspace_root()` correctly navigates from a subdirectory up to the workspace root
- `discover_members()` returns the correct component and project counts for `simple_project` fixture
- `find_project("my_design")` returns the correct project manifest
