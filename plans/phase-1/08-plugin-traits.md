# Task 08: Plugin Trait Definitions

**Prerequisites:** Task 07 complete
**Goal:** Define the `BackendPlugin` Rust trait and supporting types. Phase 1 uses a compiled-in Rust implementation; the trait is the interface the Vivado backend will implement.

## Spec Reference
`system_plan.md` §10.1 (Plugin Types), §10.3.2 (Backend Plugin Interface), §10.4 (BuildContext)

**Note:** Phase 1 defines these as Rust traits (not Python ABCs). Python loading via PyO3 comes in Phase 2.

## Files to Implement
`crates/loom-core/src/plugin/backend.rs`
`crates/loom-core/src/plugin/generator.rs`
`crates/loom-core/src/build/context.rs`

---

## `build/context.rs` — BuildContext

```rust
use std::path::PathBuf;
use std::collections::HashMap;
use crate::resolve::resolver::ResolvedProject;

/// Build context passed to backend plugins.
/// Provides access to project state without coupling to internals.
#[derive(Debug, Clone)]
pub struct BuildContext {
    pub project: ResolvedProject,
    pub build_dir: PathBuf,         // output directory for this build
    pub workspace_root: PathBuf,
    pub env: HashMap<String, String>,  // environment variables
    pub strategy: String,            // build strategy name (default: "default")
}

impl BuildContext {
    pub fn new(project: ResolvedProject, workspace_root: PathBuf) -> Self {
        // .build/ lives at workspace root (spec §11.2): .build/<project_name>/<strategy>/
        let build_dir = workspace_root
            .join(".build")
            .join(&project.project.project.name)
            .join("default");

        Self {
            build_dir,
            workspace_root,
            env: std::env::vars().collect(),
            strategy: "default".to_string(),
            project,
        }
    }
}
```

---

## `plugin/backend.rs` — Backend Plugin Trait

```rust
use std::path::{Path, PathBuf};
use crate::assemble::fileset::AssembledFilesets;
use crate::resolve::resolver::ResolvedProject;
use crate::build::context::BuildContext;
use crate::error::LoomError;

/// Status of the tool environment check.
#[derive(Debug, Clone)]
pub struct EnvironmentStatus {
    pub tool_name: String,
    pub tool_path: PathBuf,
    pub version: String,
    pub required_version: Option<String>,
    pub version_matches: bool,
    pub license_ok: bool,
    pub license_detail: Option<String>,
    pub warnings: Vec<String>,
}

impl EnvironmentStatus {
    pub fn is_ok(&self) -> bool {
        self.version_matches && self.license_ok
    }
}

/// Result of a build execution.
#[derive(Debug, Clone)]
pub struct BuildResult {
    pub success: bool,
    pub exit_code: i32,
    pub log_paths: Vec<PathBuf>,
    pub bitstream_path: Option<PathBuf>,
    pub phases_completed: Vec<String>,
    pub failure_phase: Option<String>,
    pub failure_message: Option<String>,
}

/// A diagnostic message from validation.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub source_path: Option<PathBuf>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

/// The interface a synthesis/implementation backend must implement.
/// Phase 1: implemented as a Rust struct (VivadoBackend).
/// Phase 2+: can also be loaded as a Python plugin via PyO3.
pub trait BackendPlugin: Send + Sync {
    fn plugin_name(&self) -> &str;

    /// Verify the tool is installed, at the right version, and licensed.
    fn check_environment(
        &self,
        required_version: Option<&str>,
    ) -> Result<EnvironmentStatus, LoomError>;

    /// Run backend-specific pre-build validation (Phase 4 backend checks).
    fn validate(
        &self,
        project: &ResolvedProject,
        filesets: &AssembledFilesets,
        context: &BuildContext,
    ) -> Result<Vec<Diagnostic>, LoomError>;

    /// Generate tool-specific build scripts. Returns paths to the generated scripts.
    fn generate_build_scripts(
        &self,
        project: &ResolvedProject,
        filesets: &AssembledFilesets,
        context: &BuildContext,
    ) -> Result<Vec<PathBuf>, LoomError>;

    /// Execute the build. Manages tool invocation, log capture, checkpoint tracking.
    fn execute_build(
        &self,
        scripts: &[PathBuf],
        context: &BuildContext,
    ) -> Result<BuildResult, LoomError>;
}
```

---

## `plugin/generator.rs` — Generator Plugin Trait (Phase 1 stub)

```rust
use std::path::PathBuf;
use crate::build::context::BuildContext;
use crate::error::LoomError;
use crate::plugin::backend::Diagnostic;

/// A generator produces derived files from inputs.
/// Phase 1: interface defined but no generators are used.
/// Phase 2: `command` generator plugin implemented.
pub trait GeneratorPlugin: Send + Sync {
    fn plugin_name(&self) -> &str;

    fn validate_config(&self, config: &toml::Value) -> Result<Vec<Diagnostic>, LoomError>;

    fn compute_cache_key(
        &self,
        config: &toml::Value,
        input_hashes: &std::collections::HashMap<String, String>,
    ) -> Result<String, LoomError>;

    fn execute(
        &self,
        config: &toml::Value,
        context: &BuildContext,
    ) -> Result<GeneratorResult, LoomError>;

    fn clean(&self, config: &toml::Value, context: &BuildContext) -> Result<(), LoomError>;
}

#[derive(Debug, Clone)]
pub struct GeneratorResult {
    pub success: bool,
    pub produced_files: Vec<PathBuf>,
    pub log: Vec<String>,
}
```

---

## `plugin/mod.rs`

```rust
pub mod backend;
pub mod generator;

pub use backend::{BackendPlugin, BuildResult, EnvironmentStatus, Diagnostic, DiagnosticSeverity};
pub use generator::{GeneratorPlugin, GeneratorResult};
```

---

## Tests

```rust
// In plugin/backend.rs
#[cfg(test)]
mod tests {
    use super::*;

    // Test that EnvironmentStatus::is_ok() logic is correct
    #[test]
    fn test_env_status_ok() {
        let status = EnvironmentStatus {
            tool_name: "vivado".to_string(),
            tool_path: PathBuf::from("/tools/Xilinx/Vivado/2023.2/bin/vivado"),
            version: "2023.2".to_string(),
            required_version: Some("2023.2".to_string()),
            version_matches: true,
            license_ok: true,
            license_detail: None,
            warnings: vec![],
        };
        assert!(status.is_ok());
    }

    #[test]
    fn test_env_status_version_mismatch() {
        let status = EnvironmentStatus {
            tool_name: "vivado".to_string(),
            tool_path: PathBuf::from("/tools/Xilinx/Vivado/2024.1/bin/vivado"),
            version: "2024.1".to_string(),
            required_version: Some("2023.2".to_string()),
            version_matches: false,  // mismatch
            license_ok: true,
            license_detail: None,
            warnings: vec![],
        };
        assert!(!status.is_ok());
    }
}
```

## Done When

- `cargo build --workspace` compiles without errors
- `BackendPlugin`, `GeneratorPlugin` traits defined with correct signatures
- `BuildContext` type defined and constructable from a `ResolvedProject`
- `Diagnostic` and `EnvironmentStatus` types defined correctly
- All tests pass
