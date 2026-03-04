use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use loom_core::build::context::BuildContext;
use loom_core::error::LoomError;
use loom_core::plugin::backend::BuildResult;

/// Run Vivado in batch mode, executing the given Tcl scripts.
pub fn run_vivado_batch(
    scripts: &[PathBuf],
    context: &BuildContext,
) -> Result<BuildResult, LoomError> {
    if scripts.is_empty() {
        return Err(LoomError::Internal(
            "No build scripts to execute".to_string(),
        ));
    }

    std::fs::create_dir_all(&context.build_dir).map_err(|e| LoomError::Io {
        path: context.build_dir.clone(),
        source: e,
    })?;

    let log_path = context.build_dir.join("build.log");
    let vivado_path = find_vivado_executable()?;

    let mut all_phases_completed = Vec::new();
    let mut last_result: Option<BuildResult> = None;

    for script in scripts {
        let result = run_single_script(&vivado_path, script, &log_path, context)?;

        all_phases_completed.extend(result.phases_completed.clone());
        let success = result.success;
        last_result = Some(result);

        if !success {
            break;
        }
    }

    Ok(last_result.unwrap_or_else(|| BuildResult {
        success: false,
        exit_code: -1,
        log_paths: vec![log_path],
        bitstream_path: None,
        phases_completed: vec![],
        failure_phase: Some("unknown".to_string()),
        failure_message: Some("No scripts were executed".to_string()),
    }))
}

fn run_single_script(
    vivado_path: &Path,
    script: &Path,
    log_path: &Path,
    context: &BuildContext,
) -> Result<BuildResult, LoomError> {
    let log_file = std::fs::File::create(log_path).map_err(|e| LoomError::Io {
        path: log_path.to_owned(),
        source: e,
    })?;

    let mut log_writer = std::io::BufWriter::new(log_file);

    writeln!(log_writer, "# Loom build log").map_err(|e| LoomError::Io {
        path: log_path.to_owned(),
        source: e,
    })?;
    writeln!(
        log_writer,
        "# Command: vivado -mode batch -source {}",
        script.display()
    )
    .map_err(|e| LoomError::Io {
        path: log_path.to_owned(),
        source: e,
    })?;
    writeln!(log_writer, "# ---").map_err(|e| LoomError::Io {
        path: log_path.to_owned(),
        source: e,
    })?;

    let mut cmd = Command::new(vivado_path);
    cmd.arg("-mode")
        .arg("batch")
        .arg("-source")
        .arg(script)
        .arg("-nojournal")
        .arg("-nolog")
        .current_dir(&context.build_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| LoomError::ToolNotFound {
        tool: "vivado".to_string(),
        message: e.to_string(),
    })?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    let stdout_lines =
        collect_and_log_output(stdout, &mut log_writer, "OUT").map_err(|e| LoomError::Io {
            path: log_path.to_owned(),
            source: e,
        })?;

    let stderr_lines =
        collect_and_log_output(stderr, &mut log_writer, "ERR").map_err(|e| LoomError::Io {
            path: log_path.to_owned(),
            source: e,
        })?;

    let status = child.wait().map_err(|e| LoomError::Io {
        path: log_path.to_owned(),
        source: e,
    })?;

    let exit_code = status.code().unwrap_or(-1);
    let success = exit_code == 0;

    let bitstream_path = detect_bitstream_path(&stdout_lines);
    let phases_completed = detect_completed_phases(&stdout_lines);
    let failure_phase = if !success {
        detect_failure_phase(&stdout_lines, &stderr_lines)
    } else {
        None
    };

    Ok(BuildResult {
        success,
        exit_code,
        log_paths: vec![log_path.to_owned()],
        bitstream_path,
        phases_completed,
        failure_phase,
        failure_message: if !success {
            Some(extract_failure_message(&stderr_lines))
        } else {
            None
        },
    })
}

fn collect_and_log_output<R: std::io::Read>(
    reader: R,
    log: &mut impl Write,
    prefix: &str,
) -> std::io::Result<Vec<String>> {
    let mut lines = Vec::new();
    let reader = BufReader::new(reader);
    for line in reader.lines() {
        let line = line?;
        writeln!(log, "[{}] {}", prefix, line)?;
        lines.push(line);
    }
    Ok(lines)
}

