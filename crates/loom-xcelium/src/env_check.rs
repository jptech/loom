use std::path::PathBuf;
use std::process::Command;

use loom_core::error::LoomError;
use loom_core::plugin::backend::EnvironmentStatus;

/// Check the Cadence Xcelium installation.
pub fn check_xcelium_environment(
    required_version: Option<&str>,
) -> Result<EnvironmentStatus, LoomError> {
    let (xrun_path, found_version) = find_xcelium_with_version()?;

    let version_matches = match required_version {
        None => true,
        Some(req) => found_version.starts_with(req) || found_version == req,
    };

    let (license_ok, license_detail) = check_license_availability();

    Ok(EnvironmentStatus {
        tool_name: "xcelium".to_string(),
        tool_path: xrun_path,
        version: found_version,
        required_version: required_version.map(|s| s.to_string()),
        version_matches,
        license_ok,
        license_detail: Some(license_detail),
        warnings: vec![],
    })
}

fn find_xcelium_with_version() -> Result<(PathBuf, String), LoomError> {
    // Check CDS_INST_DIR env var
    if let Ok(cds_dir) = std::env::var("CDS_INST_DIR") {
        let xrun = PathBuf::from(&cds_dir)
            .join("tools")
            .join("bin")
            .join("xrun");
        if xrun.exists() {
            if let Ok(version) = query_xrun_version(&xrun) {
                return Ok((xrun, version));
            }
        }
    }

    // Try PATH
    let cmd_name = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };

    if let Ok(output) = Command::new(cmd_name).arg("xrun").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !path.is_empty() {
                if let Ok(version) = query_xrun_version(&PathBuf::from(&path)) {
                    return Ok((PathBuf::from(path), version));
                }
            }
        }
    }

    Err(LoomError::ToolNotFound {
        tool: "xrun".to_string(),
        message: "Cadence Xcelium not found. Set CDS_INST_DIR or add xrun to PATH.".to_string(),
    })
}

fn query_xrun_version(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(path).arg("-version").output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_xrun_version(&stdout)
        .ok_or_else(|| format!("Could not parse xrun version from: {}", stdout).into())
}

pub fn parse_xrun_version(output: &str) -> Option<String> {
    // "TOOL:    xrun    23.09-s003"
    // "xrun(64) 23.09-s003"
    let re = regex::Regex::new(r"(\d{2}\.\d{2}(?:-\S+)?)").ok()?;
    for line in output.lines() {
        if line.contains("xrun") {
            if let Some(cap) = re.captures(line) {
                return Some(cap[1].to_string());
            }
        }
    }
    None
}

fn check_license_availability() -> (bool, String) {
    let has_license =
        std::env::var("LM_LICENSE_FILE").is_ok() || std::env::var("CDS_LIC_FILE").is_ok();

    if has_license {
        (true, "License server environment variable set".to_string())
    } else {
        (
            false,
            "No license server configured (set LM_LICENSE_FILE or CDS_LIC_FILE)".to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_xrun_version() {
        let output = "TOOL:    xrun    23.09-s003\nVersion: 23.09-s003";
        assert_eq!(parse_xrun_version(output), Some("23.09-s003".to_string()));
    }

    #[test]
    fn test_parse_xrun_version_64() {
        let output = "xrun(64) 22.03-s004 ...";
        assert_eq!(parse_xrun_version(output), Some("22.03-s004".to_string()));
    }
}
