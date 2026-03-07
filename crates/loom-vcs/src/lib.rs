//! Synopsys VCS simulator backend.
//!
//! **Status: planned.** This crate has a scaffolded `SimulatorPlugin` implementation
//! but has never been run against the actual VCS toolchain. It will likely
//! require fixes before it works correctly.

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
use loom_core::util::{tool_arg, tool_command};

pub struct VcsBackend;

impl SimulatorPlugin for VcsBackend {
    fn plugin_name(&self) -> &str {
        "vcs"
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
        env_check::check_vcs_environment(required_version)
    }

    fn compile(
        &self,
        filesets: &AssembledFilesets,
        options: &SimOptions,
        context: &BuildContext,
    ) -> Result<CompileResult, LoomError> {
        let sim_dir = context.build_dir.join("sim_vcs");
        std::fs::create_dir_all(&sim_dir).map_err(|e| LoomError::Io {
            path: sim_dir.clone(),
            source: e,
        })?;

        let log_path = sim_dir.join("compile.log");

        let mut cmd = tool_command("vcs");
        cmd.arg("-sverilog")
            .arg("-full64")
            .arg("-timescale=1ns/1ps")
            .arg("-top")
            .arg(&options.top_module)
            .arg("-o")
            .arg(sim_dir.join("simv").display().to_string())
            .current_dir(&sim_dir);

        if options.enable_coverage {
            cmd.arg("-cm").arg("line+cond+fsm+tgl+branch+assert");
        }

        for define in &options.defines {
            tool_arg(&mut cmd, &format!("+define+{}", define));
        }

        // Add source files (synth + sim files for testbenches)
        for file in filesets.synth_files.iter().chain(filesets.sim_files.iter()) {
            match file.language {
                FileLanguage::SystemVerilog | FileLanguage::Verilog => {
                    cmd.arg(loom_core::util::to_tool_path(&file.path));
                }
                FileLanguage::Vhdl => {
                    cmd.arg("-vhdl08");
                    cmd.arg(loom_core::util::to_tool_path(&file.path));
                }
                _ => {}
            }
        }

        let output = cmd.output().map_err(|e| LoomError::ToolNotFound {
            tool: "vcs".to_string(),
            message: e.to_string(),
        })?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        let errors: Vec<String> = stderr
            .lines()
            .filter(|l| l.contains("Error-"))
            .map(|l| l.to_string())
            .collect();
        let warnings: Vec<String> = stderr
            .lines()
            .filter(|l| l.contains("Warning-"))
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
        // VCS combines compile+elaborate in one step
        Ok(ElaborateResult {
            success: compile_result.success,
            log_path: compile_result.log_path.clone(),
            snapshot: "simv".to_string(),
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

        let start = std::time::Instant::now();

        let simv = sim_dir.join("simv");
        let mut cmd = Command::new(simv.display().to_string());
        cmd.current_dir(sim_dir);

        if options.enable_coverage {
            cmd.arg("-cm").arg("line+cond+fsm+tgl+branch+assert");
        }

        for plusarg in &options.plusargs {
            cmd.arg(format!("+{}", plusarg));
        }

        if let Some(seed) = options.seed {
            cmd.arg(format!("+ntb_random_seed={}", seed));
        }

        let output = cmd
            .output()
            .map_err(|e| LoomError::Internal(format!("Failed to run VCS simv: {}", e)))?;

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
                Some(sim_dir.join("simv.vdb"))
            } else {
                None
            },
            duration_secs: duration,
            errors,
        })
    }

    fn extract_results(&self, sim_result: &SimResult) -> Result<SimReport, LoomError> {
        Ok(SimReport {
            test_name: "vcs_run".to_string(),
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
        // urg -dir db1.vdb db2.vdb -dbname merged
        let mut cmd = tool_command("urg");
        for db in coverage_dbs {
            cmd.arg("-dir").arg(db.display().to_string());
        }
        cmd.arg("-dbname").arg(output.display().to_string());

        let result = cmd
            .output()
            .map_err(|e| LoomError::Internal(format!("urg merge failed: {}", e)))?;

        if !result.status.success() {
            return Err(LoomError::Internal("urg merge failed".to_string()));
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
    fn test_vcs_capabilities() {
        let backend = VcsBackend;
        let caps = backend.capabilities();
        assert!(caps.uvm);
        assert!(caps.vhdl);
        assert!(caps.systemverilog_full);
        assert!(caps.fork_join);
        assert!(caps.code_coverage);
        assert!(caps.functional_coverage);
    }
}
