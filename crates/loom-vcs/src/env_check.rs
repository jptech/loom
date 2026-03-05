use std::path::PathBuf;
use std::process::Command;

use loom_core::error::LoomError;
use loom_core::plugin::backend::EnvironmentStatus;

/// Check the Synopsys VCS installation.
pub fn check_vcs_environment(
    required_version: Option<&str>,
) -> Result<EnvironmentStatus, LoomError> {
    let (vcs_path, found_version) = find_vcs_with_version()?;

    let version_matches = match required_version {
        None => true,
        Some(req) => found_version.starts_with(req) || found_version == req,
    };

    let (license_ok, license_detail) = check_license_availability();

    Ok(EnvironmentStatus {
        tool_name: "vcs".to_string(),
        tool_path: vcs_path,
        version: found_version,
        required_version: required_version.map(|s| s.to_string()),
        version_matches,
        license_ok,
        license_detail: Some(license_detail),
        warnings: vec![],
    })
}

fn find_vcs_with_version() -> Result<(PathBuf, String), LoomError> {
    // Check VCS_HOME env var
    if let Ok(vcs_home) = std::env::var("VCS_HOME") {
        let vcs = PathBuf::from(&vcs_home).join("bin").join("vcs");
        if vcs.exists() {
            if let Ok(version) = query_vcs_version(&vcs) {
                return Ok((vcs, version));
            }
        }
    }

    // Try PATH
    let cmd_name = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };

    if let Ok(output) = Command::new(cmd_name).arg("vcs").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !path.is_empty() {
                if let Ok(version) = query_vcs_version(&PathBuf::from(&path)) {
                    return Ok((PathBuf::from(path), version));
                }
            }
        }
    }

    Err(LoomError::ToolNotFound {
        tool: "vcs".to_string(),
        message: "Synopsys VCS not found. Set VCS_HOME or add vcs to PATH.".to_string(),
    })
}

fn query_vcs_version(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(path).arg("-ID").output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_vcs_version(&stdout)
        .ok_or_else(|| format!("Could not parse VCS version from: {}", stdout).into())
}

pub fn parse_vcs_version(output: &str) -> Option<String> {
    // "vcs script version : T-2022.06-SP2-3"
    // "Chronologic VCS (TM) Version T-2022.06-SP2-3"
    let re = regex::Regex::new(r"([A-Z]-\d{4}\.\d{2}(?:-\S+)?)").ok()?;
    for line in output.lines() {
        if line.contains("Version") || line.contains("version") {
            if let Some(cap) = re.captures(line) {
                return Some(cap[1].to_string());
            }
        }
    }
    None
}

fn check_license_availability() -> (bool, String) {
    let has_license =
        std::env::var("LM_LICENSE_FILE").is_ok() || std::env::var("SNPSLMD_LICENSE_FILE").is_ok();

    if has_license {
        (true, "License server environment variable set".to_string())
    } else {
        (
            false,
            "No license server configured (set LM_LICENSE_FILE or SNPSLMD_LICENSE_FILE)"
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_vcs_version() {
        let output = "Chronologic VCS (TM) Version T-2022.06-SP2-3 -- Wed Dec 7";
        assert_eq!(
            parse_vcs_version(output),
            Some("T-2022.06-SP2-3".to_string())
        );
    }
}
