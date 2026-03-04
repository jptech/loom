use std::path::PathBuf;

use crate::assemble::fileset::AssembledFilesets;
use crate::build::context::BuildContext;
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

/// The interface a synthesis/implementation backend must implement.
pub trait BackendPlugin: Send + Sync {
    fn plugin_name(&self) -> &str;

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
    ) -> Result<BuildResult, LoomError>;
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
