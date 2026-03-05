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

pub struct XceliumBackend;

impl SimulatorPlugin for XceliumBackend {
    fn plugin_name(&self) -> &str {
        "xcelium"
    }

    fn capabilities(&self) -> SimulatorCapabilities {
        SimulatorCapabilities {
            systemverilog_full: true,
            vhdl: true,
            mixed_language: true,
            uvm: true,
            fork_join: true,
            force_release: true,
            bind_statements: true,
            code_coverage: true,
            functional_coverage: true,
            assertion_coverage: true,
            compilation_model: "event_driven".to_string(),
            supports_gui: true,
            supports_save_restore: true,
            typical_compile_speed: "fast".to_string(),
            typical_sim_speed: "fast".to_string(),
        }
    }

    fn check_environment(
        &self,
        required_version: Option<&str>,
    ) -> Result<EnvironmentStatus, LoomError> {
        env_check::check_xcelium_environment(required_version)
    }

    fn compile(
        &self,
        filesets: &AssembledFilesets,
        options: &SimOptions,
        context: &BuildContext,
    ) -> Result<CompileResult, LoomError> {
        let sim_dir = context.build_dir.join("sim_xcelium");
        std::fs::create_dir_all(&sim_dir).map_err(|e| LoomError::Io {
            path: sim_dir.clone(),
            source: e,
        })?;

        let log_path = sim_dir.join("compile.log");

        // Xcelium uses xrun for single-step compile+elaborate+simulate,
        // but we split into phases for the trait interface.
        // Compile with xmvlog/xmvhdl
        let sv_files: Vec<String> = filesets
            .synth_files
            .iter()
            .filter(|f| {
                matches!(
                    f.language,
                    FileLanguage::SystemVerilog | FileLanguage::Verilog
                )
            })
            .map(|f| loom_core::util::to_tool_path(&f.path))
            .collect();

        if !sv_files.is_empty() {
            let mut cmd = Command::new("xmvlog");
            cmd.arg("-sv").current_dir(&sim_dir);

            for define in &options.defines {
                cmd.arg(format!("-define {}", define));
            }

            for f in &sv_files {
                cmd.arg(f);
            }

            let output = cmd.output().map_err(|e| LoomError::ToolNotFound {
                tool: "xmvlog".to_string(),
                message: e.to_string(),
            })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let errors: Vec<String> = stderr
                    .lines()
                    .filter(|l| l.contains("*E"))
                    .map(|l| l.to_string())
                    .collect();
                return Ok(CompileResult {
                    success: false,
                    log_path,
                    work_dir: sim_dir,
                    errors,
                    warnings: vec![],
                });
            }
        }

        // Compile VHDL
        let vhdl_files: Vec<String> = filesets
            .synth_files
            .iter()
            .filter(|f| matches!(f.language, FileLanguage::Vhdl))
            .map(|f| loom_core::util::to_tool_path(&f.path))
            .collect();

        if !vhdl_files.is_empty() {
            let mut cmd = Command::new("xmvhdl");
            cmd.arg("-v200x").current_dir(&sim_dir);

            for f in &vhdl_files {
                cmd.arg(f);
            }

            let output = cmd.output().map_err(|e| LoomError::ToolNotFound {
                tool: "xmvhdl".to_string(),
                message: e.to_string(),
            })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let errors: Vec<String> = stderr
                    .lines()
                    .filter(|l| l.contains("*E"))
                    .map(|l| l.to_string())
                    .collect();
                return Ok(CompileResult {
                    success: false,
                    log_path,
                    work_dir: sim_dir,
                    errors,
                    warnings: vec![],
                });
            }
        }

        Ok(CompileResult {
            success: true,
            log_path,
            work_dir: sim_dir,
            errors: vec![],
            warnings: vec![],
        })
    }

    fn elaborate(
        &self,
        compile_result: &CompileResult,
        top_module: &str,
        options: &SimOptions,
        _context: &BuildContext,
    ) -> Result<ElaborateResult, LoomError> {
        let sim_dir = &compile_result.work_dir;
        let log_path = sim_dir.join("elaborate.log");

        let mut cmd = Command::new("xmelab");
        cmd.arg(top_module).current_dir(sim_dir);

        if options.enable_coverage {
            cmd.arg("-coverage").arg("all");
        }

        let output = cmd.output().map_err(|e| LoomError::ToolNotFound {
            tool: "xmelab".to_string(),
            message: e.to_string(),
        })?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        let errors: Vec<String> = stderr
            .lines()
            .filter(|l| l.contains("*E"))
            .map(|l| l.to_string())
            .collect();

        Ok(ElaborateResult {
            success: output.status.success(),
            log_path,
            snapshot: top_module.to_string(),
            errors,
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

        let start = std::time::Instant::now();

        let mut cmd = Command::new("xmsim");
        cmd.arg(&elaborate_result.snapshot).current_dir(sim_dir);

        if options.enable_coverage {
            cmd.arg("-coverage").arg("all");
        }

        for plusarg in &options.plusargs {
            cmd.arg(format!("+{}", plusarg));
        }

        if let Some(seed) = options.seed {
            cmd.arg("-svseed").arg(seed.to_string());
        }

        let output = cmd
            .output()
            .map_err(|e| LoomError::Internal(format!("Failed to run xmsim: {}", e)))?;

        let duration = start.elapsed().as_secs_f64();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let errors: Vec<String> = stdout
            .lines()
            .filter(|l| l.contains("*E") || l.contains("*F"))
            .map(|l| l.to_string())
            .collect();

        Ok(SimResult {
            success: output.status.success() && errors.is_empty(),
            exit_code: output.status.code().unwrap_or(-1),
            log_path: log_path.to_path_buf(),
            coverage_db: if options.enable_coverage {
                Some(sim_dir.join("cov_work"))
            } else {
                None
            },
            duration_secs: duration,
            errors,
        })
    }

    fn extract_results(&self, sim_result: &SimResult) -> Result<SimReport, LoomError> {
        Ok(SimReport {
            test_name: "xcelium_run".to_string(),
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
        // imc -exec merge.tcl
        let mut cmd = Command::new("imc");
        cmd.arg("-batch").arg("-exec").arg(format!(
            "merge {} -out {}",
            coverage_dbs
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(" "),
            output.display()
        ));

        let result = cmd
            .output()
            .map_err(|e| LoomError::Internal(format!("imc merge failed: {}", e)))?;

        if !result.status.success() {
            return Err(LoomError::Internal("imc merge failed".to_string()));
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
    fn test_xcelium_capabilities() {
        let backend = XceliumBackend;
        let caps = backend.capabilities();
        assert!(caps.uvm);
        assert!(caps.vhdl);
        assert!(caps.systemverilog_full);
        assert!(caps.fork_join);
        assert!(caps.code_coverage);
        assert!(caps.functional_coverage);
    }
}
