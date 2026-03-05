use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;

use loom_core::build::context::BuildContext;
use loom_core::error::LoomError;
use loom_core::plugin::backend::BuildResult;

/// Run Quartus in batch mode via quartus_sh.
pub fn run_quartus_batch(
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
    let quartus_path = find_quartus_executable()?;

    let mut all_phases_completed = Vec::new();
    let mut last_result: Option<BuildResult> = None;

    for script in scripts {
        // Check cancellation before starting next script
        if context.cancelled.load(Ordering::Relaxed) {
            return Ok(BuildResult {
                success: false,
                exit_code: 130,
                log_paths: vec![log_path],
                bitstream_path: None,
                phases_completed: all_phases_completed,
                failure_phase: Some("interrupted".to_string()),
                failure_message: Some("Build interrupted by user".to_string()),
            });
        }

        let result = run_single_script(&quartus_path, script, &log_path, context)?;

        all_phases_completed.extend(result.phases_completed.clone());

        if result.failure_phase.as_deref() == Some("interrupted") {
            return Ok(BuildResult {
                success: false,
                exit_code: 130,
                log_paths: result.log_paths,
                bitstream_path: None,
                phases_completed: all_phases_completed,
                failure_phase: Some("interrupted".to_string()),
                failure_message: Some("Build interrupted by user".to_string()),
            });
        }

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
    quartus_path: &Path,
    script: &Path,
    log_path: &Path,
    context: &BuildContext,
) -> Result<BuildResult, LoomError> {
    let log_file = std::fs::File::create(log_path).map_err(|e| LoomError::Io {
        path: log_path.to_owned(),
        source: e,
    })?;

    let mut log_writer = std::io::BufWriter::new(log_file);

    writeln!(log_writer, "# Loom Quartus build log").map_err(|e| LoomError::Io {
        path: log_path.to_owned(),
        source: e,
    })?;
    writeln!(log_writer, "# Command: quartus_sh -t {}", script.display()).map_err(|e| {
        LoomError::Io {
            path: log_path.to_owned(),
            source: e,
        }
    })?;
    writeln!(log_writer, "# ---").map_err(|e| LoomError::Io {
        path: log_path.to_owned(),
        source: e,
    })?;

    let mut cmd = Command::new(quartus_path);
    cmd.arg("-t")
        .arg(script)
        .current_dir(&context.build_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| LoomError::ToolNotFound {
        tool: "quartus_sh".to_string(),
        message: e.to_string(),
    })?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    // Read stderr on a separate thread to avoid deadlock when pipe buffers fill.
    let stderr_handle = std::thread::spawn(move || -> std::io::Result<Vec<String>> {
        let mut lines = Vec::new();
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            lines.push(line?);
        }
        Ok(lines)
    });

    let (stdout_lines, was_cancelled) =
        collect_and_log_output(stdout, &mut log_writer, "OUT", &context.cancelled).map_err(
            |e| LoomError::Io {
                path: log_path.to_owned(),
                source: e,
            },
        )?;

    if was_cancelled {
        let _ = child.kill();
        let _ = child.wait();

        let phases_completed = detect_completed_phases(&stdout_lines);

        return Ok(BuildResult {
            success: false,
            exit_code: 130,
            log_paths: vec![log_path.to_owned()],
            bitstream_path: None,
            phases_completed,
            failure_phase: Some("interrupted".to_string()),
            failure_message: Some("Build interrupted by user".to_string()),
        });
    }

    let stderr_lines = stderr_handle
        .join()
        .map_err(|_| LoomError::Internal("stderr reader thread panicked".to_string()))?
        .map_err(|e| LoomError::Io {
            path: log_path.to_owned(),
            source: e,
        })?;

    // Log collected stderr lines
    for line in &stderr_lines {
        writeln!(log_writer, "[ERR] {}", line).map_err(|e| LoomError::Io {
            path: log_path.to_owned(),
            source: e,
        })?;
    }

    let status = child.wait().map_err(|e| LoomError::Io {
        path: log_path.to_owned(),
        source: e,
    })?;

    let exit_code = status.code().unwrap_or(-1);
    let success = exit_code == 0;

    let bitstream_path = detect_sof_path(&stdout_lines);
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
    cancelled: &std::sync::atomic::AtomicBool,
) -> std::io::Result<(Vec<String>, bool)> {
    let mut lines = Vec::new();
    let mut was_cancelled = false;
    let reader = BufReader::new(reader);
    for line in reader.lines() {
        let line = line?;
        writeln!(log, "[{}] {}", prefix, line)?;
        lines.push(line);

        if cancelled.load(Ordering::Relaxed) {
            was_cancelled = true;
            break;
        }
    }
    Ok((lines, was_cancelled))
}

