use clap::Args;

use loom_core::error::LoomError;
use loom_core::manifest::{load_component_manifest, load_project_manifest};
use loom_core::resolve::{discover_members, find_workspace_root, MemberKind};

use crate::ui;
use crate::GlobalContext;

#[derive(Args)]
pub struct ListArgs {
    /// Filter by kind: projects, components, platforms
    #[arg(long)]
    pub kind: Option<String>,
}

pub fn run(args: ListArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;

    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let members = discover_members(&workspace_root, &ws_manifest)?;

    let show_projects = args.kind.is_none() || args.kind.as_deref() == Some("projects");
    let show_components = args.kind.is_none() || args.kind.as_deref() == Some("components");
    let show_platforms = args.kind.is_none() || args.kind.as_deref() == Some("platforms");

    if ctx.json {
        return print_json(&members, show_projects, show_components, show_platforms);
    }

    // Header
    ui::header(&[("\u{00B7}", &ws_manifest.workspace.name)]);

    // Projects
    if show_projects {
        let project_members: Vec<_> = members
            .iter()
            .filter(|m| m.kind == MemberKind::Project)
            .collect();

        ui::section_header(&format!("Projects ({})", project_members.len()));
        if project_members.is_empty() {
            eprintln!("    (none)");
        } else {
            for (i, member) in project_members.iter().enumerate() {
                let is_last = i == project_members.len() - 1;
                let manifest_path = member.path.join("project.toml");
                match load_project_manifest(&manifest_path) {
                    Ok(manifest) => {
                        let target_info = manifest
                            .target
                            .as_ref()
                            .map(|t| format!("{}  {}", t.part, t.backend))
                            .unwrap_or_else(|| {
                                manifest
                                    .project
                                    .platform
                                    .as_ref()
                                    .map(|p| format!("platform: {}", p))
                                    .unwrap_or_else(|| "(no target)".to_string())
                            });
                        ui::tree_item(
                            &format!("{:<24} {}", manifest.project.name, target_info),
                            is_last,
                        );
                    }
                    Err(_) => {
                        ui::tree_item(
                            &format!("{} (error loading manifest)", member.path.display()),
                            is_last,
                        );
                    }
                }
            }
        }
        ui::blank();
    }

    // Components
    if show_components {
        let component_members: Vec<_> = members
            .iter()
            .filter(|m| m.kind == MemberKind::Component)
            .collect();

        ui::section_header(&format!("Components ({})", component_members.len()));
        if component_members.is_empty() {
            eprintln!("    (none)");
        } else {
            for (i, member) in component_members.iter().enumerate() {
                let is_last = i == component_members.len() - 1;
                let manifest_path = member.path.join("component.toml");
                match load_component_manifest(&manifest_path) {
                    Ok(manifest) => {
                        let file_count = manifest
                            .filesets
                            .get("synth")
                            .map(|s| s.files.len())
                            .unwrap_or(0);
                        let languages = detect_languages(&manifest);
                        ui::tree_item(
                            &format!(
                                "{:<28} v{:<8} {} file{}  ({})",
                                manifest.component.name,
                                manifest.component.version,
                                file_count,
                                if file_count == 1 { "" } else { "s" },
                                languages,
                            ),
                            is_last,
                        );
                    }
                    Err(_) => {
                        ui::tree_item(
                            &format!("{} (error loading manifest)", member.path.display()),
                            is_last,
                        );
                    }
                }
            }
        }
        ui::blank();
    }

    // Platforms
    if show_platforms {
        let platform_members: Vec<_> = members
            .iter()
            .filter(|m| m.kind == MemberKind::Platform)
            .collect();

        if !platform_members.is_empty() {
            ui::section_header(&format!("Platforms ({})", platform_members.len()));
            for (i, member) in platform_members.iter().enumerate() {
                let is_last = i == platform_members.len() - 1;
                let dir_name = member
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?");
                ui::tree_item(dir_name, is_last);
            }
            ui::blank();
        }
    }

    Ok(())
}

fn detect_languages(manifest: &loom_core::manifest::ComponentManifest) -> String {
    let mut langs = Vec::new();
    if let Some(synth) = manifest.filesets.get("synth") {
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

fn print_json(
    members: &[loom_core::resolve::MemberPath],
    show_projects: bool,
    show_components: bool,
    show_platforms: bool,
) -> Result<(), LoomError> {
    let mut result = serde_json::Map::new();

    if show_projects {
        let projects: Vec<serde_json::Value> = members
            .iter()
            .filter(|m| m.kind == MemberKind::Project)
            .filter_map(|m| {
                let manifest = load_project_manifest(&m.path.join("project.toml")).ok()?;
                Some(serde_json::json!({
                    "name": manifest.project.name,
                    "top_module": manifest.project.top_module,
                    "target": manifest.target.as_ref().map(|t| serde_json::json!({
                        "part": t.part,
                        "backend": t.backend,
                    })),
                    "path": m.path.display().to_string(),
                }))
            })
            .collect();
        result.insert("projects".to_string(), serde_json::Value::Array(projects));
    }

    if show_components {
        let components: Vec<serde_json::Value> = members
            .iter()
            .filter(|m| m.kind == MemberKind::Component)
            .filter_map(|m| {
                let manifest = load_component_manifest(&m.path.join("component.toml")).ok()?;
                let file_count = manifest
                    .filesets
                    .get("synth")
                    .map(|s| s.files.len())
                    .unwrap_or(0);
                Some(serde_json::json!({
                    "name": manifest.component.name,
                    "version": manifest.component.version,
                    "files": file_count,
                    "path": m.path.display().to_string(),
                }))
            })
            .collect();
        result.insert(
            "components".to_string(),
            serde_json::Value::Array(components),
        );
    }

    if show_platforms {
        let platforms: Vec<serde_json::Value> = members
            .iter()
            .filter(|m| m.kind == MemberKind::Platform)
            .map(|m| {
                let name = m.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                serde_json::json!({
                    "name": name,
                    "path": m.path.display().to_string(),
                })
            })
            .collect();
        result.insert("platforms".to_string(), serde_json::Value::Array(platforms));
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::Value::Object(result)).unwrap_or_default()
    );
    Ok(())
}
