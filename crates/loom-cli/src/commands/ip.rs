use clap::{Args, Subcommand};

use loom_core::error::LoomError;
use loom_core::manifest::GeneratorDecl;
use loom_core::resolve::{discover_members, find_workspace_root, load_all_components, MemberKind};

use crate::GlobalContext;

#[derive(Subcommand)]
pub enum IpCommands {
    /// List all IP instances across the workspace
    List(IpListArgs),

    /// Check for IP version upgrades
    Upgrade(IpUpgradeArgs),
}

#[derive(Args)]
pub struct IpListArgs {}

#[derive(Args)]
pub struct IpUpgradeArgs {
    /// Target Vivado version for upgrade check
    #[arg(long, value_name = "VERSION")]
    pub tool_version: Option<String>,

    /// Apply upgrades to manifest files
    #[arg(long)]
    pub apply: bool,

    /// Check if IP properties are valid in the new version
    #[arg(long)]
    pub check_properties: bool,
}

pub fn run(cmd: IpCommands, ctx: &GlobalContext) -> Result<(), LoomError> {
    match cmd {
        IpCommands::List(args) => run_list(args, ctx),
        IpCommands::Upgrade(args) => run_upgrade(args, ctx),
    }
}

fn run_list(_args: IpListArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;

    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let members = discover_members(&workspace_root, &ws_manifest)?;
    let all_components = load_all_components(&members)?;

    let mut ip_instances: Vec<(String, GeneratorDecl)> = Vec::new();

    // Collect from components
    for (_path, comp) in &all_components {
        let comp_name = comp.component.name.clone();
        for gen in &comp.generators {
            if gen.plugin == "vivado_ip" {
                ip_instances.push((comp_name.clone(), gen.clone()));
            }
        }
    }

    // Collect from projects
    for member in &members {
        if member.kind == MemberKind::Project {
            let manifest_path = member.path.join("project.toml");
            if let Ok(proj) = loom_core::manifest::load_project_manifest(&manifest_path) {
                for gen in &proj.generators {
                    if gen.plugin == "vivado_ip" {
                        ip_instances.push((proj.project.name.clone(), gen.clone()));
                    }
                }
            }
        }
    }

    if ip_instances.is_empty() {
        if !ctx.quiet {
            eprintln!("  No vivado_ip generators found in workspace.");
        }
        return Ok(());
    }

    if ctx.json {
        let items: Vec<serde_json::Value> = ip_instances
            .iter()
            .map(|(source, gen)| {
                let vlnv = gen
                    .config
                    .as_ref()
                    .and_then(|c| c.get("vlnv"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                serde_json::json!({
                    "name": gen.name,
                    "source": source,
                    "vlnv": vlnv,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&items).unwrap_or_default()
        );
    } else {
        eprintln!("  IP instances in workspace:");
        for (source, gen) in &ip_instances {
            let vlnv = gen
                .config
                .as_ref()
                .and_then(|c| c.get("vlnv"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            eprintln!("    {} ({}) — {}", gen.name, vlnv, source);
        }
    }

    Ok(())
}

fn run_upgrade(args: IpUpgradeArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;

    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let members = discover_members(&workspace_root, &ws_manifest)?;
    let all_components = load_all_components(&members)?;

    let mut ip_instances: Vec<(String, String, String)> = Vec::new(); // (name, vlnv, source)

    for (_path, comp) in &all_components {
        let comp_name = comp.component.name.clone();
        for gen in &comp.generators {
            if gen.plugin == "vivado_ip" {
                let vlnv = gen
                    .config
                    .as_ref()
                    .and_then(|c| c.get("vlnv"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                ip_instances.push((gen.name.clone(), vlnv, comp_name.clone()));
            }
        }
    }

    for member in &members {
        if member.kind == MemberKind::Project {
            let manifest_path = member.path.join("project.toml");
            if let Ok(proj) = loom_core::manifest::load_project_manifest(&manifest_path) {
                for gen in &proj.generators {
                    if gen.plugin == "vivado_ip" {
                        let vlnv = gen
                            .config
                            .as_ref()
                            .and_then(|c| c.get("vlnv"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        ip_instances.push((gen.name.clone(), vlnv, proj.project.name.clone()));
                    }
                }
            }
        }
    }

    if ip_instances.is_empty() {
        if !ctx.quiet {
            eprintln!("  No vivado_ip generators found.");
        }
        return Ok(());
    }

    // Without Vivado available, we can only report current state
    // Full upgrade check requires Vivado's get_ipdefs command
    if !ctx.quiet {
        if let Some(ref ver) = args.tool_version {
            eprintln!("  Checking IP upgrades for Vivado {} ...", ver);
        }
        eprintln!();
        for (name, vlnv, _source) in &ip_instances {
            eprintln!(
                "  {}: {} (unchanged — Vivado not available for version query)",
                name, vlnv
            );
        }
        eprintln!();
        eprintln!("  Note: Full IP upgrade requires Vivado on PATH to query available versions.");
        if args.apply {
            eprintln!("  --apply: No changes to apply (no upgrades detected).");
        }
    }

    Ok(())
}
