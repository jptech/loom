use clap::Args;

use loom_core::assemble::assemble_filesets;
use loom_core::assemble::fileset::{ConstraintScope, FileLanguage};
use loom_core::build::context::BuildContext;
use loom_core::build::report::{report_path, BuildReport};
use loom_core::error::LoomError;
use loom_core::manifest::load_project_manifest;
use loom_core::resolve::{
    discover_members, find_workspace_root, load_all_components, resolve_project,
    resolve_project_selection, MemberKind, WorkspaceDependencySource,
};

use crate::ui::{self, Icon};
use crate::GlobalContext;

#[derive(Args)]
pub struct StatusArgs {
    /// Project name (default: auto-detect)
    #[arg(short = 'p', long)]
    pub project: Option<String>,
}

pub fn run(args: StatusArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;

    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let members = discover_members(&workspace_root, &ws_manifest)?;
    let all_components = load_all_components(&members)?;

    let project_result = resolve_project_selection(
        &members,
        args.project.as_deref(),
        Some(&cwd),
        ws_manifest.settings.default_project.as_deref(),
    );

    // If multiple projects and no explicit selection, show workspace overview
    let (project_root, project_manifest) = match project_result {
        Ok(result) => result,
        Err(LoomError::AmbiguousProject { .. }) => {
            return run_workspace_overview(&ws_manifest, &members, &workspace_root, ctx);
        }
        Err(e) => return Err(e),
    };

    let source = WorkspaceDependencySource::new(all_components);
    let resolved = resolve_project(
        project_manifest.clone(),
        project_root,
        workspace_root.clone(),
        &source,
    )?;

    let filesets = assemble_filesets(&resolved)?;
    let build_context = BuildContext::new(resolved.clone(), workspace_root);

    if ctx.json {
        return print_json(&resolved, &filesets, &build_context);
    }

    // Header
    ui::header(&[("\u{00B7}", &resolved.project.project.name)]);

    // ── Project section ──
    ui::section_header("Project");
    ui::detail_line("Name", &resolved.project.project.name);
    ui::detail_line("Top module", &resolved.project.project.top_module);

    if let Some(target) = &resolved.project.target {
        ui::detail_line("Target", &target.part);
        let backend_version = target
            .version
            .as_ref()
            .map(|v| format!("{} {}", target.backend, v))
            .unwrap_or_else(|| target.backend.clone());
        ui::detail_line("Backend", &backend_version);
    }

    if let Some(platform) = &resolved.project.project.platform {
        ui::detail_line("Platform", platform);
    }

    ui::blank();

    // ── Dependencies section ──
    let dep_count = resolved.resolved_components.len();
    ui::section_header(&format!("Dependencies ({})", dep_count));
    if dep_count == 0 {
        eprintln!("    (none)");
    } else {
        for (i, comp) in resolved.resolved_components.iter().enumerate() {
            let is_last = i == dep_count - 1;
            let file_count = comp
                .manifest
                .filesets
                .get("synth")
                .map(|s| s.files.len())
                .unwrap_or(0);
            ui::tree_item(
                &format!(
                    "{} v{}    {} files",
                    comp.manifest.component.name, comp.resolved_version, file_count
                ),
                is_last,
            );
        }
    }
    ui::blank();

    // ── Source Files section ──
    let total_files = filesets.synth_files.len();
    ui::section_header(&format!("Source Files ({})", total_files));
    let mut sv_count = 0;
    let mut v_count = 0;
    let mut vhdl_count = 0;
    for f in &filesets.synth_files {
        match f.language {
            FileLanguage::SystemVerilog => sv_count += 1,
            FileLanguage::Verilog => v_count += 1,
            FileLanguage::Vhdl => vhdl_count += 1,
            FileLanguage::Unknown => {}
        }
    }
    let mut parts = Vec::new();
    if sv_count > 0 {
        parts.push(format!("SystemVerilog: {}", sv_count));
    }
    if v_count > 0 {
        parts.push(format!("Verilog: {}", v_count));
    }
    if vhdl_count > 0 {
        parts.push(format!("VHDL: {}", vhdl_count));
    }
    if !parts.is_empty() {
        eprintln!("    {}", parts.join("   "));
    }
    ui::blank();

    // ── Constraints section ──
    let total_constraints = filesets.constraint_files.len();
    ui::section_header(&format!("Constraints ({})", total_constraints));
    let component_scoped = filesets
        .constraint_files
        .iter()
        .filter(|c| matches!(c.scope, ConstraintScope::Component { .. }))
        .count();
    let global_scoped = filesets
        .constraint_files
        .iter()
        .filter(|c| matches!(c.scope, ConstraintScope::Global))
        .count();
    if total_constraints > 0 {
        eprintln!(
            "    Component-scoped: {}   Global: {}",
            component_scoped, global_scoped
        );
    }
    ui::blank();

    // ── Last Build section ──
    ui::section_header("Last Build");
    let report_file = report_path(&build_context.build_dir);
    if report_file.exists() {
        match BuildReport::load_from_file(&report_file) {
            Ok(report) => print_build_summary(&report),
            Err(_) => {
                eprintln!("    Could not load build report.");
            }
        }
    } else {
        eprintln!("    No build found. Run 'loom build' to start.");
    }
    ui::blank();

    Ok(())
}

