use clap::Args;

use loom_core::error::LoomError;
use loom_core::resolve::{discover_members, find_workspace_root, MemberKind, MemberPath};

use crate::GlobalContext;

#[derive(Args)]
pub struct CleanArgs {
    /// Remove all workspace build artifacts
    #[arg(long)]
    pub all: bool,

    /// Project name
    #[arg(short = 'p', long)]
    pub project: Option<String>,
}

pub fn run(args: CleanArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;
    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let build_dir = workspace_root.join(
        ws_manifest
            .settings
            .build_dir
            .as_deref()
            .unwrap_or(".build"),
    );

    if args.all {
        if build_dir.exists() {
            std::fs::remove_dir_all(&build_dir).map_err(|e| LoomError::Io {
                path: build_dir.clone(),
                source: e,
            })?;
            if !ctx.quiet {
                eprintln!("  Removed {}", build_dir.display());
            }
        } else if !ctx.quiet {
            eprintln!("  Nothing to clean.");
        }
    } else {
        let members = discover_members(&workspace_root, &ws_manifest)?;
        let project_name = args
            .project
            .clone()
            .or_else(|| detect_project_from_cwd(&cwd, &members))
            .ok_or_else(|| LoomError::ProjectNotFound {
                name: "<auto-detect>".to_string(),
            })?;

        let project_build_dir = build_dir.join(&project_name);
        if project_build_dir.exists() {
            std::fs::remove_dir_all(&project_build_dir).map_err(|e| LoomError::Io {
                path: project_build_dir.clone(),
                source: e,
            })?;
            if !ctx.quiet {
                eprintln!("  Removed {}", project_build_dir.display());
            }
        } else if !ctx.quiet {
            eprintln!("  Nothing to clean for project '{}'.", project_name);
        }
    }
    Ok(())
}

fn detect_project_from_cwd(cwd: &std::path::Path, members: &[MemberPath]) -> Option<String> {
    for member in members {
        if member.kind == MemberKind::Project
            && (cwd.starts_with(&member.path) || cwd == member.path)
        {
            let manifest_path = member.path.join("project.toml");
            if let Ok(m) = loom_core::manifest::load_project_manifest(&manifest_path) {
                return Some(m.project.name);
            }
        }
    }
    None
}
