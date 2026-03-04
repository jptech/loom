use std::path::PathBuf;
use std::process::Command;

use loom_core::error::LoomError;
use loom_core::plugin::backend::EnvironmentStatus;

/// Check yosys installation.
pub fn check_yosys_environment(
    required_version: Option<&str>,
) -> Result<EnvironmentStatus, LoomError> {
    let (path, version) = find_yosys()?;

    let version_matches = match required_version {
        None => true,
        Some(req) => version.starts_with(req),
    };

    Ok(EnvironmentStatus {
        tool_name: "yosys".to_string(),
        tool_path: path,
        version,
        required_version: required_version.map(|s| s.to_string()),
        version_matches,
        license_ok: true,
        license_detail: Some("yosys is open source (ISC)".to_string()),
        warnings: vec![],
    })
}

fn find_yosys() -> Result<(PathBuf, String), LoomError> {
    let which_cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };

    if let Ok(output) = Command::new(which_cmd).arg("yosys").output() {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !path_str.is_empty() {
                let path = PathBuf::from(&path_str);
                let version = query_yosys_version().unwrap_or_else(|_| "unknown".to_string());
                return Ok((path, version));
            }
        }
    }

    Err(LoomError::ToolNotFound {
        tool: "yosys".to_string(),
        message: "yosys not found on PATH. Install from https://github.com/YosysHQ/yosys"
            .to_string(),
    })
}

fn query_yosys_version() -> Result<String, String> {
    let output = Command::new("yosys")
        .arg("--version")
        .output()
        .map_err(|e| e.to_string())?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_yosys_version(&stdout).ok_or_else(|| "Could not parse yosys version".to_string())
}

fn parse_yosys_version(output: &str) -> Option<String> {
    // "Yosys 0.40 (git sha1 abc123)"
    for line in output.lines() {
        if line.starts_with("Yosys") {
            return line.split_whitespace().nth(1).map(|s| s.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_yosys_version() {
        let output = "Yosys 0.40 (git sha1 abc123def)";
        assert_eq!(parse_yosys_version(output), Some("0.40".to_string()));
    }
}
