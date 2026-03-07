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
use loom_core::util::{scan_sim_output, tool_arg, tool_command, write_sim_log};

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

        let is_cocotb = options
            .extra_args
            .iter()
            .any(|a| a.starts_with("COCOTB_TEST_MODULES=") || a.starts_with("COCOTB_MODULE="));

        let mut cmd = tool_command("verilator");

        if is_cocotb {
            // cocotb needs its own C++ main (verilator.cpp) that implements
            // a VPI event loop with cbAfterDelay support. Verilator's --binary
            // main doesn't have this, so we use --cc --exe with cocotb's main.
            let share_dir = loom_core::sim::compat::cocotb_share_dir().ok_or_else(|| {
                LoomError::Internal(
                    "cocotb-config --share failed; reinstall cocotb".to_string(),
                )
            })?;
            let verilator_cpp = format!("{}/lib/verilator/verilator.cpp", share_dir);
            let include_dir = format!("{}/include", share_dir);

            cmd.arg("--cc")
                .arg("--exe")
                .arg("--vpi")
                .arg("--public-flat-rw")
                .arg("--prefix")
                .arg("Vtop");

            // cocotb's verilator.cpp is the main entry point
            cmd.arg(&verilator_cpp);
            cmd.arg("-CFLAGS").arg(format!("-I{}", include_dir));

            // Link the cocotb VPI library. --no-as-needed ensures the linker
            // keeps it even though no symbols are directly referenced (VPI
            // registers via constructor attributes).
            if let Some(lib_dir) = loom_core::sim::compat::cocotb_lib_dir() {
                cmd.arg("-LDFLAGS").arg(format!(
                    "-Wl,-rpath,{lib_dir} -L{lib_dir} -Wl,--no-as-needed -lcocotbvpi_verilator"
                ));
            }

            cmd.arg("+define+COCOTB_SIM=1");
        } else {
            cmd.arg("--binary");
        }

        cmd.arg("--timing")
            .arg("-j")
            .arg("0")
            .arg("-Wno-fatal")
            .arg("-Wno-WIDTHTRUNC")
            .arg("-Wno-WIDTHEXPAND")
            .arg("--top-module")
            .arg(&options.top_module)
            .current_dir(&sim_dir);

        if options.waves {
            cmd.arg("--trace");
        }

        if options.enable_coverage {
            cmd.arg("--coverage");
        }

        for define in &options.defines {
            tool_arg(&mut cmd, &format!("+define+{}", define));
        }

        // Output binary name — cocotb uses --prefix Vtop so the makefile is Vtop.mk
        let binary_name = format!("V{}", options.top_module);
        cmd.arg("-o").arg(&binary_name);

        // Add source files (Verilog/SV only, no VHDL) — include sim files for testbenches
        for file in filesets.synth_files.iter().chain(filesets.sim_files.iter()) {
            if matches!(
                file.language,
                FileLanguage::SystemVerilog | FileLanguage::Verilog
            ) {
                cmd.arg(loom_core::util::to_tool_path(&file.path));
            }
        }

        let output = cmd.output().map_err(|e| LoomError::ToolNotFound {
            tool: "verilator".to_string(),
            message: e.to_string(),
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // For cocotb (--cc --exe), verilator only generates C++ and a Makefile.
        // We need to run make to actually build the binary.
        if is_cocotb && output.status.success() {
            let mk_name = "Vtop.mk";
            let make_out = Command::new("make")
                .arg("-C")
                .arg(sim_dir.join("obj_dir"))
                .arg("-f")
                .arg(mk_name)
                .arg("-j")
                .output()
                .map_err(|e| LoomError::ToolNotFound {
                    tool: "make".to_string(),
                    message: e.to_string(),
                })?;

            let make_stdout = String::from_utf8_lossy(&make_out.stdout);
            let make_stderr = String::from_utf8_lossy(&make_out.stderr);

            // Append make output to the log
            let combined_stdout = format!("{}\n{}", stdout, make_stdout);
            let combined_stderr = format!("{}\n{}", stderr, make_stderr);
            write_sim_log(&log_path, &combined_stdout, &combined_stderr);

            if !make_out.status.success() {
                let errors: Vec<String> = make_stderr
                    .lines()
                    .filter(|l| l.contains("error:") || l.contains("Error"))
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
        } else {
            write_sim_log(&log_path, &stdout, &stderr);
        }
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

        let is_cocotb = options
            .extra_args
            .iter()
            .any(|a| a.starts_with("COCOTB_TEST_MODULES=") || a.starts_with("COCOTB_MODULE="));

        let mut cmd = Command::new(&binary);
        cmd.current_dir(sim_dir);

        for plusarg in &options.plusargs {
            cmd.arg(format!("+{}", plusarg));
        }

        for arg in &options.extra_args {
            if let Some((key, value)) = arg.split_once('=') {
                cmd.env(key, value);
            } else {
                cmd.arg(arg);
            }
        }

        // cocotb needs several environment variables for VPI initialization
        if is_cocotb {
            cmd.env("TOPLEVEL_LANG", "verilog");

            // GPI_USERS: tells the VPI library where to find the cocotb GPI entry point
            if let Ok(out) = Command::new("cocotb-config")
                .arg("--pygpi-entry-point")
                .output()
            {
                if out.status.success() {
                    let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !val.is_empty() {
                        cmd.env("GPI_USERS", &val);
                    }
                }
            }
            if let Ok(out) = Command::new("cocotb-config")
                .arg("--python-bin")
                .output()
            {
                if out.status.success() {
                    let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !val.is_empty() {
                        cmd.env("PYGPI_PYTHON_BIN", &val);
                    }
                }
            }
            if let Ok(out) = Command::new("cocotb-config")
                .arg("--libpython")
                .output()
            {
                if out.status.success() {
                    let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !val.is_empty() {
                        cmd.env("LIBPYTHON_LOC", &val);
                    }
                }
            }
            if let Some(lib_dir) = loom_core::sim::compat::cocotb_lib_dir() {
                cmd.env("LD_LIBRARY_PATH", &lib_dir);
            }
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
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = if stderr.is_empty() {
            stdout.to_string()
        } else {
            format!("{}\n{}", stdout, stderr)
        };
        write_sim_log(&log_path, &stdout, &stderr);

        let scan = scan_sim_output(&combined);
        let success = scan.is_pass(output.status.success());
        let mut errors: Vec<String> = scan
            .fail_lines
            .into_iter()
            .chain(scan.error_lines)
            .collect();
        if !success && errors.is_empty() {
            if scan.empty_output {
                errors.push("simulation produced no output (silent failure)".to_string());
            } else if !output.status.success() {
                errors.push(format!(
                    "simulation exited with code {}",
                    output.status.code().unwrap_or(-1)
                ));
            } else {
                errors.push(
                    "no PASS/FAIL/$finish found in output — add self-checking to your testbench"
                        .to_string(),
                );
            }
        }

        Ok(SimResult {
            success,
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
        let warning_count = if sim_result.log_path.exists() {
            let log = std::fs::read_to_string(&sim_result.log_path).unwrap_or_default();
            scan_sim_output(&log).warning_count
        } else {
            0
        };
        Ok(SimReport {
            test_name: "verilator_run".to_string(),
            passed: sim_result.success,
            duration_secs: sim_result.duration_secs,
            error_count: sim_result.errors.len(),
            warning_count,
            coverage: None,
        })
    }

    fn merge_coverage(
        &self,
        coverage_dbs: &[PathBuf],
        output: &Path,
    ) -> Result<CoverageReport, LoomError> {
        // verilator_coverage --write merged.dat file1.dat file2.dat
        let mut cmd = tool_command("verilator_coverage");
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
