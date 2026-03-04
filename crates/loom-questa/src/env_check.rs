use std::path::PathBuf;
use std::process::Command;

use loom_core::error::LoomError;
use loom_core::plugin::backend::EnvironmentStatus;

/// Check the Questa/ModelSim installation.
pub fn check_questa_environment(
    required_version: Option<&str>,
) -> Result<EnvironmentStatus, LoomError> {
    let (vsim_path, found_version) = find_questa_with_version()?;

    let version_matches = match required_version {
        None => true,
        Some(req) => found_version.starts_with(req) || found_version == req,
    };

    let (license_ok, license_detail) = check_license_availability();

    Ok(EnvironmentStatus {
        tool_name: "questa".to_string(),
        tool_path: vsim_path,
        version: found_version,
        required_version: required_version.map(|s| s.to_string()),
        version_matches,
        license_ok,
        license_detail: Some(license_detail),
        warnings: vec![],
    })
}

fn find_questa_with_version() -> Result<(PathBuf, String), LoomError> {
    // Check MTI_HOME env var
    if let Ok(mti_home) = std::env::var("MTI_HOME") {
        let vsim = PathBuf::from(&mti_home).join("bin").join(vsim_exe());
        if vsim.exists() {
            if let Ok(version) = query_vsim_version(&vsim) {
                return Ok((vsim, version));
            }
        }
    }

    // Check QUESTA_HOME env var
    if let Ok(questa_home) = std::env::var("QUESTA_HOME") {
        let vsim = PathBuf::from(&questa_home).join("bin").join(vsim_exe());
        if vsim.exists() {
            if let Ok(version) = query_vsim_version(&vsim) {
                return Ok((vsim, version));
            }
        }
    }

    // Try PATH
    let cmd_name = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };

    if let Ok(output) = Command::new(cmd_name).arg("vsim").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !path.is_empty() {
                if let Ok(version) = query_vsim_version(&PathBuf::from(&path)) {
                    return Ok((PathBuf::from(path), version));
                }
            }
        }
    }

    Err(LoomError::ToolNotFound {
        tool: "vsim".to_string(),
        message: "Questa/ModelSim not found. Set MTI_HOME or QUESTA_HOME, or add vsim to PATH."
            .to_string(),
    })
}

fn query_vsim_version(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(path).arg("-version").output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_vsim_version(&stdout)
        .ok_or_else(|| format!("Could not parse vsim version from: {}", stdout).into())
}

pub fn parse_vsim_version(output: &str) -> Option<String> {
    // "Model Technology ModelSim - INTEL FPGA STARTER EDITION vsim 2021.2 Simulator"
    // "Questa Intel Starter FPGA Edition-64 vsim 2023.4 Simulator"
    for line in output.lines() {
        if line.contains("vsim") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            for (i, part) in parts.iter().enumerate() {
                if *part == "vsim" && i + 1 < parts.len() {
                    return Some(parts[i + 1].to_string());
                }
            }
        }
    }
    None
}

fn check_license_availability() -> (bool, String) {
    let has_license =
        std::env::var("LM_LICENSE_FILE").is_ok() || std::env::var("MGLS_LICENSE_FILE").is_ok();

    if has_license {
        (true, "License server environment variable set".to_string())
    } else {
        (
            true,
            "License assumed available (set LM_LICENSE_FILE or MGLS_LICENSE_FILE to verify)"
                .to_string(),
        )
    }
}

fn vsim_exe() -> &'static str {
    if cfg!(target_os = "windows") {
        "vsim.exe"
    } else {
        "vsim"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_vsim_version_questa() {
        let output = "Questa Intel Starter FPGA Edition-64 vsim 2023.4 Simulator 2023.10";
        assert_eq!(parse_vsim_version(output), Some("2023.4".to_string()));
    }

    #[test]
    fn test_parse_vsim_version_modelsim() {
        let output = "Model Technology ModelSim - INTEL FPGA STARTER EDITION vsim 2021.2 Simulator";
        assert_eq!(parse_vsim_version(output), Some("2021.2".to_string()));
    }
}