/// Find Vivado executable.
pub fn find_vivado_executable() -> Result<PathBuf, LoomError> {
    if let Ok(path) = std::env::var("VIVADO_PATH") {
        let p = PathBuf::from(&path);
        if p.exists() {
            return Ok(p);
        }
    }

    if let Ok(val) = std::env::var("XILINX_VIVADO") {
        let bin = PathBuf::from(val).join("bin").join(vivado_exe_name());
        if bin.exists() {
            return Ok(bin);
        }
    }

    let standard_paths = find_standard_installations();
    for path in &standard_paths {
        if path.exists() {
            return Ok(path.clone());
        }
    }

    if let Some(path) = find_on_path() {
        return Ok(path);
    }

    Err(LoomError::ToolNotFound {
        tool: "vivado".to_string(),
        message: "Not found in VIVADO_PATH, XILINX_VIVADO, standard paths, or system PATH. \
                  Run `loom env check` for details."
            .to_string(),
    })
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

fn detect_bitstream_path(lines: &[String]) -> Option<PathBuf> {
    for line in lines {
        if line.contains("Bitfile:") {
            if let Some(path_str) = line.split("Bitfile:").nth(1) {
                let path = PathBuf::from(path_str.trim());
                if path.exists() {
                    return Some(path);
                }
            }
        }
    }
    None
}

pub fn detect_completed_phases(lines: &[String]) -> Vec<String> {
    let phase_patterns: &[(&[&str], &str)] = &[
        (&["synth_design completed", "Synthesis Report"], "synthesis"),
        (&["opt_design completed", "Opt Design Report"], "optimize"),
        (&["place_design completed", "Placement Report"], "place"),
        (&["route_design completed", "Routing Report"], "route"),
        (&["write_bitstream completed"], "bitstream"),
    ];

    let mut phases = Vec::new();
    for line in lines {
        for (patterns, phase_name) in phase_patterns {
            if patterns.iter().any(|p| line.contains(p))
                && !phases.contains(&phase_name.to_string())
            {
                phases.push(phase_name.to_string());
            }
        }
    }
    phases
}

fn detect_failure_phase(stdout: &[String], stderr: &[String]) -> Option<String> {
    for line in stdout.iter().chain(stderr.iter()).rev() {
        if line.contains("ERROR") {
            if line.contains("synth") {
                return Some("synthesis".to_string());
            }
            if line.contains("opt") {
                return Some("optimize".to_string());
            }
            if line.contains("place") {
                return Some("place".to_string());
            }
            if line.contains("route") {
                return Some("route".to_string());
            }
            if line.contains("bitstream") {
                return Some("bitstream".to_string());
            }
        }
    }
    Some("unknown".to_string())
}

fn extract_failure_message(stderr: &[String]) -> String {
    let errors: Vec<&str> = stderr
        .iter()
        .filter(|l| l.contains("ERROR"))
        .map(|l| l.as_str())
        .take(5)
        .collect();
    if errors.is_empty() {
        "Build failed. Check build log for details.".to_string()
    } else {
        errors.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_completed_phases_from_log() {
        let lines = vec![
            "INFO: [Vivado 12-111] synth_design completed".to_string(),
            "INFO: [Vivado 12-112] opt_design completed".to_string(),
        ];
        let phases = detect_completed_phases(&lines);
        assert!(phases.contains(&"synthesis".to_string()));
        assert!(phases.contains(&"optimize".to_string()));
        assert!(!phases.contains(&"place".to_string()));
    }

    #[test]
    fn test_detect_all_phases() {
        let lines = vec![
            "synth_design completed".to_string(),
            "opt_design completed".to_string(),
            "place_design completed".to_string(),
            "route_design completed".to_string(),
            "write_bitstream completed".to_string(),
        ];
        let phases = detect_completed_phases(&lines);
        assert_eq!(phases.len(), 5);
    }
}
