use clap::Args;

use loom_core::build::context::BuildContext;
use loom_core::build::report::{report_path, BuildReport};
use loom_core::error::LoomError;
use loom_core::resolve::{
    discover_members, find_project, find_workspace_root, load_all_components, resolve_project,
    WorkspaceDependencySource,
};

use crate::GlobalContext;

#[derive(Args)]
pub struct ReportArgs {
    /// Project name (default: auto-detect)
    #[arg(short = 'p', long)]
    pub project: Option<String>,
}

pub fn run(args: ReportArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;

    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let members = discover_members(&workspace_root, &ws_manifest)?;
    let all_components = load_all_components(&members)?;

    let (project_root, project_manifest) = match &args.project {
        Some(name) => find_project(&members, Some(name))?,
        None => find_project(&members, None)?,
    };

    let source = WorkspaceDependencySource::new(all_components);
    let resolved = resolve_project(
        project_manifest,
        project_root,
        workspace_root.clone(),
        &source,
    )?;

    let build_context = BuildContext::new(resolved, workspace_root);
    let path = report_path(&build_context.build_dir);

    if !path.exists() {
        return Err(LoomError::Internal(format!(
            "No build report found at {}. Run 'loom build' first.",
            path.display()
        )));
    }

    let report = BuildReport::load_from_file(&path)?;

    if ctx.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    } else {
        eprintln!("  Project:  {}", report.project);
        eprintln!("  Backend:  {} {}", report.tool.name, report.tool.version);
        eprintln!("  Target:   {}", report.target.part);
        eprintln!("  Strategy: {}", report.strategy);
        eprintln!(
            "  Status:   {}",
            if report.status.success {
                "PASSED"
            } else {
                "FAILED"
            }
        );
        if !report.status.phases_completed.is_empty() {
            eprintln!(
                "  Phases:   {}",
                report.status.phases_completed.join(" -> ")
            );
        }
        if let Some(ref phase) = report.status.failure_phase {
            eprintln!("  Failed:   {}", phase);
        }
        if let Some(ref git) = report.git {
            eprintln!(
                "  Git:      {} {}",
                &git.commit[..8.min(git.commit.len())],
                if git.dirty { "(dirty)" } else { "" }
            );
        }
        eprintln!("  Time:     {}", report.timestamp);
    }

    Ok(())
}
