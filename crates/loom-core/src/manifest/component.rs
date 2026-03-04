use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

use super::generator::GeneratorDecl;
use super::test::{TestDecl, TestSuiteDecl};

#[derive(Debug, Clone, Deserialize)]
pub struct ComponentManifest {
    pub component: ComponentMeta,
    #[serde(default)]
    pub filesets: HashMap<String, FileSet>,
    #[serde(default)]
    pub dependencies: HashMap<String, DependencySpec>,
    pub synth: Option<SynthOptions>,
    #[serde(rename = "generators", default)]
    pub generators: Vec<GeneratorDecl>,
    /// Named variants for vendor/platform-specific customization.
    #[serde(default)]
    pub variants: HashMap<String, ComponentVariant>,
    /// Test declarations.
    #[serde(rename = "tests", default)]
    pub tests: Vec<TestDecl>,
    /// Test suites.
    #[serde(default)]
    pub test_suites: HashMap<String, TestSuiteDecl>,
}

/// A named variant that overlays the base component.
#[derive(Debug, Clone, Deserialize)]
pub struct ComponentVariant {
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub filesets: Option<VariantFilesetOverride>,
    #[serde(rename = "generators", default)]
    pub generators: Vec<GeneratorDecl>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VariantFilesetOverride {
    pub synth: Option<VariantFileset>,
    pub sim: Option<VariantFileset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VariantFileset {
    #[serde(default)]
    pub add_files: Vec<PathBuf>,
    #[serde(default)]
    pub remove_files: Vec<PathBuf>,
    #[serde(default)]
    pub add_constraints: Vec<PathBuf>,
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

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
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
    fn test_parse_generator_manifest() {
        let toml_str = r#"
[component]
name = "org/comp"
version = "1.0.0"
[filesets.synth]
files = []

[[generators]]
name = "regmap"
plugin = "command"
command = "python gen.py"
inputs = ["regs.yaml"]
outputs = ["generated/regs.sv"]
fileset = "synth"

[[generators]]
name = "sys_clk"
plugin = "vivado_ip"
[generators.config]
vlnv = "xilinx.com:ip:clk_wiz"
"#;
        let manifest: ComponentManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.generators.len(), 2);
        assert_eq!(manifest.generators[0].name, "regmap");
        assert_eq!(manifest.generators[0].plugin, "command");
        assert_eq!(manifest.generators[0].inputs.len(), 1);
        assert_eq!(manifest.generators[0].outputs.len(), 1);
        assert_eq!(manifest.generators[1].plugin, "vivado_ip");
        assert!(manifest.generators[1].config.is_some());
    }

    #[test]
    fn test_no_generators_default() {
        let toml_str = r#"
[component]
name = "org/comp"
version = "1.0.0"
[filesets.synth]
files = []
"#;
        let manifest: ComponentManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.generators.is_empty());
    }

    #[test]
    fn test_parse_component_with_variants() {
        let toml_str = r#"
[component]
name = "org/memory_ctrl"
version = "1.0.0"

[filesets.synth]
files = ["rtl/memory_ctrl.sv"]

[variants.xilinx]
description = "Xilinx MIG-based implementation"
tags = ["vendor:xilinx"]

[variants.xilinx.filesets.synth]
add_files = ["rtl/xilinx/mig_wrapper.sv"]
add_constraints = ["constraints/xilinx/mig_timing.xdc"]

[[variants.xilinx.generators]]
name = "mig_ip"
plugin = "vivado_ip"
[variants.xilinx.generators.config]
vlnv = "xilinx.com:ip:mig_7series"

[variants.intel]
description = "Intel EMIF-based"
tags = ["vendor:intel"]

[variants.intel.filesets.synth]
add_files = ["rtl/intel/emif_wrapper.sv"]
"#;
        let manifest: ComponentManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.variants.len(), 2);
        let xilinx = &manifest.variants["xilinx"];
        assert_eq!(xilinx.tags, vec!["vendor:xilinx"]);
        assert_eq!(
            xilinx
                .filesets
                .as_ref()
                .unwrap()
                .synth
                .as_ref()
                .unwrap()
                .add_files
                .len(),
            1
        );
        assert_eq!(xilinx.generators.len(), 1);
        assert_eq!(xilinx.generators[0].plugin, "vivado_ip");
    }

    #[test]
    fn test_no_variants_default() {
        let toml_str = r#"
[component]
name = "org/comp"
version = "1.0.0"
[filesets.synth]
files = []
"#;
        let manifest: ComponentManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.variants.is_empty());
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