fn print_build_summary(report: &BuildReport) {
    let status_str = if report.status.success {
        "passed"
    } else {
        "failed"
    };
    let icon = if report.status.success {
        Icon::Check
    } else {
        Icon::Cross
    };
    let dur = report
        .metrics
        .duration_secs
        .map(|s| format!(" \u{00B7} {}", ui::format_duration(s)))
        .unwrap_or_default();
    let ts = format_timestamp(&report.timestamp);

    eprintln!(
        "    {} {}{}{}",
        icon.render(),
        status_str,
        dur,
        if ts.is_empty() {
            String::new()
        } else {
            format!(" \u{00B7} {}", ts)
        }
    );

    if let Some(util) = &report.metrics.utilization {
        eprintln!(
            "    {:<4} {:>5.1}%  {}   {:<4} {:>5.1}%  {}",
            "LUT",
            util.lut_percent,
            ui::util_bar(util.lut_percent),
            "FF",
            util.ff_percent,
            ui::util_bar(util.ff_percent),
        );
        eprintln!(
            "    {:<4} {:>5.1}%  {}   {:<4} {:>5.1}%  {}",
            "BRAM",
            util.bram_percent,
            ui::util_bar(util.bram_percent),
            "DSP",
            0.0,
            ui::util_bar(0.0),
        );
    }

    if let Some(timing) = &report.metrics.timing {
        let wns_icon = if timing.wns >= 0.0 {
            ui::CHECK.to_string()
        } else {
            ui::CROSS.to_string()
        };
        let whs_icon = if timing.whs >= 0.0 {
            ui::CHECK.to_string()
        } else {
            ui::CROSS.to_string()
        };
        eprintln!(
            "    Timing  WNS {:+.3}ns {}  WHS {:+.3}ns {}",
            timing.wns, wns_icon, timing.whs, whs_icon
        );
    }
}

fn format_timestamp(ts: &str) -> String {
    // Try to parse RFC3339 and reformat as "Mar 4, 2026 14:32"
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        dt.format("%b %-d, %Y %H:%M").to_string()
    } else {
        String::new()
    }
}

