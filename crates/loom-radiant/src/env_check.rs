use std::path::PathBuf;
use std::process::Command;

use loom_core::error::LoomError;
use loom_core::plugin::backend::EnvironmentStatus;

/// Check the Lattice Radiant installation and return a detailed status.
pub fn check_radiant_environment(
    required_version: Option<&str>,
) -> Result<EnvironmentStatus, LoomError> {
    let (radiant_path, found_version) = find_radiant_with_version()?;

    let version_matches = match required_version {
        None => true,
        Some(req) => versions_compatible(&found_version, req),
    };

    let (license_ok, license_detail) = check_license_availability();

    Ok(EnvironmentStatus {
        tool_name: "radiant".to_string(),
        tool_path: radiant_path,
        version: found_version,
        required_version: required_version.map(|s| s.to_string()),
        version_matches,
        license_ok,
        license_detail: Some(license_detail),
        warnings: vec![],
    })
}

fn find_radiant_with_version() -> Result<(PathBuf, String), LoomError> {
    let candidates = get_radiant_candidates();

    for path in candidates {
        if !path.exists() {
            continue;
        }
        if let Ok(version) = query_radiant_version(&path) {
            return Ok((path, version));
        }
    }

    Err(LoomError::ToolNotFound {
        tool: "radiant".to_string(),
        message: "Lattice Radiant not found. Check standard installation paths or add to PATH."
            .to_string(),
    })
}

fn get_radiant_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(val) = std::env::var("RADIANT_PATH") {
        candidates.push(PathBuf::from(val));
    }

    if let Ok(val) = std::env::var("FOUNDRY") {
        // FOUNDRY typically points to <radiant>/ispfpga
        let bin = PathBuf::from(val)
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("bin")
            .join(radiant_exe());
        candidates.push(bin);
    }

    candidates.extend(find_standard_installations());

    if let Some(path) = find_on_path() {
        candidates.push(path);
    }

    candidates
}

fn query_radiant_version(
    radiant_path: &std::path::Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(radiant_path).arg("--version").output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    parse_radiant_version_string(&combined).ok_or_else(|| {
        "Could not parse Radiant version from output"
            .to_string()
            .into()
    })
}

pub fn parse_radiant_version_string(output: &str) -> Option<String> {
    // Radiant version strings vary; look for common patterns
    // "Lattice Radiant Software 2023.2" or "radiant 2023.2"
    let re = regex::Regex::new(r"(\d+\.\d+(?:\.\d+)?)").ok()?;
    for line in output.lines() {
        let lower = line.to_lowercase();
        if lower.contains("radiant") {
            if let Some(cap) = re.captures(line) {
                return Some(cap[1].to_string());
            }
        }
    }
    None
}

fn versions_compatible(found: &str, required: &str) -> bool {
    found.starts_with(required) || found == required
}

fn check_license_availability() -> (bool, String) {
    let has_license =
        std::env::var("LM_LICENSE_FILE").is_ok() || std::env::var("LATTICE_LICENSE_FILE").is_ok();

    if has_license {
        (true, "License environment variable set".to_string())
    } else {
        (
            true,
            "License assumed available (set LM_LICENSE_FILE to verify)".to_string(),
        )
    }
}

fn radiant_exe() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "radiantc.exe"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "radiantc"
    }
}

fn find_standard_installations() -> Vec<PathBuf> {
    let mut found = Vec::new();

    let base_dirs: Vec<&str> = if cfg!(target_os = "windows") {
        vec![r"C:\lscc\radiant", r"C:\Lattice\Radiant"]
    } else {
        vec!["/usr/local/lscc/radiant", "/opt/lscc/radiant"]
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
                .map(|e| e.path().join("bin").join(radiant_exe()))
                .filter(|p| p.exists())
                .collect();
            versions.sort_by(|a, b| b.cmp(a));
            found.extend(versions);
        }
    }

    found
}

fn find_on_path() -> Option<PathBuf> {
    let cmd_name = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };

    Command::new(cmd_name)
        .arg("radiantc")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_radiant_version() {
        let output = "Lattice Radiant Software 2023.2\nBuild 12345";
        let version = parse_radiant_version_string(output);
        assert_eq!(version, Some("2023.2".to_string()));
    }

    #[test]
    fn test_versions_compatible() {
        assert!(versions_compatible("2023.2", "2023.2"));
        assert!(versions_compatible("2023.2.1", "2023.2"));
        assert!(!versions_compatible("2024.1", "2023.2"));
    }
}
