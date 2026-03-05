use clap::Args;

use loom_core::error::LoomError;
use loom_core::resolve::{discover_members, find_workspace_root, resolve_project_selection};

use crate::ui::{self, Icon};
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
                ui::status(Icon::Check, "Removed", &build_dir.display().to_string());
            }
        } else if !ctx.quiet {
            ui::status(Icon::Dot, "Clean", "nothing to clean");
        }
    } else {
        let members = discover_members(&workspace_root, &ws_manifest)?;
        let (_, project_manifest) = resolve_project_selection(
            &members,
            args.project.as_deref(),
            Some(&cwd),
            ws_manifest.settings.default_project.as_deref(),
        )?;
        let project_name = project_manifest.project.name;

        let project_build_dir = build_dir.join(&project_name);
        if project_build_dir.exists() {
            std::fs::remove_dir_all(&project_build_dir).map_err(|e| LoomError::Io {
                path: project_build_dir.clone(),
                source: e,
            })?;
            if !ctx.quiet {
                ui::status(
                    Icon::Check,
                    "Removed",
                    &project_build_dir.display().to_string(),
                );
            }
        } else if !ctx.quiet {
            ui::status(
                Icon::Dot,
                "Clean",
                &format!("nothing to clean for '{}'", project_name),
            );
        }
    }
    Ok(())
}
