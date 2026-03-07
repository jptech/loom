use std::io::BufRead;
use std::path::Path;
use std::process::Stdio;

use loom_core::build::context::BuildContext;
use loom_core::build::progress::BuildEvent;
use loom_core::error::LoomError;
use loom_core::plugin::backend::BuildResult;
use loom_core::util::tool_command;

use crate::output_parser;
use crate::YosysArchitecture;

/// Run nextpnr for the given architecture.
pub fn run_nextpnr(
    arch: &YosysArchitecture,
    json_file: &Path,
    part: &str,
    context: &BuildContext,
    progress: Option<&(dyn Fn(BuildEvent) + Send + Sync)>,
) -> Result<BuildResult, LoomError> {
    let log_path = context.build_dir.join("nextpnr.log");
    let start = std::time::Instant::now();

    if let Some(cb) = progress {
        cb(BuildEvent::PhaseStarted {
            phase: "place".to_string(),
        });
    }
    let output_file = match arch {
        YosysArchitecture::Ice40 => context.build_dir.join("design.asc"),
        YosysArchitecture::Ecp5 => context.build_dir.join("design.config"),
        YosysArchitecture::Gowin => context.build_dir.join("design.fs"),
    };

    let mut cmd = tool_command(arch.nextpnr_binary());
    cmd.current_dir(&context.build_dir);

    // Add architecture-specific flags
    match arch {
        YosysArchitecture::Ice40 => {
            let device = map_ice40_device(part);
            cmd.arg(format!("--{}", device));
            cmd.arg("--json").arg(json_file);
            cmd.arg("--asc").arg(&output_file);
        }
        YosysArchitecture::Ecp5 => {
            let device = map_ecp5_device(part);
            cmd.arg("--json").arg(json_file);
            cmd.arg("--textcfg").arg(&output_file);
            cmd.arg(format!("--{}", device));
        }
        YosysArchitecture::Gowin => {
            cmd.arg("--json").arg(json_file);
            cmd.arg("--write").arg(&output_file);
            cmd.arg("--device").arg(part);
        }
    }

    // Find and add constraint files
    let constraint_ext = arch.constraint_format();
    for entry in std::fs::read_dir(&context.build_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some(constraint_ext) {
            match arch {
                YosysArchitecture::Ice40 => {
                    cmd.arg("--pcf").arg(&path);
                }
                YosysArchitecture::Ecp5 => {
                    cmd.arg("--lpf").arg(&path);
                }
                YosysArchitecture::Gowin => {
                    cmd.arg("--cst").arg(&path);
                }
            }
        }
    }

    // Stream stderr to detect place→route transition live
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| LoomError::ToolNotFound {
        tool: arch.nextpnr_binary().to_string(),
        message: e.to_string(),
    })?;

    let stderr_pipe = child.stderr.take();
    let mut log_lines = Vec::new();
    let mut place_elapsed: Option<f64> = None;
    let mut route_started = false;

    if let Some(stderr_reader) = stderr_pipe {
        let reader = std::io::BufReader::new(stderr_reader);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };

            // Detect routing phase start: nextpnr prints "Info: Routing.." or "Routing design.."
            if !route_started && line.contains("Routing") && !line.contains("Routing globals") {
                route_started = true;
                place_elapsed = Some(start.elapsed().as_secs_f64());
                if let Some(cb) = progress {
                    cb(BuildEvent::PhaseCompleted {
                        phase: "place".to_string(),
                        elapsed_secs: place_elapsed.unwrap(),
                        memory_mb: None,
                    });
                    cb(BuildEvent::PhaseStarted {
                        phase: "route".to_string(),
                    });
                }
            }

            log_lines.push(line);
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| LoomError::ToolNotFound {
            tool: arch.nextpnr_binary().to_string(),
            message: e.to_string(),
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr_remaining = String::from_utf8_lossy(&output.stderr);
    // Combine: streamed stderr lines + any remaining stdout/stderr
    let mut log_content = log_lines.join("\n");
    if !stderr_remaining.is_empty() {
        if !log_content.is_empty() {
            log_content.push('\n');
        }
        log_content.push_str(&stderr_remaining);
    }
    if !stdout.is_empty() {
        if !log_content.is_empty() {
            log_content.push('\n');
        }
        log_content.push_str(&stdout);
    }
    let _ = std::fs::write(&log_path, &log_content);

    let total_elapsed = start.elapsed().as_secs_f64();
    let success = output.status.success();

    if let Some(cb) = progress {
        // Parse and emit utilization metrics
        if let Some(util) = output_parser::parse_nextpnr_utilization(&log_content) {
            let metrics = output_parser::to_utilization_metrics(&util);
            cb(BuildEvent::UtilizationAvailable(metrics));
        }

        // Parse and emit timing metrics
        let clocks = output_parser::parse_nextpnr_timing(&log_content);
        if !clocks.is_empty() {
            let timing = output_parser::to_timing_metrics(&clocks);
            cb(BuildEvent::TimingAvailable {
                stage: "post_route".to_string(),
                timing,
            });
        }

        // If we never detected the routing transition, emit both completions now
        if !route_started {
            cb(BuildEvent::PhaseCompleted {
                phase: "place".to_string(),
                elapsed_secs: total_elapsed / 2.0,
                memory_mb: None,
            });
            cb(BuildEvent::PhaseCompleted {
                phase: "route".to_string(),
                elapsed_secs: total_elapsed / 2.0,
                memory_mb: None,
            });
        } else {
            let route_elapsed = total_elapsed - place_elapsed.unwrap_or(0.0);
            cb(BuildEvent::PhaseCompleted {
                phase: "route".to_string(),
                elapsed_secs: route_elapsed,
                memory_mb: None,
            });
        }
    }

    Ok(BuildResult {
        success,
        exit_code: output.status.code().unwrap_or(-1),
        log_paths: vec![log_path],
        bitstream_path: None,
        phases_completed: if success {
            vec!["place".to_string(), "route".to_string()]
        } else {
            vec![]
        },
        failure_phase: if !success {
            Some("place".to_string())
        } else {
            None
        },
        failure_message: if !success {
            Some("nextpnr place and route failed".to_string())
        } else {
            None
        },
    })
}

