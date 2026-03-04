use std::path::PathBuf;
use std::process::Command;

use loom_core::error::LoomError;
use loom_core::plugin::backend::EnvironmentStatus;

/// Check the Verilator installation.
pub fn check_verilator_environment(
    required_version: Option<&str>,
) -> Result<EnvironmentStatus, LoomError> {
    let (path, version) = find_verilator()?;

    let version_matches = match required_version {
        None => true,
        Some(req) => version.starts_with(req),
    };

    Ok(EnvironmentStatus {
        tool_name: "verilator".to_string(),
        tool_path: path,
        version,
        required_version: required_version.map(|s| s.to_string()),
        version_matches,
        license_ok: true,
        license_detail: Some("Verilator is open source (LGPL)".to_string()),
        warnings: vec![],
    })
}

fn find_verilator() -> Result<(PathBuf, String), LoomError> {
    let which_cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };

    if let Ok(output) = Command::new(which_cmd).arg("verilator").output() {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !path_str.is_empty() {
                let path = PathBuf::from(&path_str);
                let version = query_version().unwrap_or_else(|_| "unknown".to_string());
                return Ok((path, version));
            }
        }
    }

    Err(LoomError::ToolNotFound {
        tool: "verilator".to_string(),
        message: "Verilator not found on PATH. Install from https://verilator.org".to_string(),
    })
}

fn query_version() -> Result<String, String> {
    let output = Command::new("verilator")
        .arg("--version")
        .output()
        .map_err(|e| e.to_string())?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output like "Verilator 5.024 2024-04-05 rev v5.024"
    parse_verilator_version(&stdout).ok_or_else(|| "Could not parse version".to_string())
}

fn parse_verilator_version(output: &str) -> Option<String> {
    for line in output.lines() {
        if line.starts_with("Verilator") {
            return line.split_whitespace().nth(1).map(|s| s.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_verilator_version() {
        let output = "Verilator 5.024 2024-04-05 rev v5.024";
        assert_eq!(parse_verilator_version(output), Some("5.024".to_string()));
    }
}
