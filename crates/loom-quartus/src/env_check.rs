use std::path::PathBuf;
use std::process::Command;

use loom_core::error::LoomError;
use loom_core::plugin::backend::EnvironmentStatus;

/// Check the Quartus installation and return a detailed status.
pub fn check_quartus_environment(
    required_version: Option<&str>,
) -> Result<EnvironmentStatus, LoomError> {
    let (quartus_path, found_version) = find_quartus_with_version()?;

    let version_matches = match required_version {
        None => true,
        Some(req) => versions_compatible(&found_version, req),
    };

    let (license_ok, license_detail) = check_license_availability();

    Ok(EnvironmentStatus {
        tool_name: "quartus_sh".to_string(),
        tool_path: quartus_path,
        version: found_version,
        required_version: required_version.map(|s| s.to_string()),
        version_matches,
        license_ok,
        license_detail: Some(license_detail),
        warnings: vec![],
    })
}

fn find_quartus_with_version() -> Result<(PathBuf, String), LoomError> {
    let candidates = get_quartus_candidates();

    for path in candidates {
        if !path.exists() {
            continue;
        }
        if let Ok(version) = query_quartus_version(&path) {
            return Ok((path, version));
        }
    }

    Err(LoomError::ToolNotFound {
        tool: "quartus_sh".to_string(),
        message:
            "Quartus not found. Check QUARTUS_ROOTDIR, standard installation paths, or add to PATH."
                .to_string(),
    })
}

fn get_quartus_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(val) = std::env::var("QUARTUS_SH_PATH") {
        candidates.push(PathBuf::from(val));
    }

    if let Ok(val) = std::env::var("QUARTUS_ROOTDIR") {
        let bin = PathBuf::from(val).join("bin").join(quartus_exe_name());
        candidates.push(bin);
    }

    candidates.extend(find_standard_installations());

    if let Some(path) = find_on_path() {
        candidates.push(path);
    }

    candidates
}

fn query_quartus_version(
    quartus_path: &std::path::Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(quartus_path).arg("--version").output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_quartus_version_string(&stdout)
        .ok_or_else(|| format!("Could not parse Quartus version from: {}", stdout).into())
}

/// Parse version from Quartus output like:
/// "Quartus Prime Lite Edition Version 23.1std.0 Build 991 ..."
/// or "Quartus Prime Pro Edition Version 23.4 Build ..."
fn parse_quartus_version_string(output: &str) -> Option<String> {
    for line in output.lines() {
        if line.contains("Version") && line.contains("Quartus") {
            // Find "Version X.Y"
            if let Some(idx) = line.find("Version") {
                let after = &line[idx + "Version".len()..].trim_start();
                let version = after.split_whitespace().next()?;
                return Some(version.to_string());
            }
        }
    }
    None
}

fn versions_compatible(found: &str, required: &str) -> bool {
    // Major version match: "23.1std.0" should match "23.1"
    found == required || found.starts_with(required)
}

fn check_license_availability() -> (bool, String) {
    let has_license_var =
        std::env::var("LM_LICENSE_FILE").is_ok() || std::env::var("ALTERAD_LICENSE_FILE").is_ok();

    if has_license_var {
        (true, "License server environment variable set".to_string())
    } else {
        (
            true,
            "License assumed available (Quartus Lite requires no license; for Pro set LM_LICENSE_FILE)".to_string(),
        )
    }
}

fn quartus_exe_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "quartus_sh.exe"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "quartus_sh"
    }
}

fn find_standard_installations() -> Vec<PathBuf> {
    let mut found = Vec::new();

    let base_dirs: Vec<&str> = if cfg!(target_os = "windows") {
        vec![
            r"C:\intelFPGA_lite",
            r"C:\intelFPGA_pro",
            r"C:\intelFPGA",
            r"C:\altera",
        ]
    } else {
        vec![
            "/opt/intelFPGA_lite",
            "/opt/intelFPGA_pro",
            "/opt/intelFPGA",
            "/opt/altera",
            "/tools/intelFPGA_lite",
            "/tools/intelFPGA_pro",
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
                .filter_map(|e| {
                    let quartus_dir = e.path().join("quartus");
                    if quartus_dir.is_dir() {
                        let bin = quartus_dir.join("bin").join(quartus_exe_name());
                        if bin.exists() {
                            return Some(bin);
                        }
                    }
                    None
                })
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
            .arg("quartus_sh")
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
            .arg("quartus_sh")
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
    fn test_parse_quartus_version_lite() {
        let output =
            "Quartus Prime Lite Edition Version 23.1std.0 Build 991 11/28/2023 SJ Lite Edition";
        let version = parse_quartus_version_string(output);
        assert_eq!(version, Some("23.1std.0".to_string()));
    }

    #[test]
    fn test_parse_quartus_version_pro() {
        let output = "Quartus Prime Pro Edition Version 23.4 Build 123";
        let version = parse_quartus_version_string(output);
        assert_eq!(version, Some("23.4".to_string()));
    }

    #[test]
    fn test_versions_compatible_exact() {
        assert!(versions_compatible("23.1std.0", "23.1std.0"));
        assert!(!versions_compatible("23.4", "23.1"));
    }

    #[test]
    fn test_versions_compatible_prefix() {
        assert!(versions_compatible("23.1std.0", "23.1"));
    }
}
