use clap::Args;
use colored::Colorize;

use loom_core::build::context::BuildContext;
use loom_core::build::report::{report_path, BuildReport};
use loom_core::error::LoomError;
use loom_core::plugin::reporter::{
    ConsoleReporter, GitHubActionsReporter, JUnitReporter, JsonReporter, ReporterPlugin,
};
use loom_core::resolve::{
    discover_members, find_project, find_workspace_root, load_all_components, resolve_project,
    WorkspaceDependencySource,
};

use crate::ui::{self, Icon};
use crate::GlobalContext;

#[derive(Args)]
pub struct ReportArgs {
    /// Project name (default: auto-detect)
    #[arg(short = 'p', long)]
    pub project: Option<String>,

    /// Output format (console, json, github-actions, junit)
    #[arg(short, long, default_value = "console")]
    pub format: String,

    /// Diff against a previous report (by git ref or path)
    #[arg(long)]
    pub diff: Option<String>,

    /// Write report to file instead of stdout
    #[arg(short, long)]
    pub output: Option<std::path::PathBuf>,
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

    // Handle --diff
    if let Some(ref diff_ref) = args.diff {
        return run_diff(&report, diff_ref, &build_context.build_dir, ctx);
    }

    // Select reporter based on --format (or --json flag)
    let format = if ctx.json { "json" } else { &args.format };
    let reporter: Box<dyn ReporterPlugin> = match format {
        "json" => Box::new(JsonReporter),
        "github-actions" | "gh" => Box::new(GitHubActionsReporter),
        "junit" | "xml" => Box::new(JUnitReporter),
        _ => Box::new(ConsoleReporter),
    };

    let options = toml::Value::Table(toml::map::Map::new());
    let output = reporter.format_report(&report, &options)?;
    let text = String::from_utf8_lossy(&output.content);

    if let Some(ref out_path) = args.output {
        std::fs::write(out_path, &output.content).map_err(|e| LoomError::Io {
            path: out_path.clone(),
            source: e,
        })?;
        if !ctx.quiet {
            ui::status(
                Icon::Check,
                "Report",
                &format!("written to {}", out_path.display()),
            );
        }
    } else {
        println!("{}", text);
    }

    Ok(())
}

/// Diff current report against a previous one.
fn run_diff(
    current: &BuildReport,
    diff_ref: &str,
    build_dir: &std::path::Path,
    ctx: &GlobalContext,
) -> Result<(), LoomError> {
    // Try as a file path first, then as a git ref
    let previous = if std::path::Path::new(diff_ref).exists() {
        BuildReport::load_from_file(std::path::Path::new(diff_ref))?
    } else {
        // Try git show <ref>:<report_path>
        let report_rel = report_path(build_dir);
        let output = std::process::Command::new("git")
            .args(["show", &format!("{}:{}", diff_ref, report_rel.display())])
            .output()
            .map_err(|e| LoomError::Internal(format!("Failed to run git show: {}", e)))?;

        if !output.status.success() {
            return Err(LoomError::Internal(format!(
                "Could not load report from git ref '{}'. Try a file path instead.",
                diff_ref
            )));
        }

        let json_str = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str(&json_str)
            .map_err(|e| LoomError::Internal(format!("Failed to parse previous report: {}", e)))?
    };

    // Compute diffs
    if ctx.json {
        let diff = compute_metrics_diff(current, &previous);
        println!(
            "{}",
            serde_json::to_string_pretty(&diff).unwrap_or_default()
        );
    } else {
        print_metrics_diff(current, &previous);
    }

    Ok(())
}

fn compute_metrics_diff(current: &BuildReport, previous: &BuildReport) -> serde_json::Value {
    let mut diff = serde_json::Map::new();

    // Timing diff
    if let (Some(ct), Some(pt)) = (&current.metrics.timing, &previous.metrics.timing) {
        diff.insert(
            "timing".to_string(),
            serde_json::json!({
                "wns": {"current": ct.wns, "previous": pt.wns, "delta": ct.wns - pt.wns},
                "whs": {"current": ct.whs, "previous": pt.whs, "delta": ct.whs - pt.whs},
            }),
        );
    }

    // Utilization diff
    if let (Some(cu), Some(pu)) = (&current.metrics.utilization, &previous.metrics.utilization) {
        diff.insert(
            "utilization".to_string(),
            serde_json::json!({
                "lut_percent": {"current": cu.lut_percent, "previous": pu.lut_percent, "delta": cu.lut_percent - pu.lut_percent},
                "ff_percent": {"current": cu.ff_percent, "previous": pu.ff_percent, "delta": cu.ff_percent - pu.ff_percent},
                "bram_percent": {"current": cu.bram_percent, "previous": pu.bram_percent, "delta": cu.bram_percent - pu.bram_percent},
            }),
        );
    }

    // Duration diff
    if let (Some(cd), Some(pd)) = (
        current.metrics.duration_secs,
        previous.metrics.duration_secs,
    ) {
        diff.insert(
            "duration_secs".to_string(),
            serde_json::json!({"current": cd, "previous": pd, "delta": cd - pd}),
        );
    }

    serde_json::Value::Object(diff)
}

fn print_metrics_diff(current: &BuildReport, previous: &BuildReport) {
    ui::blank();
    ui::section_header(&format!("Metrics diff: {} vs previous", current.project));
    ui::blank();

    if let (Some(ct), Some(pt)) = (&current.metrics.timing, &previous.metrics.timing) {
        ui::section_header("Timing");
        print_delta("WNS", ct.wns, pt.wns, "ns", true);
        print_delta("WHS", ct.whs, pt.whs, "ns", true);
        ui::blank();
    }

    if let (Some(cu), Some(pu)) = (&current.metrics.utilization, &previous.metrics.utilization) {
        ui::section_header("Utilization");
        print_delta("LUT", cu.lut_percent, pu.lut_percent, "%", false);
        print_delta("FF", cu.ff_percent, pu.ff_percent, "%", false);
        print_delta("BRAM", cu.bram_percent, pu.bram_percent, "%", false);
        ui::blank();
    }

    if let (Some(cd), Some(pd)) = (
        current.metrics.duration_secs,
        previous.metrics.duration_secs,
    ) {
        ui::section_header("Duration");
        print_delta("Time", cd, pd, "s", false);
        ui::blank();
    }
}

fn print_delta(label: &str, current: f64, previous: f64, unit: &str, higher_is_better: bool) {
    let delta = current - previous;
    let (icon, icon_str) = if delta > 0.001 {
        if higher_is_better {
            (Icon::Check, ui::CHECK)
        } else {
            (Icon::Warning, ui::WARNING)
        }
    } else if delta < -0.001 {
        if higher_is_better {
            (Icon::Warning, ui::WARNING)
        } else {
            (Icon::Check, ui::CHECK)
        }
    } else {
        (Icon::Dot, ui::DASH)
    };
    let _ = icon; // used only for reference
    let detail = format!(
        "{:+.3}{}  (was {:.3}{}, \u{0394} {:+.3}{})  {}",
        current, unit, previous, unit, delta, unit, icon_str
    );
    eprintln!("    {:<6} {}", label, detail.dimmed());
}