fn map_ice40_device(part: &str) -> String {
    let lower = part.to_lowercase();
    if lower.contains("lp8k")
        || lower.contains("hx8k")
        || lower.contains("lp4k")
        || lower.contains("hx4k")
    {
        // 4k maps to 8k in nextpnr
        "8k".to_string()
    } else if lower.contains("up5k") {
        "up5k".to_string()
    } else if lower.contains("lp1k") || lower.contains("hx1k") {
        "1k".to_string()
    } else {
        "8k".to_string()
    }
}

fn map_ecp5_device(part: &str) -> String {
    let lower = part.to_lowercase();
    if lower.contains("85") {
        "85k".to_string()
    } else if lower.contains("45") {
        "45k".to_string()
    } else if lower.contains("25") {
        "25k".to_string()
    } else if lower.contains("12") {
        "12k".to_string()
    } else {
        "85k".to_string()
    }
}

/// Generate nextpnr command line for display/logging.
pub fn nextpnr_command_line(arch: &YosysArchitecture, json_file: &Path, part: &str) -> String {
    format!(
        "{} --json {} (part: {})",
        arch.nextpnr_binary(),
        json_file.display(),
        part
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_ice40_device() {
        assert_eq!(map_ice40_device("lp8k"), "8k");
        assert_eq!(map_ice40_device("up5k"), "up5k");
        assert_eq!(map_ice40_device("hx1k"), "1k");
    }

    #[test]
    fn test_map_ecp5_device() {
        assert_eq!(map_ecp5_device("LFE5U-85F"), "85k");
        assert_eq!(map_ecp5_device("LFE5U-25F"), "25k");
    }

    #[test]
    fn test_nextpnr_command_line() {
        let cmd = nextpnr_command_line(&YosysArchitecture::Ice40, Path::new("design.json"), "lp8k");
        assert!(cmd.contains("nextpnr-ice40"));
    }
}
