use clap::Args;

use loom_core::error::LoomError;
use loom_core::resolve::{discover_members, find_workspace_root, MemberKind};

use crate::GlobalContext;

#[derive(Args)]
pub struct LintArgs {
    /// Project name (default: current directory)
    #[arg(short = 'p', long)]
    pub project: Option<String>,
}

pub fn run(_args: LintArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;
    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let members = discover_members(&workspace_root, &ws_manifest)?;

    let mut error_count = 0;

    // Lint all components
    for member in members.iter().filter(|m| m.kind == MemberKind::Component) {
        let manifest_path = member.path.join("component.toml");
        match loom_core::manifest::load_component_manifest(&manifest_path) {
            Err(e) => {
                eprintln!("  error: {}: {}", manifest_path.display(), e);
                error_count += 1;
            }
            Ok(manifest) => {
                for err in manifest.validate() {
                    eprintln!("  error: {}: {}", manifest_path.display(), err);
                    error_count += 1;
                }
            }
        }
    }

    // Lint all projects
    for member in members.iter().filter(|m| m.kind == MemberKind::Project) {
        let manifest_path = member.path.join("project.toml");
        match loom_core::manifest::load_project_manifest(&manifest_path) {
            Err(e) => {
                eprintln!("  error: {}: {}", manifest_path.display(), e);
                error_count += 1;
            }
            Ok(manifest) => {
                for err in manifest.validate() {
                    eprintln!("  error: {}: {}", manifest_path.display(), err);
                    error_count += 1;
                }
            }
        }
    }

    if !ctx.quiet {
        if error_count == 0 {
            println!("  All manifests are valid.");
        } else {
            println!("  {} error(s).", error_count);
        }
    }

    if error_count > 0 {
        Err(LoomError::ValidationFailed { error_count })
    } else {
        Ok(())
    }
}
