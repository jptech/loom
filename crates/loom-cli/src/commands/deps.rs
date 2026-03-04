use clap::{Args, Subcommand};

use loom_core::error::LoomError;
use loom_core::resolve::lockfile::{generate_lockfile, write_lockfile};
use loom_core::resolve::{
    discover_members, find_project, find_workspace_root, load_all_components, resolve_project,
    MemberKind, WorkspaceDependencySource,
};

use crate::GlobalContext;

#[derive(Subcommand)]
pub enum DepsCommands {
    /// Show dependency tree
    Tree(DepsTreeArgs),
    /// Re-resolve all dependencies and regenerate lockfile
    Update(DepsUpdateArgs),
}

#[derive(Args)]
pub struct DepsTreeArgs {
    #[arg(short = 'p', long)]
    pub project: Option<String>,
}

#[derive(Args)]
pub struct DepsUpdateArgs {
    /// Specific dependency to update (default: all)
    pub dependency: Option<String>,
}

pub fn run(cmd: DepsCommands, ctx: &GlobalContext) -> Result<(), LoomError> {
    match cmd {
        DepsCommands::Tree(args) => run_tree(args, ctx),
        DepsCommands::Update(args) => run_update(args, ctx),
    }
}

fn run_tree(args: DepsTreeArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
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
    let resolved = resolve_project(project_manifest, project_root, workspace_root, &source)?;

    if ctx.json {
        let tree: Vec<_> = resolved
            .resolved_components
            .iter()
            .map(|c| {
                serde_json::json!({
                    "name": c.manifest.component.name,
                    "version": c.resolved_version.to_string(),
                    "path": c.source_path.display().to_string(),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&tree).unwrap_or_default()
        );
    } else {
        println!("{}", resolved.project.project.name);
        for (i, comp) in resolved.resolved_components.iter().enumerate() {
            let is_last = i == resolved.resolved_components.len() - 1;
            let prefix = if is_last { "└──" } else { "├──" };
            println!(
                "  {} {} v{}",
                prefix, comp.manifest.component.name, comp.resolved_version
            );
        }
    }
    Ok(())
}

fn run_update(_args: DepsUpdateArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;
    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let members = discover_members(&workspace_root, &ws_manifest)?;
    let all_components = load_all_components(&members)?;

    if !ctx.quiet {
        eprintln!("  Re-resolving dependencies...");
    }

    if let Some(member) = members.iter().find(|m| m.kind == MemberKind::Project) {
        let manifest_path = member.path.join("project.toml");
        let project_manifest = loom_core::manifest::load_project_manifest(&manifest_path)?;
        let source = WorkspaceDependencySource::new(all_components.clone());
        let resolved = resolve_project(
            project_manifest,
            member.path.clone(),
            workspace_root.clone(),
            &source,
        )?;
        let lockfile = generate_lockfile(&resolved, &members)?;
        write_lockfile(&lockfile, &workspace_root)?;
        if !ctx.quiet {
            eprintln!("  Lockfile updated.");
        }
    }

    Ok(())
}
