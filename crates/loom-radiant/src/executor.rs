use std::path::PathBuf;
use std::process::Command;

use loom_core::build::context::BuildContext;
use loom_core::error::LoomError;
use loom_core::plugin::backend::BuildResult;

/// Run the Radiant build via radiantc batch mode.
pub fn run_radiant_batch(
    scripts: &[PathBuf],
    context: &BuildContext,
) -> Result<BuildResult, LoomError> {
    if scripts.is_empty() {
        return Err(LoomError::Internal("No build scripts provided".to_string()));
    }

    let script = &scripts[0];
    let radiant_path = find_radiant_executable()?;
    let log_path = context.build_dir.join("radiant_build.log");

    let output = Command::new(&radiant_path)
        .arg("script")
        .arg(script.display().to_string())
        .current_dir(&context.build_dir)
        .output()
        .map_err(|e| LoomError::ToolNotFound {
            tool: "radiantc".to_string(),
            message: e.to_string(),
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    // Write log
    let _ = std::fs::write(&log_path, &combined);

    let errors: Vec<String> = combined
        .lines()
        .filter(|l| l.contains("ERROR") || l.contains("Error:"))
        .map(|l| l.to_string())
        .collect();

    let _warnings: Vec<String> = combined
        .lines()
        .filter(|l| l.contains("WARNING") || l.contains("Warning:"))
        .map(|l| l.to_string())
        .collect();

    let completed_phases = detect_completed_phases(&combined);

    let bitstream = detect_bitstream(&context.build_dir);
    let failure_phase = if !errors.is_empty() {
        completed_phases.last().cloned()
    } else {
        None
    };
    let failure_message = if !errors.is_empty() {
        Some(errors.join("\n"))
    } else {
        None
    };

    Ok(BuildResult {
        success: output.status.success() && errors.is_empty(),
        exit_code: output.status.code().unwrap_or(-1),
        log_paths: vec![log_path],
        bitstream_path: bitstream.into_iter().next(),
        phases_completed: completed_phases,
        failure_phase,
        failure_message,
    })
}

fn find_radiant_executable() -> Result<PathBuf, LoomError> {
    // Check env vars first
    if let Ok(val) = std::env::var("RADIANT_PATH") {
        let path = PathBuf::from(val);
        if path.exists() {
            return Ok(path);
        }
    }

    // Try PATH
    let exe_name = if cfg!(target_os = "windows") {
        "radiantc.exe"
    } else {
        "radiantc"
    };

    let cmd_name = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };

    if let Ok(output) = Command::new(cmd_name).arg(exe_name).output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }

    Err(LoomError::ToolNotFound {
        tool: "radiantc".to_string(),
        message: "Lattice Radiant not found. Set RADIANT_PATH or add to PATH.".to_string(),
    })
}

fn detect_completed_phases(output: &str) -> Vec<String> {
    let mut phases = Vec::new();

    if output.contains("Synthesis completed") || output.contains("synth_done") {
        phases.push("synthesis".to_string());
    }
    if output.contains("Map completed") || output.contains("map_done") {
        phases.push("map".to_string());
    }
    if output.contains("PAR completed") || output.contains("par_done") {
        phases.push("place".to_string());
        phases.push("route".to_string());
    }
    if output.contains("Bitstream completed") || output.contains("bitstream_done") {
        phases.push("bitstream".to_string());
    }

    phases
}

fn detect_bitstream(build_dir: &std::path::Path) -> Vec<PathBuf> {
    let mut artifacts = Vec::new();
    let impl_dir = build_dir.join("impl");

    if impl_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&impl_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(ext) = path.extension() {
                    if ext == "bit" || ext == "nvcm" || ext == "hex" {
                        artifacts.push(path);
                    }
                }
            }
        }
    }

    artifacts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_completed_phases() {
        let output = "Running Synthesis...\nSynthesis completed successfully\nRunning Map...\nMap completed\nPAR completed successfully\n";
        let phases = detect_completed_phases(output);
        assert!(phases.contains(&"synthesis".to_string()));
        assert!(phases.contains(&"map".to_string()));
        assert!(phases.contains(&"place".to_string()));
    }
}
