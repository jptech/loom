use std::path::PathBuf;

use crate::assemble::fileset::AssembledFilesets;
use crate::build::context::BuildContext;
use crate::build::progress::BuildEvent;
use crate::error::LoomError;
use crate::resolve::resolver::ResolvedProject;

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

/// Options for build execution (resume, stop-after, etc.).
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    /// Resume from a checkpoint.
    pub resume: bool,
    /// Stop after this phase.
    pub stop_after: Option<String>,
    /// Start at this phase (skip earlier phases).
    pub start_at: Option<String>,
    /// Dry run — don't actually build.
    pub dry_run: bool,
}

/// Declares what a backend can and cannot do.
#[derive(Debug, Clone)]
pub struct BackendCapabilities {
    pub supports_ooc: bool,
    pub supports_incremental: bool,
    pub supports_ip_generation: bool,
    pub supports_block_design: bool,
    pub supports_strategy_sweep: bool,
    pub checkpoint_format: Option<String>,
    pub constraint_formats: Vec<String>,
    pub sub_phases: Vec<String>,
}

impl Default for BackendCapabilities {
    fn default() -> Self {
        Self {
            supports_ooc: false,
            supports_incremental: false,
            supports_ip_generation: false,
            supports_block_design: false,
            supports_strategy_sweep: false,
            checkpoint_format: None,
            constraint_formats: vec![],
            sub_phases: vec![
                "synthesis".to_string(),
                "place".to_string(),
                "route".to_string(),
                "bitstream".to_string(),
            ],
        }
    }
}

/// The interface a synthesis/implementation backend must implement.
pub trait BackendPlugin: Send + Sync {
    fn plugin_name(&self) -> &str;

    /// Return the backend's capabilities.
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::default()
    }

    fn check_environment(
        &self,
        required_version: Option<&str>,
    ) -> Result<EnvironmentStatus, LoomError>;

    fn validate(
        &self,
        project: &ResolvedProject,
        filesets: &AssembledFilesets,
        context: &BuildContext,
    ) -> Result<Vec<Diagnostic>, LoomError>;

    fn generate_build_scripts(
        &self,
        project: &ResolvedProject,
        filesets: &AssembledFilesets,
        context: &BuildContext,
    ) -> Result<Vec<PathBuf>, LoomError>;

    fn execute_build(
        &self,
        scripts: &[PathBuf],
        context: &BuildContext,
        progress: Option<&(dyn Fn(BuildEvent) + Send + Sync)>,
    ) -> Result<BuildResult, LoomError>;

    /// Resume a build from a checkpoint file.
    fn resume_build(
        &self,
        _checkpoint: &std::path::Path,
        _from_phase: &str,
        _options: &BuildOptions,
        _context: &BuildContext,
    ) -> Result<BuildResult, LoomError> {
        Err(LoomError::Internal(
            "resume_build not supported by this backend".to_string(),
        ))
    }

    /// Extract metrics from a completed build.
    fn extract_metrics(&self, _context: &BuildContext) -> Result<serde_json::Value, LoomError> {
        Ok(serde_json::Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            version_matches: false,
            license_ok: true,
            license_detail: None,
            warnings: vec![],
        };
        assert!(!status.is_ok());
    }
}
