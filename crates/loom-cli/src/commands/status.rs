use clap::Args;

use loom_core::assemble::assemble_filesets;
use loom_core::assemble::fileset::{ConstraintScope, FileLanguage};
use loom_core::build::context::BuildContext;
use loom_core::build::report::{report_path, BuildReport};
use loom_core::error::LoomError;
use loom_core::resolve::{
    discover_members, find_project, find_workspace_root, load_all_components, resolve_project,
    WorkspaceDependencySource,
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

    let (project_root, project_manifest) = match &args.project {
        Some(name) => find_project(&members, Some(name))?,
        None => find_project(&members, None)?,
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
