use clap::Subcommand;

use loom_core::error::LoomError;

use crate::backend_registry::get_backend;
use crate::GlobalContext;

#[derive(Subcommand)]
pub enum EnvCommands {
    /// Check tool environment
    Check,
}

pub fn run(cmd: EnvCommands, ctx: &GlobalContext) -> Result<(), LoomError> {
    match cmd {
        EnvCommands::Check => run_check(ctx),
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
