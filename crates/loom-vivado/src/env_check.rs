use std::path::PathBuf;
use std::process::Command;

use loom_core::error::LoomError;
use loom_core::plugin::backend::EnvironmentStatus;

/// Check the Vivado installation and return a detailed status.
pub fn check_vivado_environment(
    required_version: Option<&str>,
) -> Result<EnvironmentStatus, LoomError> {
    let (vivado_path, found_version) = find_vivado_with_version()?;

    let version_matches = match required_version {
        None => true,
        Some(req) => versions_compatible(&found_version, req),
    };

    let (license_ok, license_detail) = check_license_availability();

    Ok(EnvironmentStatus {
        tool_name: "vivado".to_string(),
        tool_path: vivado_path,
        version: found_version,
        required_version: required_version.map(|s| s.to_string()),
        version_matches,
        license_ok,
        license_detail: Some(license_detail),
        warnings: vec![],
    })
}

fn find_vivado_with_version() -> Result<(PathBuf, String), LoomError> {
    let candidates = get_vivado_candidates();

    for path in candidates {
        if !path.exists() {
            continue;
        }
        if let Ok(version) = query_vivado_version(&path) {
            return Ok((path, version));
        }
    }

    Err(LoomError::ToolNotFound {
        tool: "vivado".to_string(),
        message:
            "Vivado not found. Check VIVADO_PATH, standard installation paths, or add to PATH."
                .to_string(),
    })
}

fn get_vivado_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(val) = std::env::var("VIVADO_PATH") {
        candidates.push(PathBuf::from(val));
    }

    if let Ok(val) = std::env::var("XILINX_VIVADO") {
        let bin = PathBuf::from(val).join("bin").join(vivado_exe_name());
        candidates.push(bin);
    }

    candidates.extend(find_standard_installations());

    if let Some(path) = find_on_path() {
        candidates.push(path);
    }

    candidates
}

fn query_vivado_version(
    vivado_path: &std::path::Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(vivado_path).arg("-version").output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_vivado_version_string(&stdout)
        .ok_or_else(|| format!("Could not parse Vivado version from: {}", stdout).into())
}

fn parse_vivado_version_string(output: &str) -> Option<String> {
    for line in output.lines() {
        if line.starts_with("Vivado v") {
            let version = line
                .trim_start_matches("Vivado v")
                .split_whitespace()
                .next()?;
            return Some(version.to_string());
        }
    }
    None
}

fn versions_compatible(found: &str, required: &str) -> bool {
    found == required
}

fn check_license_availability() -> (bool, String) {
    let has_license_var =
        std::env::var("LM_LICENSE_FILE").is_ok() || std::env::var("XILINXD_LICENSE_FILE").is_ok();

    if has_license_var {
        (true, "License server environment variable set".to_string())
    } else {
        (
            true,
            "License assumed available (set LM_LICENSE_FILE to verify)".to_string(),
        )
    }
}

fn vivado_exe_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "vivado.bat"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "vivado"
    }
}

fn find_standard_installations() -> Vec<PathBuf> {
    let mut found = Vec::new();

    let base_dirs: Vec<&str> = if cfg!(target_os = "windows") {
        vec![r"C:\Xilinx\Vivado", r"C:\tools\Xilinx\Vivado"]
    } else {
        vec![
            "/tools/Xilinx/Vivado",
            "/opt/Xilinx/Vivado",
            "/home/Xilinx/Vivado",
        ]
    };

    for base_dir in base_dirs {
        let base = PathBuf::from(base_dir);
        if !base.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&base) {
            let mut versions: Vec<PathBuf> = entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.path().join("bin").join(vivado_exe_name()))
                .filter(|p| p.exists())
                .collect();
            versions.sort_by(|a, b| b.cmp(a));
            found.extend(versions);
        }
    }

    found
}

fn find_on_path() -> Option<PathBuf> {
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("which")
            .arg("vivado")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                let path_str = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if path_str.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(path_str))
                }
            })
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("where")
            .arg("vivado")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                let path_str = String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if path_str.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(path_str))
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_vivado_version_string() {
        let output = "Vivado v2023.2 (64-bit)\n\
                      SW Build 4029153 on Fri Oct 13 20:13:54 MDT 2023";
        let version = parse_vivado_version_string(output);
        assert_eq!(version, Some("2023.2".to_string()));
    }

    #[test]
    fn test_parse_vivado_version_2024() {
        let output = "Vivado v2024.1 (64-bit)\nSW Build 12345678";
        let version = parse_vivado_version_string(output);
        assert_eq!(version, Some("2024.1".to_string()));
    }

    #[test]
    fn test_versions_compatible_exact_match() {
        assert!(versions_compatible("2023.2", "2023.2"));
        assert!(!versions_compatible("2024.1", "2023.2"));
    }
}
