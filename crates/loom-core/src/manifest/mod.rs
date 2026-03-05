pub mod common;
pub mod component;
pub mod generator;
pub mod platform;
pub mod project;
pub mod test;
pub mod workspace;

pub use component::{ComponentManifest, DependencySpec, FileSet};
pub use generator::GeneratorDecl;
pub use platform::PlatformManifest;
pub use project::{BuildConfig, CheckpointConfig, ProjectManifest, ReportConfig, TargetSpec};
pub use test::{TestCaseResult, TestDecl, TestStatus, TestSuiteDecl, TestSuiteReport};
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

pub fn load_platform_manifest(path: &Path) -> Result<PlatformManifest, LoomError> {
    parse_toml_file(path)
}

fn parse_toml_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, LoomError> {
    let content = std::fs::read_to_string(path).map_err(|e| LoomError::Io {
        path: path.to_owned(),
        source: e,
    })?;
    toml::from_str::<T>(&content).map_err(|e| LoomError::ManifestParse {
        path: path.to_owned(),
        message: e.to_string(),
    })
}
