use std::process::Command;

use loom_core::build::context::BuildContext;
use loom_core::error::LoomError;
use loom_core::plugin::simulator::{CompileResult, ElaborateResult, SimOptions};

/// Elaborate using xelab.
pub fn elaborate_xsim(
    compile_result: &CompileResult,
    top_module: &str,
    _options: &SimOptions,
    _context: &BuildContext,
) -> Result<ElaborateResult, LoomError> {
    let sim_dir = &compile_result.work_dir;
    let log_path = sim_dir.join("elaborate.log");
    let snapshot = format!("{}_snap", top_module);

    let mut cmd = Command::new("xelab");
    cmd.arg(top_module)
        .arg("-s")
        .arg(&snapshot)
        .arg("--debug")
        .arg("typical")
        .current_dir(sim_dir);

    let output = cmd.output().map_err(|e| LoomError::ToolNotFound {
        tool: "xelab".to_string(),
        message: e.to_string(),
    })?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let errors: Vec<String> = stderr
        .lines()
        .filter(|l| l.contains("ERROR"))
        .map(|l| l.to_string())
        .collect();

    Ok(ElaborateResult {
        success: output.status.success(),
        log_path,
        snapshot,
        errors,
    })
}

/// Generate xelab command line for display/logging.
pub fn xelab_command_line(top_module: &str, snapshot: &str) -> String {
    format!("xelab {} -s {} --debug typical", top_module, snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xelab_command_line() {
        let cmd = xelab_command_line("tb_top", "tb_top_snap");
        assert!(cmd.contains("xelab"));
        assert!(cmd.contains("tb_top"));
        assert!(cmd.contains("-s tb_top_snap"));
    }
}
