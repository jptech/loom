# Task 09: Pre-Build Validation

**Prerequisites:** Task 08 complete
**Goal:** Implement Phase 4 (VALIDATE) — a set of fast pre-flight checks that catch errors before invoking the vendor tool.

## Spec Reference
`system_plan.md` §7.4 (Pre-Build Validation)

## File to Implement
`crates/loom-core/src/build/validate.rs`

## Validation Checks (Phase 1 Scope)

Phase 1 implements these built-in checks:
1. **File existence** — every file in the assembled file-set exists on disk
2. **File type consistency** — files have recognized extensions
3. **Non-empty files** — files are not zero bytes
4. **Dependency completeness** — all declared deps resolved (already done by resolver, but double-check)
5. **Target specification** — project has a `[target]` block with non-empty part/backend
6. **Tool environment** — the required backend is available (delegates to backend plugin's `check_environment`)

## Implementation

```rust
use std::path::Path;
use crate::assemble::fileset::AssembledFilesets;
use crate::resolve::resolver::ResolvedProject;
use crate::plugin::backend::{BackendPlugin, Diagnostic, DiagnosticSeverity};
use crate::build::context::BuildContext;
use crate::error::LoomError;

pub struct ValidationResult {
    pub diagnostics: Vec<Diagnostic>,
}

impl ValidationResult {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == DiagnosticSeverity::Error)
    }

    pub fn errors(&self) -> Vec<&Diagnostic> {
        self.diagnostics.iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect()
    }

    pub fn warnings(&self) -> Vec<&Diagnostic> {
        self.diagnostics.iter()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
            .collect()
    }
}

fn make_error(message: impl Into<String>, source: Option<std::path::PathBuf>) -> Diagnostic {
    Diagnostic {
        severity: DiagnosticSeverity::Error,
        message: message.into(),
        source_path: source,
        line: None,
    }
}

fn make_warning(message: impl Into<String>, source: Option<std::path::PathBuf>) -> Diagnostic {
    Diagnostic {
        severity: DiagnosticSeverity::Warning,
        message: message.into(),
        source_path: source,
        line: None,
    }
}

/// Run all Phase 4 pre-build validation checks.
pub fn validate_pre_build(
    resolved: &ResolvedProject,
    filesets: &AssembledFilesets,
    context: &BuildContext,
    backend: &dyn BackendPlugin,
) -> Result<ValidationResult, LoomError> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // Check 1: File existence and basic integrity
    for file in &filesets.synth_files {
        if !file.path.exists() {
            diagnostics.push(make_error(
                format!(
                    "Source file not found: '{}' (from component '{}')",
                    file.path.display(), file.source_component
                ),
                Some(file.path.clone()),
            ));
            continue;
        }

        // Non-empty check
        let metadata = std::fs::metadata(&file.path)
            .map_err(|e| LoomError::Io { path: file.path.clone(), source: e })?;
        if metadata.len() == 0 {
            diagnostics.push(make_warning(
                format!("Source file is empty: '{}'", file.path.display()),
                Some(file.path.clone()),
            ));
        }

        // Extension check
        let ext = file.path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let recognized = matches!(ext.to_lowercase().as_str(), "sv" | "svh" | "v" | "vh" | "vhd" | "vhdl");
        if !recognized {
            diagnostics.push(make_warning(
                format!(
                    "File '{}' has unrecognized extension '.{}'. Expected: .sv, .v, .vhd",
                    file.path.display(), ext
                ),
                Some(file.path.clone()),
            ));
        }
    }

    // Check 2: Constraint file existence
    for constraint in &filesets.constraint_files {
        if !constraint.path.exists() {
            diagnostics.push(make_error(
                format!(
                    "Constraint file not found: '{}' (from component '{}')",
                    constraint.path.display(), constraint.source_component
                ),
                Some(constraint.path.clone()),
            ));
        }
    }

    // Check 3: Target specification
    if let Some(target) = &resolved.project.target {
        if target.part.is_empty() {
            diagnostics.push(make_error(
                "Project [target].part cannot be empty.",
                Some(resolved.project_root.join("project.toml")),
            ));
        }
        if target.backend.is_empty() {
            diagnostics.push(make_error(
                "Project [target].backend cannot be empty.",
                Some(resolved.project_root.join("project.toml")),
            ));
        }
    } else {
        diagnostics.push(make_error(
            "Project must specify a [target] block with part and backend. \
             Platform support (Phase 3) is not yet available.",
            Some(resolved.project_root.join("project.toml")),
        ));
    }

    // Check 4: Tool environment (delegate to backend plugin)
    let required_version = resolved.project.target
        .as_ref()
        .and_then(|t| t.version.as_deref());

    match backend.check_environment(required_version) {
        Ok(env_status) if env_status.is_ok() => {
            for warning in &env_status.warnings {
                diagnostics.push(make_warning(warning.clone(), None));
            }
        }
        Ok(env_status) => {
            if !env_status.version_matches {
                if let Some(required) = &env_status.required_version {
                    diagnostics.push(make_error(
                        format!(
                            "Backend '{}' version mismatch: required {}, found {}.\n\
                             Update platform.toml or install the correct version.",
                            env_status.tool_name, required, env_status.version
                        ),
                        None,
                    ));
                }
            }
            if !env_status.license_ok {
                diagnostics.push(make_error(
                    format!("Backend '{}' license check failed.", env_status.tool_name),
                    None,
                ));
            }
        }
        Err(e) => {
            diagnostics.push(make_error(
                format!("Environment check failed: {}", e),
                None,
            ));
        }
    }

    // Check 5: Backend-specific validation
    match backend.validate(resolved, filesets, context) {
        Ok(backend_diagnostics) => diagnostics.extend(backend_diagnostics),
        Err(e) => diagnostics.push(make_error(
            format!("Backend validation error: {}", e), None,
        )),
    }

    Ok(ValidationResult { diagnostics })
}
```

## Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::backend::{BuildResult, EnvironmentStatus};
    use std::path::PathBuf;

    /// A mock backend that always passes validation
    struct MockBackend { pass_env: bool }

    impl BackendPlugin for MockBackend {
        fn plugin_name(&self) -> &str { "mock" }

        fn check_environment(&self, _req: Option<&str>) -> Result<EnvironmentStatus, LoomError> {
            Ok(EnvironmentStatus {
                tool_name: "mock".to_string(),
                tool_path: PathBuf::from("/usr/bin/mock"),
                version: "1.0.0".to_string(),
                required_version: None,
                version_matches: self.pass_env,
                license_ok: true,
                license_detail: None,
                warnings: vec![],
            })
        }

        fn validate(&self, _p: &_, _f: &_, _c: &_) -> Result<Vec<Diagnostic>, LoomError> {
            Ok(vec![])
        }

        fn generate_build_scripts(&self, _p: &_, _f: &_, _c: &_) -> Result<Vec<PathBuf>, LoomError> {
            Ok(vec![])
        }

        fn execute_build(&self, _s: &_, _c: &_) -> Result<BuildResult, LoomError> {
            unimplemented!()
        }
    }

    #[test]
    fn test_missing_file_detected() {
        // Build an AssembledFilesets with a non-existent file path
        // Call validate_pre_build() with MockBackend
        // Expect at least one Error diagnostic about the missing file
    }

    #[test]
    fn test_valid_project_passes() {
        // Use simple_project fixture
        // All files exist, backend mock passes
        // Expect ValidationResult with no errors
    }

    #[test]
    fn test_missing_target_block() {
        // Create a ProjectManifest with target = None
        // Expect Error diagnostic
    }
}
```

## Done When

- `cargo test -p loom-core` passes
- A project with all files present and valid target block: zero error diagnostics
- A project with missing source files: error diagnostic per missing file
- A project with no `[target]` block: error diagnostic
- Environmental failure from mock backend: error diagnostic
