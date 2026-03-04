# Task 03: Project and Workspace Manifest Parsing

**Prerequisites:** Task 02 complete
**Goal:** Parse `project.toml` and `workspace.toml` into strongly-typed Rust structs.

## Spec Reference
`system_plan.md` §5.1 (Project Manifest), §11.2 (Workspace Manifest), §15 Phase 1 Core Data Types

## Files to Implement
- `crates/loom-core/src/manifest/project.rs`
- `crates/loom-core/src/manifest/workspace.rs`
- `crates/loom-core/src/manifest/common.rs`

---

## `common.rs` — Shared Types

```rust
// These types are reused across component, project, and workspace manifests.
// FileSet and DependencySpec live in component.rs and are re-exported here.
pub use super::component::{FileSet, DependencySpec};
```

---

## `project.rs` — Project Manifest

```rust
use std::path::PathBuf;
use std::collections::HashMap;
use serde::Deserialize;
use super::component::{FileSet, DependencySpec};

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectManifest {
    pub project: ProjectMeta,
    /// Direct part specification (Phase 1 — no platform support yet)
    pub target: Option<TargetSpec>,
    pub filesets: HashMap<String, FileSet>,
    #[serde(default)]
    pub dependencies: HashMap<String, DependencySpec>,
    pub build: Option<BuildConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub top_module: String,
    pub description: Option<String>,
    // Phase 3+: platform = "zcu104"
}

/// Direct part/backend specification.
/// Used in Phase 1 when no platform model exists.
/// Spec §5.1: [target] block.
#[derive(Debug, Clone, Deserialize)]
pub struct TargetSpec {
    pub part: String,        // "xczu7ev-ffvc1156-2-e"
    pub backend: String,     // "vivado"
    pub version: Option<String>,  // "2023.2"
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BuildConfig {
    pub build_dir: Option<String>,       // default: ".build"
    pub default_strategy: Option<String>, // default: "default"
}

impl ProjectManifest {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.project.name.is_empty() {
            errors.push("Project name cannot be empty.".to_string());
        }
        if self.project.top_module.is_empty() {
            errors.push("Project top_module cannot be empty.".to_string());
        }
        if self.target.is_none() {
            // Phase 3+: platform field will also be accepted
            errors.push(
                "Project must specify [target] with part and backend. \
                 (Platform support comes in Phase 3.)".to_string()
            );
        }

        errors
    }

    /// Returns the build directory path relative to project root.
    pub fn build_dir(&self) -> &str {
        self.build
            .as_ref()
            .and_then(|b| b.build_dir.as_deref())
            .unwrap_or(".build")
    }
}
```

---

## `workspace.rs` — Workspace Manifest

```rust
use std::collections::HashMap;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceManifest {
    pub workspace: WorkspaceMeta,
    #[serde(default)]
    pub settings: WorkspaceSettings,
    #[serde(default)]
    pub resolution: ResolutionConfig,
    #[serde(default)]
    pub plugins: HashMap<String, PluginDecl>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceMeta {
    pub name: String,
    pub members: Vec<String>,  // glob patterns: ["lib/*", "projects/*"]
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WorkspaceSettings {
    pub default_tool_version: Option<String>,
    pub build_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResolutionConfig {
    #[serde(default)]
    pub overrides: HashMap<String, ResolutionOverride>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResolutionOverride {
    pub path: Option<std::path::PathBuf>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginDecl {
    pub path: Option<std::path::PathBuf>,
}
```

---

## Add Load Functions to `manifest/mod.rs`

```rust
pub mod component;
pub mod project;
pub mod workspace;
pub mod common;

pub use component::{ComponentManifest, FileSet, DependencySpec};
pub use project::{ProjectManifest, TargetSpec, BuildConfig};
pub use workspace::WorkspaceManifest;

use std::path::Path;
use crate::error::LoomError;

pub fn load_component_manifest(path: &Path) -> Result<ComponentManifest, LoomError> {
    parse_toml_file(path)
}

pub fn load_project_manifest(path: &Path) -> Result<ProjectManifest, LoomError> {
    parse_toml_file(path)
}

pub fn load_workspace_manifest(path: &Path) -> Result<WorkspaceManifest, LoomError> {
    parse_toml_file(path)
}

fn parse_toml_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, LoomError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| LoomError::Io { path: path.to_owned(), source: e })?;
    toml::from_str::<T>(&content)
        .map_err(|e| LoomError::ManifestParse {
            path: path.to_owned(),
            message: e.to_string(),
        })
}
```

---

## Tests

```rust
// In project.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_project_manifest() {
        let toml = r#"
[project]
name = "radar_processor"
top_module = "radar_top"

[target]
part = "xczu7ev-ffvc1156-2-e"
backend = "vivado"
version = "2023.2"

[filesets.synth]
files = ["src/radar_top.sv"]
constraints = ["constraints/timing.xdc"]

[dependencies]
axi_async_fifo = ">=1.0.0"
"#;
        let manifest: ProjectManifest = toml::from_str(toml).unwrap();
        assert_eq!(manifest.project.name, "radar_processor");
        assert_eq!(manifest.project.top_module, "radar_top");
        let target = manifest.target.as_ref().unwrap();
        assert_eq!(target.part, "xczu7ev-ffvc1156-2-e");
        assert_eq!(target.backend, "vivado");
        assert!(manifest.dependencies.contains_key("axi_async_fifo"));
    }

    #[test]
    fn test_validate_missing_target() {
        let toml = r#"
[project]
name = "my_proj"
top_module = "top"
[filesets.synth]
files = []
"#;
        let manifest: ProjectManifest = toml::from_str(toml).unwrap();
        let errors = manifest.validate();
        assert!(!errors.is_empty());
    }
}

// In workspace.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_workspace_manifest() {
        let toml = r#"
[workspace]
name = "my_fpga_repo"
members = ["lib/*", "projects/*"]

[settings]
default_tool_version = "2023.2"
"#;
        let manifest: WorkspaceManifest = toml::from_str(toml).unwrap();
        assert_eq!(manifest.workspace.name, "my_fpga_repo");
        assert_eq!(manifest.workspace.members.len(), 2);
        assert_eq!(manifest.settings.default_tool_version.as_deref(), Some("2023.2"));
    }
}
```

## Done When

- `cargo test -p loom-core` passes all tests
- `load_project_manifest()` parses `tests/fixtures/simple_project/projects/my_design/project.toml`
- `load_workspace_manifest()` parses `tests/fixtures/simple_project/workspace.toml`