/// Find the quartus_sh executable.
pub fn find_quartus_executable() -> Result<PathBuf, LoomError> {
    if let Ok(path) = std::env::var("QUARTUS_SH_PATH") {
        let p = PathBuf::from(&path);
        if p.exists() {
            return Ok(p);
        }
    }

    if let Ok(val) = std::env::var("QUARTUS_ROOTDIR") {
        let bin = PathBuf::from(val).join("bin").join(quartus_exe_name());
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
        tool: "quartus_sh".to_string(),
        message: "Not found in QUARTUS_SH_PATH, QUARTUS_ROOTDIR, standard paths, or system PATH. \
                  Run `loom env check` for details."
            .to_string(),
    })
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

/// Detect .sof output path from Quartus log.
fn detect_sof_path(lines: &[String]) -> Option<PathBuf> {
    for line in lines {
        // Quartus reports assembler output
        if line.contains(".sof") && (line.contains("Generated") || line.contains("output_files")) {
            // Try to extract the path
            for word in line.split_whitespace() {
                if word.ends_with(".sof") || word.ends_with(".sof\"") {
                    let cleaned = word.trim_matches('"');
                    return Some(PathBuf::from(cleaned));
                }
            }
        }
    }
    None
}

/// Map Quartus log output to generic phase names.
pub fn detect_completed_phases(lines: &[String]) -> Vec<String> {
    let phase_patterns: &[(&[&str], &str)] = &[
        (
            &[
                "Analysis & Synthesis was successful",
                "Quartus Prime Analysis & Synthesis was successful",
            ],
            "synthesis",
        ),
        (
            &[
                "Fitter was successful",
                "Quartus Prime Fitter was successful",
            ],
            "place",
        ),
        (
            &[
                "Timing Analyzer was successful",
                "Quartus Prime Timing Analyzer was successful",
            ],
            "route",
        ),
        (
            &[
                "Assembler was successful",
                "Quartus Prime Assembler was successful",
            ],
            "bitstream",
        ),
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
        let lower = line.to_lowercase();
        if lower.contains("error") {
            if lower.contains("analysis") || lower.contains("synthesis") {
                return Some("synthesis".to_string());
            }
            if lower.contains("fitter") {
                return Some("place".to_string());
            }
            if lower.contains("timing") {
                return Some("route".to_string());
            }
            if lower.contains("assembler") {
                return Some("bitstream".to_string());
            }
        }
    }
    Some("unknown".to_string())
}

fn extract_failure_message(stderr: &[String]) -> String {
    let errors: Vec<&str> = stderr
        .iter()
        .filter(|l| l.contains("Error") || l.contains("ERROR"))
        .map(|l| l.as_str())
        .take(5)
        .collect();
    if errors.is_empty() {
        "Quartus build failed. Check build log for details.".to_string()
    } else {
        errors.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_completed_phases() {
        let lines = vec![
            "Info: Quartus Prime Analysis & Synthesis was successful".to_string(),
            "Info: Quartus Prime Fitter was successful".to_string(),
        ];
        let phases = detect_completed_phases(&lines);
        assert!(phases.contains(&"synthesis".to_string()));
        assert!(phases.contains(&"place".to_string()));
        assert!(!phases.contains(&"route".to_string()));
    }

    #[test]
    fn test_detect_all_phases() {
        let lines = vec![
            "Analysis & Synthesis was successful".to_string(),
            "Fitter was successful".to_string(),
            "Timing Analyzer was successful".to_string(),
            "Assembler was successful".to_string(),
        ];
        let phases = detect_completed_phases(&lines);
        assert_eq!(phases.len(), 4);
    }

    #[test]
    fn test_detect_sof_path() {
        let lines = vec!["Info: Generated output_files/my_design.sof".to_string()];
        let path = detect_sof_path(&lines);
        assert!(path.is_some());
    }
}
