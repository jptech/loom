use std::path::{Path, PathBuf};

use crate::assemble::fileset::AssembledFilesets;
use crate::build::context::BuildContext;
use crate::error::LoomError;
use crate::plugin::backend::EnvironmentStatus;

/// Capabilities of a simulator.
#[derive(Debug, Clone)]
pub struct SimulatorCapabilities {
    pub systemverilog_full: bool,
    pub vhdl: bool,
    pub mixed_language: bool,
    pub uvm: bool,
    pub fork_join: bool,
    pub force_release: bool,
    pub bind_statements: bool,
    pub code_coverage: bool,
    pub functional_coverage: bool,
    pub assertion_coverage: bool,
    /// "event_driven" | "cycle_accurate" | "formal"
    pub compilation_model: String,
    pub supports_gui: bool,
    pub supports_save_restore: bool,
    pub typical_compile_speed: String,
    pub typical_sim_speed: String,
}

impl Default for SimulatorCapabilities {
    fn default() -> Self {
        Self {
            systemverilog_full: false,
            vhdl: false,
            mixed_language: false,
            uvm: false,
            fork_join: false,
            force_release: false,
            bind_statements: false,
            code_coverage: false,
            functional_coverage: false,
            assertion_coverage: false,
            compilation_model: "event_driven".to_string(),
            supports_gui: false,
            supports_save_restore: false,
            typical_compile_speed: "fast".to_string(),
            typical_sim_speed: "medium".to_string(),
        }
    }
}

/// Options for simulation.
#[derive(Debug, Clone, Default)]
pub struct SimOptions {
    pub top_module: String,
    pub defines: Vec<String>,
    pub plusargs: Vec<String>,
    pub seed: Option<u64>,
    pub timeout_secs: Option<u64>,
    pub enable_coverage: bool,
    pub gui: bool,
    pub waves: bool,
    pub extra_args: Vec<String>,
}

/// Result of compilation step.
#[derive(Debug, Clone)]
pub struct CompileResult {
    pub success: bool,
    pub log_path: PathBuf,
    pub work_dir: PathBuf,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Result of elaboration step.
#[derive(Debug, Clone)]
pub struct ElaborateResult {
    pub success: bool,
    pub log_path: PathBuf,
    pub snapshot: String,
    pub errors: Vec<String>,
}

/// Result of simulation step.
#[derive(Debug, Clone)]
pub struct SimResult {
    pub success: bool,
    pub exit_code: i32,
    pub log_path: PathBuf,
    pub coverage_db: Option<PathBuf>,
    pub duration_secs: f64,
    pub errors: Vec<String>,
}

/// Simulation report with pass/fail and optional coverage.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimReport {
    pub test_name: String,
    pub passed: bool,
    pub duration_secs: f64,
    pub error_count: usize,
    pub warning_count: usize,
    pub coverage: Option<CoverageReport>,
}

/// Coverage report data.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CoverageReport {
    pub line_coverage: Option<f64>,
    pub toggle_coverage: Option<f64>,
    pub branch_coverage: Option<f64>,
    pub functional_coverage: Option<f64>,
}

/// Requirements a test declares for simulator compatibility.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SimRequirements {
    #[serde(default)]
    pub uvm: bool,
    #[serde(default)]
    pub fork_join: bool,
    #[serde(default)]
    pub force_release: bool,
    #[serde(default)]
    pub vhdl: bool,
    #[serde(default)]
    pub mixed_language: bool,
}

impl SimRequirements {
    /// Check if a simulator's capabilities satisfy these requirements.
    pub fn is_compatible_with(&self, caps: &SimulatorCapabilities) -> Vec<String> {
        let mut incompatibilities = Vec::new();
        if self.uvm && !caps.uvm {
            incompatibilities.push("requires UVM support".to_string());
        }
        if self.fork_join && !caps.fork_join {
            incompatibilities.push("requires fork/join support".to_string());
        }
        if self.force_release && !caps.force_release {
            incompatibilities.push("requires force/release support".to_string());
        }
        if self.vhdl && !caps.vhdl {
            incompatibilities.push("requires VHDL support".to_string());
        }
        if self.mixed_language && !caps.mixed_language {
            incompatibilities.push("requires mixed-language support".to_string());
        }
        incompatibilities
    }
}

/// The interface a simulation backend must implement.
pub trait SimulatorPlugin: Send + Sync {
    fn plugin_name(&self) -> &str;

    fn capabilities(&self) -> SimulatorCapabilities;

    fn check_environment(
        &self,
        required_version: Option<&str>,
    ) -> Result<EnvironmentStatus, LoomError>;

    fn compile(
        &self,
        filesets: &AssembledFilesets,
        options: &SimOptions,
        context: &BuildContext,
    ) -> Result<CompileResult, LoomError>;

    fn elaborate(
        &self,
        compile_result: &CompileResult,
        top_module: &str,
        options: &SimOptions,
        context: &BuildContext,
    ) -> Result<ElaborateResult, LoomError>;

    fn simulate(
        &self,
        elaborate_result: &ElaborateResult,
        options: &SimOptions,
        context: &BuildContext,
    ) -> Result<SimResult, LoomError>;

    fn extract_results(&self, sim_result: &SimResult) -> Result<SimReport, LoomError>;

    fn merge_coverage(
        &self,
        _coverage_dbs: &[PathBuf],
        _output: &Path,
    ) -> Result<CoverageReport, LoomError> {
        Err(LoomError::Internal(
            "Coverage merging not supported by this simulator".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sim_requirements_compatible() {
        let reqs = SimRequirements::default();
        let caps = SimulatorCapabilities::default();
        assert!(reqs.is_compatible_with(&caps).is_empty());
    }

    #[test]
    fn test_sim_requirements_incompatible_uvm() {
        let reqs = SimRequirements {
            uvm: true,
            ..Default::default()
        };
        let caps = SimulatorCapabilities::default();
        let issues = reqs.is_compatible_with(&caps);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("UVM"));
    }

    #[test]
    fn test_sim_requirements_multiple_incompatible() {
        let reqs = SimRequirements {
            uvm: true,
            fork_join: true,
            vhdl: true,
            ..Default::default()
        };
        let caps = SimulatorCapabilities::default();
        let issues = reqs.is_compatible_with(&caps);
        assert_eq!(issues.len(), 3);
    }

    #[test]
    fn test_sim_requirements_all_satisfied() {
        let reqs = SimRequirements {
            uvm: true,
            fork_join: true,
            force_release: true,
            vhdl: true,
            mixed_language: true,
        };
        let caps = SimulatorCapabilities {
            uvm: true,
            fork_join: true,
            force_release: true,
            vhdl: true,
            mixed_language: true,
            ..Default::default()
        };
        assert!(reqs.is_compatible_with(&caps).is_empty());
    }
}
