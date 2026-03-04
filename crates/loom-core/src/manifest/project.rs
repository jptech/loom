use std::collections::HashMap;

use serde::Deserialize;

use super::component::{DependencySpec, FileSet};

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectManifest {
    pub project: ProjectMeta,
    pub target: Option<TargetSpec>,
    #[serde(default)]
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct TargetSpec {
    pub part: String,
    pub backend: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BuildConfig {
    pub build_dir: Option<String>,
    pub default_strategy: Option<String>,
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
            errors.push(
                "Project must specify [target] with part and backend. \
                 (Platform support comes in Phase 3.)"
                    .to_string(),
            );
        }

        errors
    }

    pub fn build_dir(&self) -> &str {
        self.build
            .as_ref()
            .and_then(|b| b.build_dir.as_deref())
            .unwrap_or(".build")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_project_manifest() {
        let toml_str = r#"
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
        let manifest: ProjectManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.project.name, "radar_processor");
        assert_eq!(manifest.project.top_module, "radar_top");
        let target = manifest.target.as_ref().unwrap();
        assert_eq!(target.part, "xczu7ev-ffvc1156-2-e");
        assert_eq!(target.backend, "vivado");
        assert!(manifest.dependencies.contains_key("axi_async_fifo"));
    }

    #[test]
    fn test_validate_missing_target() {
        let toml_str = r#"
[project]
name = "my_proj"
top_module = "top"
[filesets.synth]
files = []
"#;
        let manifest: ProjectManifest = toml::from_str(toml_str).unwrap();
        let errors = manifest.validate();
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_build_dir_default() {
        let toml_str = r#"
[project]
name = "test"
top_module = "top"
"#;
        let manifest: ProjectManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.build_dir(), ".build");
    }

    #[test]
    fn test_build_dir_custom() {
        let toml_str = r#"
[project]
name = "test"
top_module = "top"
[build]
build_dir = "output"
"#;
        let manifest: ProjectManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.build_dir(), "output");
    }

    #[test]
    fn test_parse_fixture_project() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/simple_project/projects/my_design/project.toml");
        let content = std::fs::read_to_string(&path).unwrap();
        let manifest: ProjectManifest = toml::from_str(&content).unwrap();
        assert_eq!(manifest.project.name, "my_design");
        assert!(manifest.validate().is_empty());
    }
}
