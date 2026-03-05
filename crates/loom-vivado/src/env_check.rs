use std::path::PathBuf;
use std::process::Command;

use loom_core::error::LoomError;
use loom_core::plugin::backend::EnvironmentStatus;

/// Check the Vivado installation and return a detailed status.
pub fn check_vivado_environment(
    required_version: Option<&str>,
) -> Result<EnvironmentStatus, LoomError> {
    let mut diagnostics = Vec::new();
    let (vivado_path, found_version) = find_vivado_with_version(&mut diagnostics)?;

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

/// Find Vivado executable on this system.
///
/// Search order:
///   1. VIVADO_PATH env var (direct path to executable)
///   2. XILINX_VIVADO env var ($XILINX_VIVADO/bin/vivado)
///   3. Standard installation directories (C:\Xilinx, C:\AMD, /tools/Xilinx, etc.)
///   4. System PATH (via `where` on Windows, `which` on Unix)
pub fn find_vivado_executable() -> Result<PathBuf, LoomError> {
    let mut diagnostics = Vec::new();
    find_vivado_with_version(&mut diagnostics).map(|(path, _version)| path)
}

fn find_vivado_with_version(diagnostics: &mut Vec<String>) -> Result<(PathBuf, String), LoomError> {
    let candidates = get_vivado_candidates(diagnostics);

    for (path, source) in &candidates {
        if !path.exists() {
            diagnostics.push(format!("  {} -> {} (not found)", source, path.display()));
            continue;
        }
        match query_vivado_version(path) {
            Ok(version) => {
                diagnostics.push(format!("  {} -> {} (v{})", source, path.display(), version));
                return Ok((path.clone(), version));
            }
            Err(e) => {
                diagnostics.push(format!(
                    "  {} -> {} (exists but version query failed: {})",
                    source,
                    path.display(),
                    e
                ));
            }
        }
    }

    let search_details = if diagnostics.is_empty() {
        "  (no candidates found)".to_string()
    } else {
        diagnostics.join("\n")
    };

    Err(LoomError::ToolNotFound {
        tool: "vivado".to_string(),
        message: format!(
            "Vivado not found.\n\
             \n\
             Search details:\n\
             {}\n\
             \n\
             To fix, do one of the following:\n\
             - Set VIVADO_PATH to the vivado executable (e.g. C:\\AMD\\2025.2\\Vivado\\bin\\vivado.bat)\n\
             - Set XILINX_VIVADO to the Vivado root (e.g. C:\\AMD\\2025.2\\Vivado)\n\
             - Add the Vivado bin directory to your PATH\n\
             - Install Vivado in a standard location (C:\\Xilinx\\Vivado or C:\\AMD)",
            search_details
        ),
    })
}

/// Returns (candidate_path, source_description) pairs.
fn get_vivado_candidates(diagnostics: &mut Vec<String>) -> Vec<(PathBuf, String)> {
    let mut candidates = Vec::new();

    // 1. VIVADO_PATH — direct path to executable
    match std::env::var("VIVADO_PATH") {
        Ok(val) => {
            candidates.push((PathBuf::from(&val), "VIVADO_PATH".to_string()));
        }
        Err(_) => diagnostics.push("  VIVADO_PATH: not set".to_string()),
    }

    // 2. XILINX_VIVADO — root directory, append bin/vivado
    match std::env::var("XILINX_VIVADO") {
        Ok(val) => {
            let bin = PathBuf::from(&val).join("bin").join(vivado_exe_name());
            candidates.push((bin, format!("XILINX_VIVADO={}", val)));
        }
        Err(_) => diagnostics.push("  XILINX_VIVADO: not set".to_string()),
    }

    // 3. Standard installation directories
    let standard = find_standard_installations(diagnostics);
    for path in standard {
        candidates.push((path.clone(), format!("standard: {}", path.display())));
    }

    // 4. System PATH lookup
    match find_on_path() {
        FindOnPathResult::Found(path) => {
            candidates.push((path.clone(), format!("PATH: {}", path.display())));
        }
        FindOnPathResult::NotFound => {
            diagnostics.push("  PATH lookup: 'vivado' not found on system PATH".to_string());
        }
        FindOnPathResult::Error(msg) => {
            diagnostics.push(format!("  PATH lookup: {}", msg));
        }
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
        // Handle both Xilinx-era "Vivado v2023.2" and AMD-era "vivado v2025.2"
        let lower = line.to_lowercase();
        if lower.starts_with("vivado v") {
            let version = line[8..] // skip "vivado v" / "Vivado v"
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

fn find_standard_installations(diagnostics: &mut Vec<String>) -> Vec<PathBuf> {
    let mut found = Vec::new();

    // Xilinx-era layout: {base}/Vivado/{version}/bin/vivado
    //   e.g. C:\Xilinx\Vivado\2023.2\bin\vivado.bat
    let xilinx_base_dirs: Vec<&str> = if cfg!(target_os = "windows") {
        vec![r"C:\Xilinx\Vivado", r"C:\tools\Xilinx\Vivado"]
    } else {
        vec![
            "/tools/Xilinx/Vivado",
            "/opt/Xilinx/Vivado",
            "/home/Xilinx/Vivado",
        ]
    };

    for base_dir in xilinx_base_dirs {
        scan_versioned_subdirs(base_dir, "bin", diagnostics, &mut found);
    }

    // AMD-era layout (2024+): {base}/{version}/Vivado/bin/vivado
    //   e.g. C:\AMD\2025.2\Vivado\bin\vivado.bat
    //   e.g. /tools/AMD/2025.2/Vivado/bin/vivado
    let amd_base_dirs: Vec<&str> = if cfg!(target_os = "windows") {
        vec![r"C:\AMD", r"C:\tools\AMD"]
    } else {
        vec!["/tools/AMD", "/opt/AMD", "/home/AMD"]
    };

    for base_dir in amd_base_dirs {
        scan_amd_layout(base_dir, diagnostics, &mut found);
    }

    found
}

/// Scan Xilinx-era layout: {base}/{version}/bin/vivado
fn scan_versioned_subdirs(
    base_dir: &str,
    bin_subdir: &str,
    diagnostics: &mut Vec<String>,
    found: &mut Vec<PathBuf>,
) {
    let base = PathBuf::from(base_dir);
    if !base.exists() {
        return;
    }
    diagnostics.push(format!("  scanning {}", base_dir));
    match std::fs::read_dir(&base) {
        Ok(entries) => {
            let mut versions: Vec<PathBuf> = entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.path().join(bin_subdir).join(vivado_exe_name()))
                .collect();
            versions.sort_by(|a, b| b.cmp(a)); // newest first
            found.extend(versions);
        }
        Err(e) => {
            diagnostics.push(format!("  scanning {}: read error: {}", base_dir, e));
        }
    }
}

/// Scan AMD-era layout: {base}/{version}/Vivado/bin/vivado
fn scan_amd_layout(base_dir: &str, diagnostics: &mut Vec<String>, found: &mut Vec<PathBuf>) {
    let base = PathBuf::from(base_dir);
    if !base.exists() {
        return;
    }
    diagnostics.push(format!("  scanning {} (AMD layout)", base_dir));
    match std::fs::read_dir(&base) {
        Ok(entries) => {
            let mut versions: Vec<PathBuf> = entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.path().join("Vivado").join("bin").join(vivado_exe_name()))
                .collect();
            versions.sort_by(|a, b| b.cmp(a)); // newest first
            found.extend(versions);
        }
        Err(e) => {
            diagnostics.push(format!("  scanning {}: read error: {}", base_dir, e));
        }
    }
}

enum FindOnPathResult {
    Found(PathBuf),
    NotFound,
    Error(String),
}

fn find_on_path() -> FindOnPathResult {
    #[cfg(not(target_os = "windows"))]
    {
        match Command::new("which").arg("vivado").output() {
            Ok(output) => {
                if output.status.success() {
                    let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if path_str.is_empty() {
                        FindOnPathResult::NotFound
                    } else {
                        FindOnPathResult::Found(PathBuf::from(path_str))
                    }
                } else {
                    FindOnPathResult::NotFound
                }
            }
            Err(e) => FindOnPathResult::Error(format!("failed to run 'which': {}", e)),
        }
    }
    #[cfg(target_os = "windows")]
    {
        // Try both 'where vivado' and 'where vivado.bat' — some environments
        // resolve differently depending on PATHEXT configuration.
        for query in &["vivado", "vivado.bat"] {
            match Command::new("where.exe").arg(query).output() {
                Ok(output) => {
                    if output.status.success() {
                        let path_str = String::from_utf8_lossy(&output.stdout)
                            .lines()
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if !path_str.is_empty() {
                            return FindOnPathResult::Found(PathBuf::from(path_str));
                        }
                    }
                }
                Err(e) => {
                    return FindOnPathResult::Error(format!(
                        "failed to run 'where.exe {}': {}",
                        query, e
                    ));
                }
            }
        }
        FindOnPathResult::NotFound
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
    fn test_parse_vivado_version_amd_lowercase() {
        // AMD-era Vivado (2025+) uses lowercase "vivado v..."
        let output = "vivado v2025.2 (64-bit)\nTool Version Limit: 2025.11\nSW Build 6299465";
        let version = parse_vivado_version_string(output);
        assert_eq!(version, Some("2025.2".to_string()));
    }

    #[test]
    fn test_versions_compatible_exact_match() {
        assert!(versions_compatible("2023.2", "2023.2"));
        assert!(!versions_compatible("2024.1", "2023.2"));
    }

    #[test]
    fn test_diagnostics_populated_on_failure() {
        // When no Vivado is installed, the error should contain diagnostic details
        let mut diagnostics = Vec::new();
        let result = find_vivado_with_version(&mut diagnostics);
        if let Err(LoomError::ToolNotFound { message, .. }) = result {
            assert!(
                message.contains("Search details:"),
                "Error should contain search details"
            );
            assert!(
                message.contains("To fix"),
                "Error should contain remediation steps"
            );
        }
        // If Vivado IS installed, the test trivially passes
    }
}
