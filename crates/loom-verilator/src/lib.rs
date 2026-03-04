pub mod env_check;

use std::path::{Path, PathBuf};
use std::process::Command;

use loom_core::assemble::fileset::{AssembledFilesets, FileLanguage};
use loom_core::build::context::BuildContext;
use loom_core::error::LoomError;
use loom_core::plugin::backend::EnvironmentStatus;
use loom_core::plugin::simulator::{
    CompileResult, CoverageReport, ElaborateResult, SimOptions, SimReport, SimResult,
    SimulatorCapabilities, SimulatorPlugin,
};

pub struct VerilatorBackend;

impl SimulatorPlugin for VerilatorBackend {
    fn plugin_name(&self) -> &str {
        "verilator"
    }

    fn capabilities(&self) -> SimulatorCapabilities {
        SimulatorCapabilities {
            systemverilog_full: false,
            vhdl: false,
            mixed_language: false,
            uvm: false,
            fork_join: false,
            force_release: false,
            bind_statements: false,
            code_coverage: true,
            functional_coverage: false,
            assertion_coverage: false,
            compilation_model: "cycle_accurate".to_string(),
            supports_gui: false,
            supports_save_restore: false,
            typical_compile_speed: "fast".to_string(),
            typical_sim_speed: "fast".to_string(),
        }
    }

    fn check_environment(
        &self,
        required_version: Option<&str>,
    ) -> Result<EnvironmentStatus, LoomError> {
        env_check::check_verilator_environment(required_version)
    }

    fn compile(
        &self,
        filesets: &AssembledFilesets,
        options: &SimOptions,
        context: &BuildContext,
    ) -> Result<CompileResult, LoomError> {
        let sim_dir = context.build_dir.join("sim_verilator");
        std::fs::create_dir_all(&sim_dir).map_err(|e| LoomError::Io {
            path: sim_dir.clone(),
            source: e,
        })?;

        let log_path = sim_dir.join("compile.log");

        let mut cmd = Command::new("verilator");
        cmd.arg("--cc")
            .arg("--exe")
            .arg("--build")
            .arg("-j")
            .arg("0")
            .arg("--top-module")
            .arg(&options.top_module)
            .current_dir(&sim_dir);

        if options.enable_coverage {
            cmd.arg("--coverage");
        }

        for define in &options.defines {
            cmd.arg(format!("+define+{}", define));
        }

        // Add source files (Verilog/SV only, no VHDL)
        for file in &filesets.synth_files {
            if matches!(
                file.language,
                FileLanguage::SystemVerilog | FileLanguage::Verilog
            ) {
                cmd.arg(file.path.display().to_string().replace('\\', "/"));
            }
        }

        let output = cmd.output().map_err(|e| LoomError::ToolNotFound {
            tool: "verilator".to_string(),
            message: e.to_string(),
        })?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        let errors: Vec<String> = stderr
            .lines()
            .filter(|l| l.contains("%Error"))
            .map(|l| l.to_string())
            .collect();
        let warnings: Vec<String> = stderr
            .lines()
            .filter(|l| l.contains("%Warning"))
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
        // Verilator combines compile+elaborate in one step
        Ok(ElaborateResult {
            success: compile_result.success,
            log_path: compile_result.log_path.clone(),
            snapshot: "Vtop".to_string(),
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

        // Run the compiled binary
        let binary = sim_dir
            .join("obj_dir")
            .join(format!("V{}", options.top_module));

        let start = std::time::Instant::now();

        let mut cmd = Command::new(&binary);
        cmd.current_dir(sim_dir);

        for plusarg in &options.plusargs {
            cmd.arg(format!("+{}", plusarg));
        }

        let output = cmd.output().map_err(|e| {
            LoomError::Internal(format!(
                "Failed to run Verilator binary at {}: {}",
                binary.display(),
                e
            ))
        })?;

        let duration = start.elapsed().as_secs_f64();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let errors: Vec<String> = stdout
            .lines()
            .filter(|l| l.contains("Error") || l.contains("FATAL"))
            .map(|l| l.to_string())
            .collect();

        Ok(SimResult {
            success: output.status.success(),
            exit_code: output.status.code().unwrap_or(-1),
            log_path: log_path.to_path_buf(),
            coverage_db: if options.enable_coverage {
                Some(sim_dir.join("coverage.dat"))
            } else {
                None
            },
            duration_secs: duration,
            errors,
        })
    }

    fn extract_results(&self, sim_result: &SimResult) -> Result<SimReport, LoomError> {
        Ok(SimReport {
            test_name: "verilator_run".to_string(),
            passed: sim_result.success,
            duration_secs: sim_result.duration_secs,
            error_count: sim_result.errors.len(),
            warning_count: 0,
            coverage: None,
        })
    }

    fn merge_coverage(
        &self,
        coverage_dbs: &[PathBuf],
        output: &Path,
    ) -> Result<CoverageReport, LoomError> {
        // verilator_coverage --write merged.dat file1.dat file2.dat
        let mut cmd = Command::new("verilator_coverage");
        cmd.arg("--write").arg(output.display().to_string());
        for db in coverage_dbs {
            cmd.arg(db.display().to_string());
        }

        let result = cmd
            .output()
            .map_err(|e| LoomError::Internal(format!("verilator_coverage failed: {}", e)))?;

        if !result.status.success() {
            return Err(LoomError::Internal(
                "verilator_coverage merge failed".to_string(),
            ));
        }

        Ok(CoverageReport {
            line_coverage: None,
            toggle_coverage: None,
            branch_coverage: None,
            functional_coverage: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verilator_capabilities() {
        let backend = VerilatorBackend;
        let caps = backend.capabilities();
        assert!(!caps.uvm);
        assert!(!caps.fork_join);
        assert!(!caps.vhdl);
        assert_eq!(caps.compilation_model, "cycle_accurate");
    }
}