fn print_json(
    resolved: &loom_core::resolve::resolver::ResolvedProject,
    filesets: &loom_core::assemble::fileset::AssembledFilesets,
    build_context: &BuildContext,
) -> Result<(), LoomError> {
    let report_file = report_path(&build_context.build_dir);
    let report = if report_file.exists() {
        BuildReport::load_from_file(&report_file).ok()
    } else {
        None
    };

    let deps: Vec<serde_json::Value> = resolved
        .resolved_components
        .iter()
        .map(|c| {
            serde_json::json!({
                "name": c.manifest.component.name,
                "version": c.resolved_version.to_string(),
            })
        })
        .collect();

    let json = serde_json::json!({
        "project": resolved.project.project.name,
        "top_module": resolved.project.project.top_module,
        "target": resolved.project.target.as_ref().map(|t| serde_json::json!({
            "part": t.part,
            "backend": t.backend,
        })),
        "dependencies": deps,
        "synth_files": filesets.synth_files.len(),
        "constraint_files": filesets.constraint_files.len(),
        "last_build": report,
    });

    println!(
        "{}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
    Ok(())
}

fn run_workspace_overview(
    ws_manifest: &loom_core::manifest::WorkspaceManifest,
    members: &[loom_core::resolve::MemberPath],
    workspace_root: &std::path::Path,
    ctx: &GlobalContext,
) -> Result<(), LoomError> {
    let project_members: Vec<_> = members
        .iter()
        .filter(|m| m.kind == MemberKind::Project)
        .collect();

    let build_dir = workspace_root.join(
        ws_manifest
            .settings
            .build_dir
            .as_deref()
            .unwrap_or(".build"),
    );

    if ctx.json {
        let projects: Vec<serde_json::Value> = project_members
            .iter()
            .filter_map(|m| {
                let manifest = load_project_manifest(&m.path.join("project.toml")).ok()?;
                let project_build = build_dir.join(&manifest.project.name).join("default");
                let report = report_path(&project_build);
                let build_report = if report.exists() {
                    BuildReport::load_from_file(&report).ok()
                } else {
                    None
                };
                Some(serde_json::json!({
                    "name": manifest.project.name,
                    "target": manifest.target.as_ref().map(|t| &t.part),
                    "backend": manifest.target.as_ref().map(|t| &t.backend),
                    "last_build": build_report,
                }))
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "workspace": ws_manifest.workspace.name,
                "projects": projects,
            }))
            .unwrap_or_default()
        );
        return Ok(());
    }

    // Header
    ui::header(&[("\u{00B7}", &ws_manifest.workspace.name)]);

    ui::section_header(&format!(
        "Workspace Status ({} projects)",
        project_members.len()
    ));
    ui::blank();

    // Column header
    eprintln!(
        "    {:<20} {:<10} {:<8} {:<8} {:<10}",
        "Project", "Status", "Time", "LUT %", "WNS"
    );
    eprintln!("    {}", "\u{2500}".repeat(60));

    for member in &project_members {
        let manifest_path = member.path.join("project.toml");
        let name = match load_project_manifest(&manifest_path) {
            Ok(m) => m.project.name,
            Err(_) => {
                let dir_name = member
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?");
                dir_name.to_string()
            }
        };

        let project_build = build_dir.join(&name).join("default");
        let report_file = report_path(&project_build);

        if report_file.exists() {
            if let Ok(report) = BuildReport::load_from_file(&report_file) {
                let status_str = if report.status.success {
                    format!("{} passed", ui::CHECK)
                } else {
                    format!("{} failed", ui::CROSS)
                };
                let dur = report
                    .metrics
                    .duration_secs
                    .map(ui::format_duration)
                    .unwrap_or_else(|| "\u{2014}".to_string());
                let lut = report
                    .metrics
                    .utilization
                    .as_ref()
                    .map(|u| format!("{:.1}%", u.lut_percent))
                    .unwrap_or_else(|| "\u{2014}".to_string());
                let wns = report
                    .metrics
                    .timing
                    .as_ref()
                    .map(|t| format!("{:+.3}", t.wns))
                    .unwrap_or_else(|| "\u{2014}".to_string());

                eprintln!(
                    "    {:<20} {:<10} {:<8} {:<8} {:<10}",
                    name, status_str, dur, lut, wns
                );
            } else {
                eprintln!(
                    "    {:<20} {:<10} {:<8} {:<8} {:<10}",
                    name, "\u{2014} error", "\u{2014}", "\u{2014}", "\u{2014}"
                );
            }
        } else {
            eprintln!(
                "    {:<20} {:<10} {:<8} {:<8} {:<10}",
                name,
                format!("{} none", ui::DASH),
                "\u{2014}",
                "\u{2014}",
                "\u{2014}"
            );
        }
    }

    ui::blank();
    eprintln!("    Use 'loom status -p <name>' for detailed project status.");
    ui::blank();

    Ok(())
}
