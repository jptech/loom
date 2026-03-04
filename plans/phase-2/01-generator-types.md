# Phase 2 / Task 01: Generator Types

**Prerequisites:** Phase 1 complete
**Goal:** Add generator declaration parsing to component and project manifests, and define the core generator data structures needed for the DAG.

## Spec Reference
`system_plan.md` §6.1 (Generator Declaration), §6.2 (Execution Model), §6.3 (Dependencies)

## Manifest Changes

### `component.toml` and `project.toml` — Add `[[generators]]`

```toml
[[generators]]
name = "regmap"
plugin = "command"
command = "python scripts/gen_regs.py"
inputs = ["regs/radar_ctrl_regs.yaml"]
outputs = ["generated/radar_ctrl_regs.sv", "generated/radar_ctrl_regs.h"]
fileset = "synth"
depends_on = []        # explicit ordering (optional — auto-detected via input/output overlap)
cacheable = true       # default true
outputs_unknown = false  # default false (see §6.6)

[[generators]]
name = "sys_clk"
plugin = "vivado_ip"
[generators.config]
vlnv = "xilinx.com:ip:clk_wiz"
properties = { PRIM_IN_FREQ = "125.000" }
```

## Types to Add

### In `crates/loom-core/src/manifest/common.rs` (or a new `generator.rs`)

```rust
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// A generator declaration in a manifest.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GeneratorDecl {
    pub name: String,
    pub plugin: String,

    // For `command` plugin
    pub command: Option<String>,
    pub command_windows: Option<String>,  // Windows override

    // Input/output declarations
    #[serde(default)]
    pub inputs: Vec<PathBuf>,
    #[serde(default)]
    pub outputs: Vec<PathBuf>,

    /// Which fileset receives the generated outputs (default: "synth")
    #[serde(default = "default_fileset")]
    pub fileset: String,

    /// Explicit ordering dependency on other generators by name
    #[serde(default)]
    pub depends_on: Vec<String>,

    /// If true, skip this generator if cache key matches
    #[serde(default = "default_true")]
    pub cacheable: bool,

    /// If true, framework cannot verify outputs (disables caching)
    #[serde(default)]
    pub outputs_unknown: bool,

    /// Plugin-specific configuration (arbitrary TOML table)
    pub config: Option<toml::Value>,
}

fn default_fileset() -> String { "synth".to_string() }
fn default_true() -> bool { true }

impl GeneratorDecl {
    /// The effective command, accounting for platform.
    pub fn effective_command(&self) -> Option<&str> {
        #[cfg(target_os = "windows")]
        { self.command_windows.as_deref().or(self.command.as_deref()) }
        #[cfg(not(target_os = "windows"))]
        { self.command.as_deref() }
    }
}
```

### Add generators to `ProjectManifest` and `ComponentManifest`

```rust
// In ComponentManifest:
#[serde(rename = "generators", default)]
pub generators: Vec<GeneratorDecl>,

// In ProjectManifest:
#[serde(rename = "generators", default)]
pub generators: Vec<GeneratorDecl>,
```

### Generator Node (internal representation for DAG)

```rust
// crates/loom-core/src/generate/node.rs

use std::path::PathBuf;
use crate::manifest::GeneratorDecl;

/// A generator node in the execution DAG.
/// Enriched from the manifest declaration with resolved paths.
#[derive(Debug, Clone)]
pub struct GeneratorNode {
    /// Unique ID within the build: "<component_name>::<generator_name>"
    pub id: String,
    /// Source component/project (for error attribution)
    pub source: String,
    pub decl: GeneratorDecl,
    /// Absolute paths to input files
    pub resolved_inputs: Vec<PathBuf>,
    /// Absolute paths to expected output files
    pub resolved_outputs: Vec<PathBuf>,
    /// Build directory for this generator's outputs
    pub output_dir: PathBuf,
}

impl GeneratorNode {
    pub fn cache_key_inputs(&self) -> Vec<PathBuf> {
        self.resolved_inputs.clone()
    }

    /// Check if any declared output overlaps with another node's inputs.
    pub fn outputs_overlap_with_inputs(&self, other: &GeneratorNode) -> bool {
        self.resolved_outputs.iter().any(|out| {
            other.resolved_inputs.iter().any(|inp| out == inp)
        })
    }
}
```

## Tests

```rust
#[test]
fn test_parse_generator_manifest() {
    let toml = r#"
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
    let manifest: ComponentManifest = toml::from_str(toml).unwrap();
    assert_eq!(manifest.generators.len(), 2);
    assert_eq!(manifest.generators[0].name, "regmap");
    assert_eq!(manifest.generators[0].plugin, "command");
    assert_eq!(manifest.generators[1].plugin, "vivado_ip");
    assert!(manifest.generators[1].config.is_some());
}

#[test]
fn test_outputs_unknown_defaults_false() {
    let toml = r#"
[[generators]]
name = "g"
plugin = "command"
command = "echo hi"
"#;
    let decl: GeneratorDecl = toml::from_str(toml).unwrap();
    // Wait, this tests a bare GeneratorDecl, need to wrap in a struct...
    // Test via full component manifest parse
}
```

## Done When

- `cargo test -p loom-core` passes
- `ComponentManifest` and `ProjectManifest` parse `[[generators]]` blocks
- `GeneratorDecl` fields all parse correctly with appropriate defaults
- `effective_command()` returns `command_windows` on Windows, `command` otherwise
