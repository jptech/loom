use std::path::PathBuf;
use std::process::Command;

use loom_core::error::LoomError;
use loom_core::plugin::backend::EnvironmentStatus;

/// Check the xsim (Vivado simulator) installation.
pub fn check_xsim_environment(
    required_version: Option<&str>,
) -> Result<EnvironmentStatus, LoomError> {
    let (path, version) = find_xsim_with_version()?;

    let version_matches = match required_version {
        None => true,
        Some(req) => version == req,
    };

    Ok(EnvironmentStatus {
        tool_name: "xsim".to_string(),
        tool_path: path,
        version,
        required_version: required_version.map(|s| s.to_string()),
        version_matches,
        license_ok: true,
        license_detail: Some("xsim included with Vivado license".to_string()),
        warnings: vec![],
    })
}

fn find_xsim_with_version() -> Result<(PathBuf, String), LoomError> {
    // xsim is co-located with vivado in the bin directory
    if let Ok(vivado_path) = std::env::var("XILINX_VIVADO") {
        let xvlog = PathBuf::from(&vivado_path)
            .join("bin")
            .join(xsim_exe_name());
        if xvlog.exists() {
            let version = query_version(&xvlog).unwrap_or_else(|_| "unknown".to_string());
            return Ok((xvlog, version));
        }
    }

    // Try PATH
    let which_cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };

    if let Ok(output) = Command::new(which_cmd).arg("xvlog").output() {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !path_str.is_empty() {
                let path = PathBuf::from(&path_str);
                let version = query_version(&path).unwrap_or_else(|_| "unknown".to_string());
                return Ok((path, version));
            }
        }
    }

    Err(LoomError::ToolNotFound {
        tool: "xsim".to_string(),
        message: "xsim/xvlog not found. Set XILINX_VIVADO or add Vivado bin to PATH.".to_string(),
    })
}

fn query_version(tool_path: &std::path::Path) -> Result<String, String> {
    let output = Command::new(tool_path)
        .arg("--version")
        .output()
        .map_err(|e| e.to_string())?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("Vivado Simulator") || line.contains("xvlog") {
            if let Some(version) = line.split_whitespace().find(|w| w.starts_with("v20")) {
                return Ok(version.trim_start_matches('v').to_string());
            }
        }
    }
    Ok("unknown".to_string())
}

fn xsim_exe_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "xvlog.bat"
    } else {
        "xvlog"
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_exe_name() {
        let name = super::xsim_exe_name();
        assert!(name.contains("xvlog"));
    }
}
