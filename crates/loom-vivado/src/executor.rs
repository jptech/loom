use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;

use loom_core::build::context::BuildContext;
use loom_core::build::progress::{BuildEvent, VivadoOutputParser};
use loom_core::error::LoomError;
use loom_core::plugin::backend::BuildResult;

use crate::env_check;

/// Run Vivado in batch mode, executing the given Tcl scripts.
pub fn run_vivado_batch(
    scripts: &[PathBuf],
    context: &BuildContext,
) -> Result<BuildResult, LoomError> {
    run_vivado_batch_with_progress(scripts, context, None)
}

/// Run Vivado in batch mode with an optional progress callback.
pub fn run_vivado_batch_with_progress(
    scripts: &[PathBuf],
    context: &BuildContext,
    progress: Option<&(dyn Fn(BuildEvent) + Send + Sync)>,
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
    let vivado_path = env_check::find_vivado_executable()?;

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

        let result = run_single_script(&vivado_path, script, &log_path, context, progress)?;

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
    vivado_path: &Path,
    script: &Path,
    log_path: &Path,
    context: &BuildContext,
    progress: Option<&(dyn Fn(BuildEvent) + Send + Sync)>,
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

    // Read stderr on a separate thread to avoid deadlock when pipe buffers fill.
    let stderr_handle = std::thread::spawn(move || -> std::io::Result<Vec<String>> {
        let mut lines = Vec::new();
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            lines.push(line?);
        }
        Ok(lines)
    });

    let mut parser = VivadoOutputParser::new();
    let (stdout_lines, was_cancelled) = collect_and_log_output_with_progress(
        stdout,
        &mut log_writer,
        "OUT",
        &mut parser,
        progress,
        &context.cancelled,
    )
    .map_err(|e| LoomError::Io {
        path: log_path.to_owned(),
        source: e,
    })?;

    if was_cancelled {
        // Kill the child process and collect what we have
        let _ = child.kill();
        let _ = child.wait();

        let phases_completed = if !parser.phases_completed().is_empty() {
            parser.phases_completed().to_vec()
        } else {
            detect_completed_phases(&stdout_lines)
        };

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

    let bitstream_path = detect_bitstream_path(&stdout_lines);
    // Use phases from parser if available, fall back to post-hoc detection
    let phases_completed = if !parser.phases_completed().is_empty() {
        parser.phases_completed().to_vec()
    } else {
        detect_completed_phases(&stdout_lines)
    };
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

fn collect_and_log_output_with_progress<R: std::io::Read>(
    reader: R,
    log: &mut impl Write,
    prefix: &str,
    parser: &mut VivadoOutputParser,
    progress: Option<&(dyn Fn(BuildEvent) + Send + Sync)>,
    cancelled: &std::sync::atomic::AtomicBool,
) -> std::io::Result<(Vec<String>, bool)> {
    let mut lines = Vec::new();
    let mut was_cancelled = false;
    let reader = BufReader::new(reader);
    for line in reader.lines() {
        let line = line?;
        writeln!(log, "[{}] {}", prefix, line)?;

        if let Some(cb) = progress {
            // Emit verbose line first
            cb(BuildEvent::VerboseLine(line.clone()));

            // Parse for structured events
            for event in parser.parse_line(&line) {
                cb(event);
            }
        } else {
            // Still parse to track phases for BuildResult
            parser.parse_line(&line);
        }

        lines.push(line);

        if cancelled.load(Ordering::Relaxed) {
            was_cancelled = true;
            break;
        }
    }

    // Flush any pending completion event from the parser
    if let Some(cb) = progress {
        for event in parser.flush() {
            cb(event);
        }
    } else {
        parser.flush();
    }

    Ok((lines, was_cancelled))
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
