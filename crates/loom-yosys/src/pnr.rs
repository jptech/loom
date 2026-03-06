use std::path::Path;

use loom_core::build::context::BuildContext;
use loom_core::error::LoomError;
use loom_core::plugin::backend::BuildResult;
use loom_core::util::tool_command;

use crate::YosysArchitecture;

/// Run nextpnr for the given architecture.
pub fn run_nextpnr(
    arch: &YosysArchitecture,
    json_file: &Path,
    part: &str,
    context: &BuildContext,
) -> Result<BuildResult, LoomError> {
    let log_path = context.build_dir.join("nextpnr.log");
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

    let output = cmd.output().map_err(|e| LoomError::ToolNotFound {
        tool: arch.nextpnr_binary().to_string(),
        message: e.to_string(),
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let _ = std::fs::write(&log_path, stdout.as_ref());

    let success = output.status.success();

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
