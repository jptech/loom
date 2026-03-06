use std::path::PathBuf;

use crate::assemble::fileset::AssembledFilesets;
use crate::build::context::BuildContext;
use crate::error::LoomError;
use crate::plugin::backend::{BackendPlugin, Diagnostic, DiagnosticSeverity};
use crate::resolve::resolver::ResolvedProject;

pub struct ValidationResult {
    pub diagnostics: Vec<Diagnostic>,
}

impl ValidationResult {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error)
    }

    pub fn errors(&self) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect()
    }

    pub fn warnings(&self) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
            .collect()
    }
}

fn make_error(message: impl Into<String>, source: Option<PathBuf>) -> Diagnostic {
    Diagnostic {
        severity: DiagnosticSeverity::Error,
        message: message.into(),
        source_path: source,
        line: None,
    }
}

fn make_warning(message: impl Into<String>, source: Option<PathBuf>) -> Diagnostic {
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
                    file.path.display(),
                    file.source_component
                ),
                Some(file.path.clone()),
            ));
            continue;
        }

        let metadata = std::fs::metadata(&file.path).map_err(|e| LoomError::Io {
            path: file.path.clone(),
            source: e,
        })?;
        if metadata.len() == 0 {
            diagnostics.push(make_warning(
                format!("Source file is empty: '{}'", file.path.display()),
                Some(file.path.clone()),
            ));
        }

        let ext = file.path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let recognized = matches!(
            ext.to_lowercase().as_str(),
            "sv" | "svh" | "v" | "vh" | "vhd" | "vhdl"
        );
        if !recognized {
            diagnostics.push(make_warning(
                format!(
                    "File '{}' has unrecognized extension '.{}'. Expected: .sv, .v, .vhd",
                    file.path.display(),
                    ext
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
                    constraint.path.display(),
                    constraint.source_component
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
            "Project must specify a [target] block with part and backend.",
            Some(resolved.project_root.join("project.toml")),
        ));
    }

    // Check 4: Tool environment
    let required_version = resolved
        .project
        .target
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
                            "Backend '{}' version mismatch: required {}, found {}.",
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
            diagnostics.push(make_error(format!("Environment check failed: {}", e), None));
        }
    }

    // Check 5: Backend-specific validation
    match backend.validate(resolved, filesets, context) {
        Ok(backend_diagnostics) => diagnostics.extend(backend_diagnostics),
        Err(e) => diagnostics.push(make_error(format!("Backend validation error: {}", e), None)),
    }

    Ok(ValidationResult { diagnostics })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assemble::fileset::{
        assemble_filesets, AssembledFile, AssembledFilesets, FileLanguage,
    };
    use crate::plugin::backend::{BuildResult, EnvironmentStatus};
    use crate::resolve::resolver::{resolve_project, WorkspaceDependencySource};
    use crate::resolve::workspace::{
        discover_members, find_project, find_workspace_root, load_all_components,
    };

    struct MockBackend {
        pass_env: bool,
    }

    impl BackendPlugin for MockBackend {
        fn plugin_name(&self) -> &str {
            "mock"
        }

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

        fn validate(
            &self,
            _p: &ResolvedProject,
            _f: &AssembledFilesets,
            _c: &BuildContext,
        ) -> Result<Vec<Diagnostic>, LoomError> {
            Ok(vec![])
        }

        fn generate_build_scripts(
            &self,
            _p: &ResolvedProject,
            _f: &AssembledFilesets,
            _c: &BuildContext,
        ) -> Result<Vec<PathBuf>, LoomError> {
            Ok(vec![])
        }

        fn execute_build(
            &self,
            _s: &[PathBuf],
            _c: &BuildContext,
            _progress: Option<&(dyn Fn(crate::build::progress::BuildEvent) + Send + Sync)>,
        ) -> Result<BuildResult, LoomError> {
            unimplemented!()
        }
    }

    fn resolve_simple_project() -> ResolvedProject {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/simple_project");
        let (root, ws_manifest) = find_workspace_root(&fixture).unwrap();
        let members = discover_members(&root, &ws_manifest).unwrap();
        let all_components = load_all_components(&members).unwrap();
        let (project_root, project_manifest) = find_project(&members, None).unwrap();
        let source = WorkspaceDependencySource::new(all_components);
        resolve_project(project_manifest, project_root, root, &source).unwrap()
    }

    #[test]
    fn test_valid_project_passes() {
        let resolved = resolve_simple_project();
        let filesets = assemble_filesets(&resolved).unwrap();
        let context = BuildContext::new(resolved.clone(), resolved.workspace_root.clone());
        let backend = MockBackend { pass_env: true };

        let result = validate_pre_build(&resolved, &filesets, &context, &backend).unwrap();
        assert!(
            !result.has_errors(),
            "Valid project should pass validation. Errors: {:?}",
            result
                .errors()
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_missing_file_detected() {
        let resolved = resolve_simple_project();
        let filesets = AssembledFilesets {
            synth_files: vec![AssembledFile {
                path: PathBuf::from("/nonexistent/file.sv"),
                source_component: "test".to_string(),
                language: FileLanguage::SystemVerilog,
            }],
            sim_files: vec![],
            constraint_files: vec![],
            defines: vec![],
        };
        let context = BuildContext::new(resolved.clone(), resolved.workspace_root.clone());
        let backend = MockBackend { pass_env: true };

        let result = validate_pre_build(&resolved, &filesets, &context, &backend).unwrap();
        assert!(result.has_errors());
        assert!(result.errors()[0].message.contains("not found"));
    }

    #[test]
    fn test_env_check_failure() {
        let resolved = resolve_simple_project();
        let filesets = assemble_filesets(&resolved).unwrap();
        let context = BuildContext::new(resolved.clone(), resolved.workspace_root.clone());
        let backend = MockBackend { pass_env: false };

        let _result = validate_pre_build(&resolved, &filesets, &context, &backend).unwrap();
    }
}
