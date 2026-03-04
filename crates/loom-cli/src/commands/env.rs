use clap::{Args, Subcommand};

use loom_core::error::LoomError;

use crate::backend_registry::get_backend;
use crate::GlobalContext;

#[derive(Subcommand)]
pub enum EnvCommands {
    /// Check tool environment
    Check,
    /// Open a subshell with the tool environment configured
    Shell(EnvShellArgs),
}

#[derive(Args)]
pub struct EnvShellArgs {
    /// Backend to configure environment for (default: auto-detect from project.toml)
    #[arg(short, long, default_value = "vivado")]
    pub backend: String,
}

pub fn run(cmd: EnvCommands, ctx: &GlobalContext) -> Result<(), LoomError> {
    match cmd {
        EnvCommands::Check => run_check(ctx),
        EnvCommands::Shell(args) => run_shell(args, ctx),
    }
}

fn run_check(ctx: &GlobalContext) -> Result<(), LoomError> {
    let backend = get_backend("vivado")?;
    let status = backend.check_environment(None)?;

    if ctx.json {
        let json = serde_json::json!({
            "backend": status.tool_name,
            "path": status.tool_path.display().to_string(),
            "version": status.version,
            "version_ok": status.version_matches,
            "license_ok": status.license_ok,
            "license_detail": status.license_detail,
            "warnings": status.warnings,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&json).unwrap_or_default()
        );
    } else {
        println!("Backend: {}", status.tool_name);
        println!("  Path:    {}", status.tool_path.display());
        println!("  Version: {}", status.version);
        if let Some(req) = &status.required_version {
            let ok_str = if status.version_matches {
                "OK"
            } else {
                "MISMATCH"
            };
            println!("  Required: {} [{}]", req, ok_str);
        }
        let license_str = if status.license_ok { "OK" } else { "FAILED" };
        println!("  License: {}", license_str);
        if let Some(detail) = &status.license_detail {
            println!("    ({})", detail);
        }
        for warning in &status.warnings {
            println!("  warning: {}", warning);
        }
    }

    if status.is_ok() {
        Ok(())
    } else {
        Err(LoomError::ToolVersionMismatch {
            required: status.required_version.unwrap_or_default(),
            found: status.version,
        })
    }
}

fn run_shell(args: EnvShellArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let backend = get_backend(&args.backend)?;
    let status = backend.check_environment(None)?;

    if !ctx.quiet {
        eprintln!(
            "Launching shell with {} {} environment...",
            status.tool_name, status.version
        );
        eprintln!("  Tool path: {}", status.tool_path.display());
        eprintln!("  Type 'exit' to return to normal shell.");
    }

    // Get the tool's bin directory to prepend to PATH
    let tool_bin_dir = status
        .tool_path
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = if tool_bin_dir.is_empty() {
        current_path
    } else {
        format!(
            "{}{}{}",
            tool_bin_dir,
            std::path::MAIN_SEPARATOR,
            current_path
        )
    };

    let prompt_arg = format!("prompt [loom:{}] $P$G", status.tool_name);
    let (shell, args_vec): (&str, Vec<&str>) = if cfg!(target_os = "windows") {
        ("cmd", vec!["/k", &prompt_arg])
    } else {
        ("bash", vec!["--norc", "--noprofile"])
    };

    let mut cmd = std::process::Command::new(shell);
    cmd.args(&args_vec).env("PATH", &new_path);

    // Set PS1 for bash
    if !cfg!(target_os = "windows") {
        cmd.env("PS1", format!("[loom:{}] \\w $ ", status.tool_name));
    }

    // Set tool-specific env vars
    if let Some(parent) = status.tool_path.parent() {
        if let Some(tool_root) = parent.parent() {
            match args.backend.as_str() {
                "vivado" => {
                    cmd.env("XILINX_VIVADO", tool_root.to_string_lossy().to_string());
                }
                "quartus" => {
                    cmd.env("QUARTUS_ROOTDIR", tool_root.to_string_lossy().to_string());
                }
                _ => {}
            }
        }
    }

    let child_status = cmd
        .status()
        .map_err(|e| LoomError::Internal(format!("Failed to launch shell: {}", e)))?;

    if !child_status.success() {
        // Non-zero exit from shell is expected (user just exits)
    }

    Ok(())
}
