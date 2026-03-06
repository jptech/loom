use loom_core::build::context::BuildContext;
use loom_core::error::LoomError;
use loom_core::plugin::simulator::{ElaborateResult, SimOptions, SimResult};
use loom_core::util::{tool_arg, tool_command};

/// Run simulation using xsim.
pub fn simulate_xsim(
    elaborate_result: &ElaborateResult,
    options: &SimOptions,
    _context: &BuildContext,
) -> Result<SimResult, LoomError> {
    let sim_dir = elaborate_result
        .log_path
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let log_path = sim_dir.join("simulate.log");

    let start = std::time::Instant::now();

    let mut cmd = tool_command("xsim");
    cmd.arg(&elaborate_result.snapshot)
        .arg("--runall")
        .current_dir(sim_dir);

    for plusarg in &options.plusargs {
        cmd.arg("--testplusarg");
        tool_arg(&mut cmd, plusarg);
    }

    let output = cmd.output().map_err(|e| LoomError::ToolNotFound {
        tool: "xsim".to_string(),
        message: e.to_string(),
    })?;

    let duration = start.elapsed().as_secs_f64();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let errors: Vec<String> = stdout
        .lines()
        .chain(stderr.lines())
        .filter(|l| l.contains("Error") || l.contains("FATAL"))
        .map(|l| l.to_string())
        .collect();

    Ok(SimResult {
        success: output.status.success() && errors.is_empty(),
        exit_code: output.status.code().unwrap_or(-1),
        log_path: log_path.to_path_buf(),
        coverage_db: None,
        duration_secs: duration,
        errors,
    })
}

/// Generate xsim command line for display/logging.
pub fn xsim_command_line(snapshot: &str) -> String {
    format!("xsim {} --runall", snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xsim_command_line() {
        let cmd = xsim_command_line("tb_top_snap");
        assert!(cmd.contains("xsim"));
        assert!(cmd.contains("tb_top_snap"));
        assert!(cmd.contains("--runall"));
    }
}
