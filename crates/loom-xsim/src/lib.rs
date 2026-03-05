pub mod compile;
pub mod elaborate;
pub mod env_check;
pub mod simulate;

use std::path::{Path, PathBuf};

use loom_core::assemble::fileset::AssembledFilesets;
use loom_core::build::context::BuildContext;
use loom_core::error::LoomError;
use loom_core::plugin::backend::EnvironmentStatus;
use loom_core::plugin::simulator::{
    CompileResult, CoverageReport, ElaborateResult, SimOptions, SimReport, SimResult,
    SimulatorCapabilities, SimulatorPlugin,
};

pub struct XsimBackend;

impl SimulatorPlugin for XsimBackend {
    fn plugin_name(&self) -> &str {
        "xsim"
    }

    fn capabilities(&self) -> SimulatorCapabilities {
        SimulatorCapabilities {
            systemverilog_full: true,
            vhdl: true,
            mixed_language: true,
            uvm: false,
            fork_join: true,
            force_release: true,
            bind_statements: true,
            code_coverage: true,
            functional_coverage: false,
            assertion_coverage: false,
            compilation_model: "event_driven".to_string(),
            supports_gui: true,
            supports_save_restore: true,
            typical_compile_speed: "medium".to_string(),
            typical_sim_speed: "medium".to_string(),
        }
    }

    fn check_environment(
        &self,
        required_version: Option<&str>,
    ) -> Result<EnvironmentStatus, LoomError> {
        env_check::check_xsim_environment(required_version)
    }

    fn compile(
        &self,
        filesets: &AssembledFilesets,
        options: &SimOptions,
        context: &BuildContext,
    ) -> Result<CompileResult, LoomError> {
        compile::compile_xsim(filesets, options, context)
    }

    fn elaborate(
        &self,
        compile_result: &CompileResult,
        top_module: &str,
        options: &SimOptions,
        context: &BuildContext,
    ) -> Result<ElaborateResult, LoomError> {
        elaborate::elaborate_xsim(compile_result, top_module, options, context)
    }

    fn simulate(
        &self,
        elaborate_result: &ElaborateResult,
        options: &SimOptions,
        context: &BuildContext,
    ) -> Result<SimResult, LoomError> {
        simulate::simulate_xsim(elaborate_result, options, context)
    }

    fn extract_results(&self, sim_result: &SimResult) -> Result<SimReport, LoomError> {
        Ok(SimReport {
            test_name: "xsim_run".to_string(),
            passed: sim_result.success,
            duration_secs: sim_result.duration_secs,
            error_count: sim_result.errors.len(),
            warning_count: 0,
            coverage: None,
        })
    }

    fn merge_coverage(
        &self,
        _coverage_dbs: &[PathBuf],
        _output: &Path,
    ) -> Result<CoverageReport, LoomError> {
        Err(LoomError::Internal(
            "xsim coverage merging not yet implemented".to_string(),
        ))
    }
}
