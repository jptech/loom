# Task 12: Vivado Environment Check

**Prerequisites:** Task 11 complete
**Goal:** Implement `check_vivado_environment()` — find Vivado, detect its version, verify it matches the project requirement, and check license availability.

## Spec Reference
`system_plan.md` §13.1 (Tool Version Enforcement), §13.2 (Tool Discovery)

## File to Implement
`crates/loom-vivado/src/env_check.rs`

## Tool Discovery Priority (from spec §13.2)

1. **Explicit configuration** — `workspace.toml` `[settings.tools.vivado] path = "..."` (Phase 1: not yet implemented, check later phases)
2. **Environment variable** — `VIVADO_PATH` or `XILINX_VIVADO`
3. **Standard installation paths** — `/tools/Xilinx/Vivado/<version>/bin/vivado` (Linux), `C:\Xilinx\Vivado\<version>\bin\vivado.bat` (Windows)
4. **PATH** — system PATH search

## Implementation

```rust
use std::path::{Path, PathBuf};
use std::process::Command;
use loom_core::plugin::backend::EnvironmentStatus;
use loom_core::error::LoomError;

/// Check the Vivado installation and return a detailed status.
pub fn check_vivado_environment(
    required_version: Option<&str>,
) -> Result<EnvironmentStatus, LoomError> {
    // 1. Find Vivado executable
    let (vivado_path, found_version) = find_vivado_with_version()?;

    // 2. Check version match
    let version_matches = match required_version {
        None => true,  // No requirement → any version matches
        Some(req) => versions_compatible(&found_version, req),
    };

    // 3. Check license
    let (license_ok, license_detail) = check_license_availability();

    let mut warnings = Vec::new();
    if !version_matches {
        // Don't warn here — the caller (validate_pre_build) will turn this into an error
    }

    Ok(EnvironmentStatus {
        tool_name: "vivado".to_string(),
        tool_path: vivado_path,
        version: found_version,
        required_version: required_version.map(|s| s.to_string()),
        version_matches,
        license_ok,
        license_detail: Some(license_detail),
        warnings,
    })
}

/// Find Vivado executable and query its version string.
fn find_vivado_with_version() -> Result<(PathBuf, String), LoomError> {
    // Priority order from spec §13.2
    let candidates = get_vivado_candidates();

    let not_found_err = || LoomError::ToolNotFound {
        tool: "vivado".to_string(),
        message: "Vivado not found. Check VIVADO_PATH, standard paths, or add to PATH.".to_string(),
    };

    for path in candidates {
        if !path.exists() { continue; }
        if let Ok(version) = query_vivado_version(&path) {
            return Ok((path, version));
        }
    }

    Err(not_found_err())
}

/// Get all candidate Vivado paths in priority order.
fn get_vivado_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    // 1. VIVADO_PATH environment variable
    if let Ok(val) = std::env::var("VIVADO_PATH") {
        candidates.push(PathBuf::from(val));
    }

    // 2. XILINX_VIVADO environment variable (older convention)
    if let Ok(val) = std::env::var("XILINX_VIVADO") {
        let bin = PathBuf::from(val).join("bin").join(vivado_exe_name());
        candidates.push(bin);
    }

    // 3. Standard installation paths — sorted by version descending
    candidates.extend(find_standard_installations());

    // 4. PATH search
    if let Some(path) = find_on_path() {
        candidates.push(path);
    }

    candidates
}

/// Run `vivado -version` and extract the version string.
fn query_vivado_version(vivado_path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(vivado_path)
        .arg("-version")
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Vivado -version output: "Vivado v2023.2 (64-bit)"
    parse_vivado_version_string(&stdout)
        .ok_or_else(|| format!("Could not parse Vivado version from: {}", stdout).into())
}

/// Parse "Vivado v2023.2 (64-bit)" → "2023.2"
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

/// Check if the found version satisfies the requirement.
/// Phase 1: exact match only. Phase 3+: semver-style range matching.
fn versions_compatible(found: &str, required: &str) -> bool {
    // For Phase 1: exact string match (e.g., "2023.2" == "2023.2")
    // Vivado versions aren't semver, they're YY.N (2023.2, 2024.1, etc.)
    found == required
}

/// Check if a Vivado license is available.
/// Returns (ok, detail_message).
/// Phase 1: basic check — we try a minimal Vivado invocation or check LM_LICENSE_FILE.
fn check_license_availability() -> (bool, String) {
    // Check if LM_LICENSE_FILE or XILINXD_LICENSE_FILE is set
    let has_license_var = std::env::var("LM_LICENSE_FILE").is_ok()
        || std::env::var("XILINXD_LICENSE_FILE").is_ok();

    if has_license_var {
        (true, "License server environment variable set".to_string())
    } else {
        // On many systems, Vivado uses a local license. We can't verify without
        // actually running Vivado, so we assume OK but warn.
        (true, "License assumed available (set LM_LICENSE_FILE to verify)".to_string())
    }
}

fn vivado_exe_name() -> &'static str {
    #[cfg(target_os = "windows")]
    { "vivado.bat" }
    #[cfg(not(target_os = "windows"))]
    { "vivado" }
}

/// Scan standard Xilinx installation directories for Vivado versions.
fn find_standard_installations() -> Vec<PathBuf> {
    let mut found = Vec::new();

    let base_dirs: Vec<&str> = if cfg!(target_os = "windows") {
        vec![r"C:\Xilinx\Vivado", r"C:\tools\Xilinx\Vivado"]
    } else {
        vec!["/tools/Xilinx/Vivado", "/opt/Xilinx/Vivado", "/home/Xilinx/Vivado"]
    };

    for base_dir in base_dirs {
        let base = PathBuf::from(base_dir);
        if !base.exists() { continue; }
        if let Ok(entries) = std::fs::read_dir(&base) {
            let mut versions: Vec<PathBuf> = entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.path().join("bin").join(vivado_exe_name()))
                .filter(|p| p.exists())
                .collect();
            // Sort descending by version (directory name) for newest-first
            versions.sort_by(|a, b| b.cmp(a));
            found.extend(versions);
        }
    }

    found
}

/// Search system PATH for vivado.
fn find_on_path() -> Option<PathBuf> {
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("which").arg("vivado").output().ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                let path_str = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if path_str.is_empty() { None } else { Some(PathBuf::from(path_str)) }
            })
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("where").arg("vivado").output().ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                let path_str = String::from_utf8_lossy(&o.stdout)
                    .lines().next().unwrap_or("").trim().to_string();
                if path_str.is_empty() { None } else { Some(PathBuf::from(path_str)) }
            })
    }
}
```

## Tests

```rust
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

    #[test]
    fn test_vivado_path_from_env() {
        // Set a fake VIVADO_PATH and verify it appears in candidates
        std::env::set_var("VIVADO_PATH", "/usr/bin/fake-vivado");
        let candidates = get_vivado_candidates();
        assert!(candidates.iter().any(|p| p == &PathBuf::from("/usr/bin/fake-vivado")));
    }

    #[test]
    #[ignore = "requires Vivado installation"]
    fn test_check_environment_with_real_vivado() {
        let status = check_vivado_environment(None).unwrap();
        assert!(!status.version.is_empty());
        // Can't assert version_matches without knowing what's installed
    }
}
```

## Done When

- `cargo test -p loom-vivado` passes (non-Vivado tests)
- `parse_vivado_version_string()` correctly extracts version from Vivado output
- `get_vivado_candidates()` returns VIVADO_PATH first, then standard paths
- `versions_compatible()` correctly handles exact match
- `check_vivado_environment()` returns `EnvironmentStatus` with `version_matches = false` when versions differ
