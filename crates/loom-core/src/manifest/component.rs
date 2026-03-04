use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ComponentManifest {
    pub component: ComponentMeta,
    #[serde(default)]
    pub filesets: HashMap<String, FileSet>,
    #[serde(default)]
    pub dependencies: HashMap<String, DependencySpec>,
    pub synth: Option<SynthOptions>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ComponentMeta {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileSet {
    #[serde(default)]
    pub files: Vec<PathBuf>,
    #[serde(default)]
    pub constraints: Vec<PathBuf>,
    #[serde(default = "default_constraint_scope")]
    pub constraint_scope: String,
    pub include_synth: Option<bool>,
    #[serde(default)]
    pub defines: Vec<String>,
    #[serde(default)]
    pub compile_options: Vec<String>,
}

fn default_constraint_scope() -> String {
    "component".to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    Simple(String),
    Detailed {
        version: String,
        variant: Option<String>,
        path: Option<PathBuf>,
    },
}

impl DependencySpec {
    pub fn version_string(&self) -> &str {
        match self {
            DependencySpec::Simple(v) => v,
            DependencySpec::Detailed { version, .. } => version,
        }
    }

    pub fn variant(&self) -> Option<&str> {
        match self {
            DependencySpec::Simple(_) => None,
            DependencySpec::Detailed { variant, .. } => variant.as_deref(),
        }
    }

    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            DependencySpec::Simple(_) => None,
            DependencySpec::Detailed { path, .. } => path.as_ref(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SynthOptions {
    #[serde(default)]
    pub ooc: bool,
    pub ooc_top: Option<String>,
}

impl ComponentManifest {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if !self.component.name.contains('/') {
            errors.push(format!(
                "Component name '{}' must use 'org/name' format (e.g., 'myorg/{}').",
                self.component.name, self.component.name
            ));
        }

        if semver::Version::parse(&self.component.version).is_err() {
            errors.push(format!(
                "Component version '{}' is not valid semver.",
                self.component.version
            ));
        }

        for (fs_name, fs) in &self.filesets {
            if fs.constraint_scope != "component" && fs.constraint_scope != "global" {
                errors.push(format!(
                    "Fileset '{}': constraint_scope must be 'component' or 'global', got '{}'.",
                    fs_name, fs.constraint_scope
                ));
            }
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_component() {
        let toml_str = r#"
[component]
name = "testorg/axi_fifo"
version = "1.0.0"
description = "A basic FIFO"

[filesets.synth]
files = ["rtl/fifo.sv"]
constraints = ["constraints/timing.xdc"]

[filesets.sim]
files = ["tb/fifo_tb.sv"]
include_synth = true
defines = ["SIMULATION"]

[dependencies]
axi_common = ">=1.0.0"
"#;
        let manifest: ComponentManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.component.name, "testorg/axi_fifo");
        assert_eq!(manifest.component.version, "1.0.0");
        assert!(manifest.filesets.contains_key("synth"));
        assert!(manifest.filesets.contains_key("sim"));
        assert_eq!(manifest.filesets["synth"].files.len(), 1);
        assert_eq!(manifest.filesets["sim"].defines, vec!["SIMULATION"]);
        assert!(manifest.dependencies.contains_key("axi_common"));
    }

    #[test]
    fn test_validate_namespace_required() {
        let toml_str = r#"
[component]
name = "bad_name"
version = "1.0.0"
[filesets.synth]
files = []
"#;
        let manifest: ComponentManifest = toml::from_str(toml_str).unwrap();
        let errors = manifest.validate();
        assert!(!errors.is_empty());
        assert!(errors[0].contains("org/name"));
    }

    #[test]
    fn test_detailed_dependency() {
        let toml_str = r#"
[component]
name = "org/my_component"
version = "1.0.0"
[filesets.synth]
files = []
[dependencies]
memory_ctrl = { version = ">=1.0.0", variant = "xilinx" }
"#;
        let manifest: ComponentManifest = toml::from_str(toml_str).unwrap();
        let dep = &manifest.dependencies["memory_ctrl"];
        assert_eq!(dep.version_string(), ">=1.0.0");
        assert_eq!(dep.variant(), Some("xilinx"));
    }

    #[test]
    fn test_default_constraint_scope() {
        let toml_str = r#"
[component]
name = "org/comp"
version = "1.0.0"
[filesets.synth]
files = []
constraints = ["timing.xdc"]
"#;
        let manifest: ComponentManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.filesets["synth"].constraint_scope, "component");
    }

    #[test]
    fn test_parse_fixture_component() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/simple_project/lib/axi_common/component.toml");
        let content = std::fs::read_to_string(&path).unwrap();
        let manifest: ComponentManifest = toml::from_str(&content).unwrap();
        assert_eq!(manifest.component.name, "testorg/axi_common");
        assert!(manifest.validate().is_empty());
    }
}
