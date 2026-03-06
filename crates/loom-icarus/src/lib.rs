pub mod env_check;

use std::path::{Path, PathBuf};

use loom_core::assemble::fileset::{AssembledFilesets, FileLanguage};
use loom_core::build::context::BuildContext;
use loom_core::error::LoomError;
use loom_core::plugin::backend::EnvironmentStatus;
use loom_core::plugin::simulator::{
    CompileResult, CoverageReport, ElaborateResult, SimOptions, SimReport, SimResult,
    SimulatorCapabilities, SimulatorPlugin,
};
use loom_core::util::{tool_arg, tool_command};

pub struct IcarusBackend;

impl SimulatorPlugin for IcarusBackend {
    fn plugin_name(&self) -> &str {
        "icarus"
    }

    fn capabilities(&self) -> SimulatorCapabilities {
        SimulatorCapabilities {
            systemverilog_full: false,
            vhdl: false,
            mixed_language: false,
            uvm: false,
            fork_join: false,
            force_release: true,
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

    fn check_environment(
        &self,
        required_version: Option<&str>,
    ) -> Result<EnvironmentStatus, LoomError> {
        env_check::check_icarus_environment(required_version)
    }

    fn compile(
        &self,
        filesets: &AssembledFilesets,
        options: &SimOptions,
        context: &BuildContext,
    ) -> Result<CompileResult, LoomError> {
        let sim_dir = context.build_dir.join("sim_icarus");
        std::fs::create_dir_all(&sim_dir).map_err(|e| LoomError::Io {
            path: sim_dir.clone(),
            source: e,
        })?;

        let log_path = sim_dir.join("compile.log");
        let output_vvp = sim_dir.join("sim.vvp");

        let mut cmd = tool_command("iverilog");
        cmd.arg("-o")
            .arg(output_vvp.display().to_string())
            .arg("-g2012") // Enable SystemVerilog (basic support)
            .arg("-s")
            .arg(&options.top_module)
            .current_dir(&sim_dir);

        for define in &options.defines {
            tool_arg(&mut cmd, &format!("-D{}", define));
        }

        // Add source files (Verilog/SV only) — include sim files for testbenches
        for file in filesets.synth_files.iter().chain(filesets.sim_files.iter()) {
            if matches!(
                file.language,
                FileLanguage::SystemVerilog | FileLanguage::Verilog
            ) {
                cmd.arg(loom_core::util::to_tool_path(&file.path));
            }
        }

        let output = cmd.output().map_err(|e| LoomError::ToolNotFound {
            tool: "iverilog".to_string(),
            message: e.to_string(),
        })?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        let errors: Vec<String> = stderr
            .lines()
            .filter(|l| l.contains("error"))
            .map(|l| l.to_string())
            .collect();
        let warnings: Vec<String> = stderr
            .lines()
            .filter(|l| l.contains("warning"))
            .map(|l| l.to_string())
            .collect();

        Ok(CompileResult {
            success: output.status.success(),
            log_path,
            work_dir: sim_dir,
            errors,
            warnings,
        })
    }

    fn elaborate(
        &self,
        compile_result: &CompileResult,
        _top_module: &str,
        _options: &SimOptions,
        _context: &BuildContext,
    ) -> Result<ElaborateResult, LoomError> {
        // Icarus Verilog combines compile+elaborate in iverilog
        Ok(ElaborateResult {
            success: compile_result.success,
            log_path: compile_result.log_path.clone(),
            snapshot: "sim.vvp".to_string(),
            errors: compile_result.errors.clone(),
        })
    }

    fn simulate(
        &self,
        elaborate_result: &ElaborateResult,
        options: &SimOptions,
        _context: &BuildContext,
    ) -> Result<SimResult, LoomError> {
        let sim_dir = elaborate_result.log_path.parent().unwrap_or(Path::new("."));
        let log_path = sim_dir.join("simulate.log");
        let vvp_file = sim_dir.join("sim.vvp");

        let start = std::time::Instant::now();

        let mut cmd = tool_command("vvp");
        cmd.arg(vvp_file.display().to_string()).current_dir(sim_dir);

        for plusarg in &options.plusargs {
            tool_arg(&mut cmd, &format!("+{}", plusarg));
        }

        let output = cmd
            .output()
            .map_err(|e| LoomError::Internal(format!("Failed to run vvp: {}", e)))?;

        let duration = start.elapsed().as_secs_f64();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let errors: Vec<String> = stdout
            .lines()
            .filter(|l| l.contains("ERROR") || l.contains("FATAL"))
            .map(|l| l.to_string())
            .collect();

        Ok(SimResult {
            success: output.status.success(),
            exit_code: output.status.code().unwrap_or(-1),
            log_path: log_path.to_path_buf(),
            coverage_db: None,
            duration_secs: duration,
            errors,
        })
    }

    fn extract_results(&self, sim_result: &SimResult) -> Result<SimReport, LoomError> {
        Ok(SimReport {
            test_name: "icarus_run".to_string(),
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
            "Icarus Verilog does not support coverage merging".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icarus_capabilities() {
        let backend = IcarusBackend;
        let caps = backend.capabilities();
        assert!(!caps.uvm);
        assert!(!caps.vhdl);
        assert!(!caps.systemverilog_full);
        assert!(caps.force_release);
        assert_eq!(caps.compilation_model, "event_driven");
    }
}
