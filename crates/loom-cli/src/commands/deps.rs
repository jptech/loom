use clap::{Args, Subcommand};

use loom_core::error::LoomError;
use loom_core::resolve::lockfile::{generate_lockfile, write_lockfile};
use loom_core::resolve::{
    discover_members, find_project, find_workspace_root, load_all_components, resolve_project,
    MemberKind, WorkspaceDependencySource,
};

use crate::ui::{self, Icon};
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

    /// Show detailed per-component information
    #[arg(long = "detail")]
    pub detail: bool,
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
        ui::tree_root(&resolved.project.project.name);
        let verbose = args.detail || ctx.verbose > 0;
        for (i, comp) in resolved.resolved_components.iter().enumerate() {
            let is_last = i == resolved.resolved_components.len() - 1;
            ui::tree_item(
                &format!(
                    "{} v{}",
                    comp.manifest.component.name, comp.resolved_version
                ),
                is_last,
            );
            if verbose {
                let synth_fs = comp.manifest.filesets.get("synth");
                let file_count = synth_fs.map(|s| s.files.len()).unwrap_or(0);
                let constraint_count = synth_fs.map(|s| s.constraints.len()).unwrap_or(0);
                let languages = detect_languages(comp);
                let ooc = comp
                    .manifest
                    .synth
                    .as_ref()
                    .map(|s| if s.ooc { "yes" } else { "no" });
                let mut detail = format!(
                    "{} files ({})  \u{00B7}  {} constraint{}",
                    file_count,
                    languages,
                    constraint_count,
                    if constraint_count == 1 { "" } else { "s" }
                );
                if let Some(ooc_val) = ooc {
                    detail.push_str(&format!("  \u{00B7}  OOC: {}", ooc_val));
                }
                ui::tree_detail(&detail, is_last);
            }
        }
    }
    Ok(())
}

fn detect_languages(comp: &loom_core::resolve::resolver::ResolvedComponent) -> String {
    let mut langs = Vec::new();
    if let Some(synth) = comp.manifest.filesets.get("synth") {
        for f in &synth.files {
            let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
            match ext {
                "sv" | "svh" => {
                    if !langs.contains(&"SystemVerilog") {
                        langs.push("SystemVerilog");
                    }
                }
                "v" | "vh" => {
                    if !langs.contains(&"Verilog") {
                        langs.push("Verilog");
                    }
                }
                "vhd" | "vhdl" => {
                    if !langs.contains(&"VHDL") {
                        langs.push("VHDL");
                    }
                }
                _ => {}
            }
        }
    }
    if langs.is_empty() {
        "unknown".to_string()
    } else {
        langs.join(", ")
    }
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
        ui::status(Icon::Dot, "Deps", "re-resolving dependencies...");
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
            ui::status(Icon::Check, "Deps", "lockfile updated");
        }
    }

    Ok(())
}
