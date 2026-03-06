use loom_core::build::context::BuildContext;
use loom_core::error::LoomError;
use loom_core::plugin::backend::BuildResult;
use loom_core::util::tool_command;

use crate::YosysArchitecture;

/// Run bitstream packing (icepack, ecppack, gowin_pack).
pub fn run_pack(
    arch: &YosysArchitecture,
    context: &BuildContext,
) -> Result<BuildResult, LoomError> {
    let log_path = context.build_dir.join("pack.log");

    let (input_file, output_file) = match arch {
        YosysArchitecture::Ice40 => (
            context.build_dir.join("design.asc"),
            context.build_dir.join("design.bit"),
        ),
        YosysArchitecture::Ecp5 => (
            context.build_dir.join("design.config"),
            context.build_dir.join("design.bit"),
        ),
        YosysArchitecture::Gowin => (
            context.build_dir.join("design.fs"),
            context.build_dir.join("design.bit"),
        ),
    };

    let mut cmd = tool_command(arch.pack_binary());
    cmd.arg(input_file.display().to_string())
        .arg(output_file.display().to_string())
        .current_dir(&context.build_dir);

    let output = cmd.output().map_err(|e| LoomError::ToolNotFound {
        tool: arch.pack_binary().to_string(),
        message: e.to_string(),
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let _ = std::fs::write(&log_path, stdout.as_ref());

    let success = output.status.success();

    Ok(BuildResult {
        success,
        exit_code: output.status.code().unwrap_or(-1),
        log_paths: vec![log_path],
        bitstream_path: if success { Some(output_file) } else { None },
        phases_completed: if success {
            vec!["bitstream".to_string()]
        } else {
            vec![]
        },
        failure_phase: if !success {
            Some("bitstream".to_string())
        } else {
            None
        },
        failure_message: if !success {
            Some("Bitstream packing failed".to_string())
        } else {
            None
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack_binary_names() {
        assert_eq!(YosysArchitecture::Ice40.pack_binary(), "icepack");
        assert_eq!(YosysArchitecture::Ecp5.pack_binary(), "ecppack");
        assert_eq!(YosysArchitecture::Gowin.pack_binary(), "gowin_pack");
    }
}
