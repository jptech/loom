use std::path::PathBuf;
use std::process::Command;

use loom_core::error::LoomError;
use loom_core::plugin::backend::EnvironmentStatus;

/// Check the Icarus Verilog installation.
pub fn check_icarus_environment(
    required_version: Option<&str>,
) -> Result<EnvironmentStatus, LoomError> {
    let (iverilog_path, found_version) = find_icarus_with_version()?;

    let version_matches = match required_version {
        None => true,
        Some(req) => found_version.starts_with(req) || found_version == req,
    };

    Ok(EnvironmentStatus {
        tool_name: "icarus".to_string(),
        tool_path: iverilog_path,
        version: found_version,
        required_version: required_version.map(|s| s.to_string()),
        version_matches,
        license_ok: true,
        license_detail: Some("Open source — no license required".to_string()),
        warnings: vec![],
    })
}

fn find_icarus_with_version() -> Result<(PathBuf, String), LoomError> {
    // Try iverilog on PATH
    let cmd_name = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };

    if let Ok(output) = Command::new(cmd_name).arg("iverilog").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !path.is_empty() {
                if let Ok(version) = query_iverilog_version(&PathBuf::from(&path)) {
                    return Ok((PathBuf::from(path), version));
                }
            }
        }
    }

    Err(LoomError::ToolNotFound {
        tool: "iverilog".to_string(),
        message: "Icarus Verilog not found. Install with: apt install iverilog (Linux), brew install icarus-verilog (macOS), or download from iverilog.icarus.com (Windows).".to_string(),
    })
}

fn query_iverilog_version(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(path).arg("-V").output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_iverilog_version(&stdout)
        .ok_or_else(|| format!("Could not parse iverilog version from: {}", stdout).into())
}

pub fn parse_iverilog_version(output: &str) -> Option<String> {
    // "Icarus Verilog version 12.0 (stable) (v12_0)"
    for line in output.lines() {
        if line.contains("Icarus Verilog version") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                return Some(parts[3].to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_iverilog_version() {
        let output = "Icarus Verilog version 12.0 (stable) (v12_0)\n";
        assert_eq!(parse_iverilog_version(output), Some("12.0".to_string()));
    }

    #[test]
    fn test_parse_iverilog_version_11() {
        let output = "Icarus Verilog version 11.0 (stable) (v11_0)\n";
        assert_eq!(parse_iverilog_version(output), Some("11.0".to_string()));
    }
}
