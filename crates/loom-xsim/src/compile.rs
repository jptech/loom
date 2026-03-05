use std::path::PathBuf;
use std::process::Command;

use loom_core::assemble::fileset::{AssembledFilesets, FileLanguage};
use loom_core::build::context::BuildContext;
use loom_core::error::LoomError;
use loom_core::plugin::simulator::{CompileResult, SimOptions};

/// Compile sources using xvlog (Verilog/SV) and xvhdl (VHDL).
pub fn compile_xsim(
    filesets: &AssembledFilesets,
    options: &SimOptions,
    context: &BuildContext,
) -> Result<CompileResult, LoomError> {
    let sim_dir = context.build_dir.join("sim");
    std::fs::create_dir_all(&sim_dir).map_err(|e| LoomError::Io {
        path: sim_dir.clone(),
        source: e,
    })?;

    let log_path = sim_dir.join("compile.log");
    let mut all_errors = Vec::new();
    let mut all_warnings = Vec::new();

    // Collect files by language
    let sv_files: Vec<_> = filesets
        .synth_files
        .iter()
        .filter(|f| {
            matches!(
                f.language,
                FileLanguage::SystemVerilog | FileLanguage::Verilog
            )
        })
        .collect();

    let vhdl_files: Vec<_> = filesets
        .synth_files
        .iter()
        .filter(|f| matches!(f.language, FileLanguage::Vhdl))
        .collect();

    // Compile SystemVerilog/Verilog files with xvlog
    if !sv_files.is_empty() {
        let mut cmd = Command::new("xvlog");
        cmd.arg("--sv").current_dir(&sim_dir);

        for define in &options.defines {
            cmd.arg("-d").arg(define);
        }

        for file in &sv_files {
            cmd.arg(loom_core::util::to_tool_path(&file.path));
        }

        let output = cmd.output().map_err(|e| LoomError::ToolNotFound {
            tool: "xvlog".to_string(),
            message: e.to_string(),
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        for line in stdout.lines().chain(stderr.lines()) {
            if line.contains("ERROR") {
                all_errors.push(line.to_string());
            } else if line.contains("WARNING") {
                all_warnings.push(line.to_string());
            }
        }

        if !output.status.success() {
            return Ok(CompileResult {
                success: false,
                log_path,
                work_dir: sim_dir,
                errors: all_errors,
                warnings: all_warnings,
            });
        }
    }

    // Compile VHDL files with xvhdl
    if !vhdl_files.is_empty() {
        let mut cmd = Command::new("xvhdl");
        cmd.current_dir(&sim_dir);

        for file in &vhdl_files {
            cmd.arg(loom_core::util::to_tool_path(&file.path));
        }

        let output = cmd.output().map_err(|e| LoomError::ToolNotFound {
            tool: "xvhdl".to_string(),
            message: e.to_string(),
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            all_errors.push(stderr.to_string());
            return Ok(CompileResult {
                success: false,
                log_path,
                work_dir: sim_dir,
                errors: all_errors,
                warnings: all_warnings,
            });
        }
    }

    Ok(CompileResult {
        success: true,
        log_path,
        work_dir: sim_dir,
        errors: all_errors,
        warnings: all_warnings,
    })
}

/// Generate xvlog command line for display/logging.
pub fn xvlog_command_line(files: &[PathBuf], defines: &[String]) -> String {
    let mut parts = vec!["xvlog".to_string(), "--sv".to_string()];
    for d in defines {
        parts.push("-d".to_string());
        parts.push(d.clone());
    }
    for f in files {
        parts.push(f.display().to_string());
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xvlog_command_line() {
        let files = vec![PathBuf::from("src/top.sv"), PathBuf::from("src/sub.sv")];
        let defines = vec!["DEBUG=1".to_string()];
        let cmd = xvlog_command_line(&files, &defines);
        assert!(cmd.contains("xvlog"));
        assert!(cmd.contains("--sv"));
        assert!(cmd.contains("-d DEBUG=1"));
        assert!(cmd.contains("src/top.sv"));
    }
}
