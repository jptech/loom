# Task 02: Component Manifest Parsing

**Prerequisites:** Task 01 complete
**Goal:** Parse `component.toml` files into strongly-typed Rust structs.

## Spec Reference
`system_plan.md` §3.1 (Component Manifest), §3.1.1 (Namespacing), §15 Phase 1 Core Data Types

## File to Implement
`crates/loom-core/src/manifest/component.rs`

## Types to Implement

```rust
use std::path::PathBuf;
use std::collections::HashMap;
use serde::Deserialize;

/// Top-level component.toml structure
#[derive(Debug, Clone, Deserialize)]
pub struct ComponentManifest {
    pub component: ComponentMeta,
    pub filesets: HashMap<String, FileSet>,
    #[serde(default)]
    pub dependencies: HashMap<String, DependencySpec>,
    pub synth: Option<SynthOptions>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ComponentMeta {
    pub name: String,          // "org/name" format, e.g. "acmecorp/axi_async_fifo"
    pub version: String,       // semver string: "1.2.0"
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileSet {
    #[serde(default)]
    pub files: Vec<PathBuf>,
    #[serde(default)]
    pub constraints: Vec<PathBuf>,
    #[serde(default = "default_constraint_scope")]
    pub constraint_scope: String,   // "component" | "global"
    pub include_synth: Option<bool>,
    #[serde(default)]
    pub defines: Vec<String>,
    #[serde(default)]
    pub compile_options: Vec<String>,
}

fn default_constraint_scope() -> String { "component".to_string() }

/// A dependency requirement, either a plain version string or a table with options.
/// Supports both:
///   axi_common = ">=1.0.0"
///   memory_ctrl = { version = ">=1.0.0", variant = "xilinx" }
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
```

## Parsing Function

In `crates/loom-core/src/manifest/mod.rs`, add:

```rust
use std::path::Path;
use crate::error::LoomError;

pub fn load_component_manifest(path: &Path) -> Result<ComponentManifest, LoomError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| LoomError::Io { path: path.to_owned(), source: e })?;
    toml::from_str::<ComponentManifest>(&content)
        .map_err(|e| LoomError::ManifestParse {
            path: path.to_owned(),
            message: e.to_string(),
        })
}
```

## Validation

Add a `validate()` method to `ComponentManifest`:
```rust
impl ComponentManifest {
    /// Validate the manifest after parsing.
    /// Returns a list of error strings. Empty = valid.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        // Enforce "org/name" namespace format
        if !self.component.name.contains('/') {
            errors.push(format!(
                "Component name '{}' must use 'org/name' format (e.g., 'myorg/{}').",
                self.component.name, self.component.name
            ));
        }

        // Validate version is semver-parseable
        if semver::Version::parse(&self.component.version).is_err() {
            errors.push(format!(
                "Component version '{}' is not valid semver.",
                self.component.version
            ));
        }

        // Validate constraint_scope values
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
```

## Tests

In `crates/loom-core/src/manifest/component.rs`, add `#[cfg(test)]` block:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_component() {
        let toml = r#"
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
        let manifest: ComponentManifest = toml::from_str(toml).unwrap();
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
        let toml = r#"
[component]
name = "bad_name"   # missing org/ prefix
version = "1.0.0"
[filesets.synth]
files = []
"#;
        let manifest: ComponentManifest = toml::from_str(toml).unwrap();
        let errors = manifest.validate();
        assert!(!errors.is_empty());
        assert!(errors[0].contains("org/name"));
    }

    #[test]
    fn test_detailed_dependency() {
        let toml = r#"
[component]
name = "org/my_component"
version = "1.0.0"
[filesets.synth]
files = []
[dependencies]
memory_ctrl = { version = ">=1.0.0", variant = "xilinx" }
"#;
        let manifest: ComponentManifest = toml::from_str(toml).unwrap();
        let dep = &manifest.dependencies["memory_ctrl"];
        assert_eq!(dep.version_string(), ">=1.0.0");
        assert_eq!(dep.variant(), Some("xilinx"));
    }

    #[test]
    fn test_default_constraint_scope() {
        let toml = r#"
[component]
name = "org/comp"
version = "1.0.0"
[filesets.synth]
files = []
constraints = ["timing.xdc"]
"#;
        let manifest: ComponentManifest = toml::from_str(toml).unwrap();
        assert_eq!(manifest.filesets["synth"].constraint_scope, "component");
    }
}
```

## Done When

- `cargo test -p loom-core` passes all tests in this module
- `load_component_manifest()` successfully parses `tests/fixtures/simple_project/lib/axi_common/component.toml`
- `validate()` returns empty vec for valid manifests, errors for invalid ones
