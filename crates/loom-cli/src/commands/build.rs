use std::io::IsTerminal;
use std::sync::Mutex;

use clap::Args;
use colored::Colorize;
use indicatif::ProgressBar;

use loom_core::assemble::assemble_filesets;
use loom_core::build::checkpoint::{load_build_state, save_build_state, BuildState};
use loom_core::build::context::BuildContext;
use loom_core::build::progress::BuildEvent;
use loom_core::build::report::{report_path, BuildMetrics, BuildReport};
use loom_core::build::validate_pre_build;
use loom_core::error::LoomError;
use loom_core::generate::execute::{merge_generated_files, run_generate_phase, GenerateEvent};
use loom_core::generate::plugins::command::CommandGenerator;
use loom_core::plugin::backend::BuildOptions;
use loom_core::plugin::generator::GeneratorPlugin;
use loom_core::resolve::lockfile::{
    check_staleness, generate_lockfile, load_lockfile, write_lockfile, LockfileStatus,
};
use loom_core::resolve::{
    apply_profile, discover_members, find_platform, find_workspace_root, load_all_components,
    resolve_platform, resolve_project, resolve_project_selection, WorkspaceDependencySource,
};

use crate::backend_registry::get_backend;
use crate::ui::{self, Icon};
use crate::GlobalContext;

#[derive(Args)]
pub struct BuildArgs {
    /// Project name (default: auto-detect from current directory)
    #[arg(short = 'p', long)]
    pub project: Option<String>,

    /// Build strategy
    #[arg(long, default_value = "default")]
    pub strategy: String,

    /// Parallel jobs
    #[arg(short = 'j', long)]
    pub jobs: Option<usize>,

    /// Resume from last checkpoint
    #[arg(long)]
    pub resume: bool,

    /// Stop after this build phase
    #[arg(long, value_name = "PHASE")]
    pub stop_after: Option<String>,

    /// Start at this build phase (skip earlier phases)
    #[arg(long, value_name = "PHASE")]
    pub start_at: Option<String>,

    /// Show execution plan without building
    #[arg(long)]
    pub dry_run: bool,

    /// Build profile (simple: "kcu116_port", dimensional: "board=kcu116,tier=reduced")
    #[arg(long)]
    pub profile: Option<String>,

    /// Build all profile combinations
    #[arg(long)]
    pub profile_all: bool,

    /// Run all declared strategies in parallel
    #[arg(long)]
    pub sweep: bool,

    /// Reference checkpoint for incremental build
    #[arg(long, value_name = "PATH")]
    pub reference: Option<std::path::PathBuf>,

    /// Show raw tool output (pass-through mode)
    #[arg(long)]
    pub passthrough: bool,
}

pub fn run(args: BuildArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;

    // -- RESOLVE --
    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let members = discover_members(&workspace_root, &ws_manifest)?;
    let all_components = load_all_components(&members)?;

    let (project_root, project_manifest) = resolve_project_selection(
        &members,
        args.project.as_deref(),
        Some(&cwd),
        ws_manifest.settings.default_project.as_deref(),
    )?;

    let errors = project_manifest.validate();
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("  Manifest error: {}", e);
        }
        return Err(LoomError::ManifestValidation {
            path: project_root.join("project.toml"),
            message: errors.join("; "),
        });
    }

    let source = WorkspaceDependencySource::new(all_components);
    let mut resolved = resolve_project(
        project_manifest,
        project_root,
        workspace_root.clone(),
        &source,
    )?;

    // Apply profile overlays (must happen BEFORE platform resolution)
    let profile_label = if let Some(ref profile_spec) = args.profile {
        Some(apply_profile(&mut resolved, profile_spec)?)
    } else {
        None
    };

    // Resolve platform if specified
    if let Some(ref platform_name) = resolved.project.project.platform {
        let (platform_root, platform_manifest) = find_platform(&members, platform_name)?;
        resolved.platform = Some(resolve_platform(&platform_manifest, &platform_root));
    }

    // Check lockfile
    match load_lockfile(&workspace_root)? {
        None => {
            let lockfile = generate_lockfile(&resolved, &members)?;
            write_lockfile(&lockfile, &workspace_root)?;
        }
        Some(lockfile) => match check_staleness(&lockfile, &resolved, &members) {
            LockfileStatus::Valid => {}
            LockfileStatus::Stale(reasons) => {
                return Err(LoomError::LockfileStale { reasons });
            }
            LockfileStatus::Missing => unreachable!(),
        },
    }

    // Derive effective target early so we can print the header before generators run
    let effective_target = resolved.effective_target();
    let part_str = effective_target
        .as_ref()
        .map(|t| t.part.as_str())
        .unwrap_or("(virtual)");
    let backend_str = effective_target
        .as_ref()
        .map(|t| t.backend.as_str())
        .unwrap_or("none");

    // Print header and early pipeline status
    if !ctx.quiet {
        ui::header(&[
            ("\u{00B7}", &resolved.project.project.name),
            ("\u{2192}", part_str),
            ("\u{00B7}", backend_str),
        ]);

        ui::status(
            Icon::Dot,
            "Resolve",
            &format!("{} components", resolved.resolved_components.len()),
        );
        if let Some(ref label) = profile_label {
            ui::status(Icon::Dot, "Profile", label);
        }
    }

    // -- GENERATE --
    let pre_build_context = BuildContext::new(resolved.clone(), workspace_root.clone());
    let get_plugin = |name: &str| -> Option<Box<dyn GeneratorPlugin>> {
        match name {
            "command" => Some(Box::new(CommandGenerator)),
            _ => None,
        }
    };

    let show_gen_progress = !ctx.quiet && !ctx.json && std::io::stderr().is_terminal();
    let gen_spinner: Mutex<Option<ProgressBar>> = Mutex::new(None);

    let gen_header_printed: Mutex<bool> = Mutex::new(false);

    let gen_callback = |event: GenerateEvent| match event {
        GenerateEvent::Started { ref name } => {
            // Print "Generate" header on first generator event
            if !*gen_header_printed.lock().unwrap() {
                *gen_header_printed.lock().unwrap() = true;
                if !ctx.quiet {
                    ui::status(Icon::Dot, "Generate", "");
                }
            }
            if show_gen_progress {
                if let Some(sp) = gen_spinner.lock().unwrap().take() {
                    sp.finish_and_clear();
                }
                let sp = ui::create_spinner(&format!(" {}", name));
                *gen_spinner.lock().unwrap() = Some(sp);
            }
        }
        GenerateEvent::Finished { ref status } => {
            if show_gen_progress {
                if let Some(sp) = gen_spinner.lock().unwrap().take() {
                    sp.finish_and_clear();
                }
            }
            // Print "Generate" header if this was a cache hit (no Started event)
            if !*gen_header_printed.lock().unwrap() {
                *gen_header_printed.lock().unwrap() = true;
                if !ctx.quiet {
                    ui::status(Icon::Dot, "Generate", "");
                }
            }
            if !ctx.quiet {
                let detail = if status.cached {
                    "cached".to_string()
                } else if let Some(secs) = status.elapsed_secs {
                    format!(
                        "{} file{}, {}",
                        status.output_count,
                        if status.output_count == 1 { "" } else { "s" },
                        ui::format_duration(secs)
                    )
                } else {
                    format!(
                        "{} file{}",
                        status.output_count,
                        if status.output_count == 1 { "" } else { "s" }
                    )
                };
                let icon = if status.cached {
                    ui::DOT.dimmed().to_string()
                } else {
                    ui::CHECK.green().to_string()
                };
                eprintln!("    {} {:<14} {}", icon, status.name, detail.dimmed());
            }
        }
    };

    let on_event: Option<&dyn Fn(GenerateEvent)> = if !ctx.quiet {
        Some(&gen_callback)
    } else {
        None
    };
    let gen_result = run_generate_phase(&resolved, &pre_build_context, &get_plugin, on_event)?;

    // -- ASSEMBLE --
    let mut filesets = assemble_filesets(&resolved)?;
    merge_generated_files(&mut filesets, &gen_result.produced_files);

    // -- VALIDATE --
    let mut build_context = BuildContext::new(resolved.clone(), workspace_root.clone());
    build_context.cancelled = ctx.cancelled.clone();
    let backend_name = effective_target
        .as_ref()
        .map(|t| t.backend.as_str())
        .unwrap_or("vivado");
    let backend = get_backend(backend_name)?;
    let validation = validate_pre_build(&resolved, &filesets, &build_context, backend.as_ref())?;

    if !ctx.quiet {
        ui::status(
            Icon::Dot,
            "Assemble",
            &format!(
                "{} files, {} constraints",
                filesets.synth_files.len(),
                filesets.constraint_files.len()
            ),
        );
    }

    if validation.has_errors() {
        if !ctx.quiet {
            ui::status(
                Icon::Cross,
                "Validate",
                &format!("{} error(s)", validation.errors().len()),
            );
            for diag in validation.errors() {
                ui::sub_item(&diag.message, false);
                if let Some(path) = &diag.source_path {
                    eprintln!("      at: {}", path.display());
                }
            }
        }
        return Err(LoomError::ValidationFailed {
            error_count: validation.errors().len(),
        });
    }

    let warning_count = validation.warnings().len();
    if !ctx.quiet {
        if warning_count > 0 {
            ui::status(
                Icon::Dot,
                "Validate",
                &format!(
                    "passed ({} warning{})",
                    warning_count,
                    if warning_count == 1 { "" } else { "s" }
                ),
            );
            for diag in validation.warnings() {
                ui::sub_warning(&diag.message);
            }
        } else {
            ui::status(Icon::Dot, "Validate", "passed");
        }
    }

    // -- DRY RUN --
    if args.dry_run {
        if ctx.json {
            let plan = serde_json::json!({
                "project": resolved.project.project.name,
                "target": {
                    "part": effective_target.as_ref().map(|t| t.part.as_str()).unwrap_or("(virtual)"),
                    "backend": effective_target.as_ref().map(|t| t.backend.as_str()).unwrap_or("none"),
                },
                "strategy": args.strategy,
                "components": resolved.resolved_components.len(),
                "synth_files": filesets.synth_files.len(),
                "constraint_files": filesets.constraint_files.len(),
                "validation_errors": validation.errors().len(),
                "validation_warnings": validation.warnings().len(),
                "dry_run": true,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&plan).unwrap_or_default()
            );
        } else {
            ui::blank();
            ui::status(Icon::Dot, "Build", "dry run — would execute");
            ui::summary_detail("Target", part_str);
            ui::summary_detail("Strategy", &args.strategy);
            ui::summary_detail("Backend", backend_str);
            if let Some(ref stop) = args.stop_after {
                ui::summary_detail("Stop after", stop);
            }
            if let Some(ref start) = args.start_at {
                ui::summary_detail("Start at", start);
            }
        }
        return Ok(());
    }

    // -- BUILD --
    let build_options = BuildOptions {
        resume: args.resume,
        stop_after: args.stop_after.clone(),
        start_at: args.start_at.clone(),
        dry_run: false,
    };

    // Check for resume
    if args.resume {
        if let Some(state) = load_build_state(&build_context.build_dir)? {
            if let Some((phase, checkpoint)) = state.resume_checkpoint() {
                if !ctx.quiet {
                    ui::status(Icon::Dot, "Resume", &format!("from {} checkpoint", phase));
                }
                let build_result =
                    backend.resume_build(checkpoint, phase, &build_options, &build_context)?;

                return handle_build_result(
                    &build_result,
                    &resolved,
                    &build_context,
                    backend_name,
                    &args.strategy,
                    ctx,
                    None,
                );
            }
        }
        if !ctx.quiet {
            ui::status(Icon::Dot, "Resume", "no checkpoint, starting fresh");
        }
    }

    if !ctx.quiet {
        ui::blank();
    }

    let scripts = backend.generate_build_scripts(&resolved, &filesets, &build_context)?;

    // Set up progress callback
    let show_progress = !ctx.quiet && !ctx.json && std::io::stderr().is_terminal();
    let verbose_mode = args.passthrough || ctx.verbose > 0;

    let captured_metrics = Mutex::new(BuildMetrics::default());
    let spinner: Mutex<Option<ProgressBar>> = Mutex::new(None);
    let build_start = std::time::Instant::now();

    // Pre-set spinner to cover Vivado startup delay (replaced by first Activity/Phase event)
    if show_progress {
        let sp = ui::create_spinner("Build");
        *spinner.lock().unwrap() = Some(sp);
    }

    let progress_callback = |event: BuildEvent| match &event {
        BuildEvent::VerboseLine(line) => {
            if verbose_mode {
                eprintln!("  {} {}", format!("[{}]", backend_name).dimmed(), line);
            }
        }
        BuildEvent::PhaseStarted { phase } => {
            if show_progress {
                if let Some(sp) = spinner.lock().unwrap().take() {
                    sp.finish_and_clear();
                }
                let sp = ui::create_spinner(&ui::capitalize(phase));
                *spinner.lock().unwrap() = Some(sp);
            }
        }
        BuildEvent::PhaseCompleted {
            phase,
            elapsed_secs,
            memory_mb,
        } => {
            if show_progress {
                if let Some(sp) = spinner.lock().unwrap().take() {
                    sp.finish_and_clear();
                }
                match memory_mb {
                    Some(mb) => {
                        ui::status_with_metrics(
                            Icon::Check,
                            &ui::capitalize(phase),
                            *elapsed_secs,
                            *mb as u64,
                        );
                    }
                    None => {
                        ui::status(
                            Icon::Check,
                            &ui::capitalize(phase),
                            &ui::format_duration(*elapsed_secs),
                        );
                    }
                }
            }
        }
        BuildEvent::UtilizationAvailable(util) => {
            captured_metrics.lock().unwrap().utilization = Some(util.clone());
            if show_progress {
                // Collect non-trivial resource categories (skip if 0 available = not applicable)
                let mut entries: Vec<(&str, f64)> = Vec::new();
                if util.lut_available > 0 {
                    entries.push(("LUT", util.lut_percent));
                }
                if util.ff_available > 0 {
                    entries.push(("FF", util.ff_percent));
                }
                if util.bram_available > 0 {
                    entries.push(("BRAM", util.bram_percent));
                }
                // Show in pairs
                let mut i = 0;
                while i < entries.len() {
                    let is_last = i + 2 >= entries.len();
                    if i + 1 < entries.len() {
                        ui::util_pair(
                            entries[i].0,
                            entries[i].1,
                            entries[i + 1].0,
                            entries[i + 1].1,
                            is_last,
                        );
                        i += 2;
                    } else {
                        // Odd entry: show solo as a pair with empty second slot
                        ui::util_single(entries[i].0, entries[i].1, true);
                        i += 1;
                    }
                }
            }
        }
        BuildEvent::TimingAvailable { stage, timing } => {
            captured_metrics.lock().unwrap().timing = Some(timing.clone());
            if show_progress {
                let label = if stage == "post_place" {
                    "Timing (est)"
                } else {
                    "Timing"
                };
                let has_clocks = !timing.clocks.is_empty();
                ui::timing_line(label, timing.wns, timing.whs, !has_clocks);
                if has_clocks {
                    let timing_cfg = resolved
                        .project
                        .build
                        .as_ref()
                        .and_then(|b| b.timing.as_ref());
                    let hide_gen = timing_cfg.is_some_and(|t| t.hide_generated());
                    let exclude = timing_cfg
                        .map(|t| t.exclude_clocks.as_slice())
                        .unwrap_or(&[]);
                    ui::clock_table(&timing.clocks, true, hide_gen, exclude);
                }
            }
        }
        BuildEvent::IntermediateTiming { .. } => {}
        BuildEvent::CriticalWarning(msg) => {
            if show_progress {
                let guard = spinner.lock().unwrap();
                if let Some(sp) = guard.as_ref() {
                    sp.suspend(|| {
                        ui::sub_warning(msg);
                    });
                } else {
                    drop(guard);
                    ui::sub_warning(msg);
                }
            }
        }
        BuildEvent::Warning(_) => {}
        BuildEvent::DrcResult { errors } => {
            if show_progress && *errors > 0 {
                ui::sub_item(&format!("DRC: {} error(s)", errors), true);
            }
        }
        BuildEvent::SynthesisSummary {
            errors,
            critical_warnings,
            warnings,
        } => {
            if show_progress && (*errors > 0 || *critical_warnings > 0) {
                ui::sub_item(
                    &format!(
                        "Synthesis: {} error(s), {} critical warning(s), {} warning(s)",
                        errors, critical_warnings, warnings
                    ),
                    true,
                );
            }
        }
        BuildEvent::Activity(msg) => {
            if show_progress {
                if let Some(sp) = spinner.lock().unwrap().take() {
                    sp.finish_and_clear();
                }
                let sp = ui::create_spinner(msg);
                *spinner.lock().unwrap() = Some(sp);
            }
        }
        BuildEvent::ActivityDone => {
            if show_progress {
                if let Some(sp) = spinner.lock().unwrap().take() {
                    sp.finish_and_clear();
                }
            }
        }
    };

    let progress_ref: Option<&(dyn Fn(BuildEvent) + Send + Sync)> = if !ctx.quiet && !ctx.json {
        Some(&progress_callback)
    } else {
        None
    };

    let build_result = backend.execute_build(&scripts, &build_context, progress_ref)?;

    // Clean up spinner
    if let Some(sp) = spinner.lock().unwrap().take() {
        sp.finish_and_clear();
    }

    let total_secs = build_start.elapsed().as_secs_f64();
    let mut metrics = captured_metrics.into_inner().unwrap();
    metrics.duration_secs = Some(total_secs);

    // Save build state
    let mut state = BuildState::new("".to_string(), backend_name.to_string());
    for phase in &build_result.phases_completed {
        state.complete_phase(phase, None);
    }
    if !build_result.success {
        if let Some(ref fail_phase) = build_result.failure_phase {
            let log = build_result.log_paths.first().cloned().unwrap_or_default();
            state.fail_phase(
                fail_phase,
                build_result.exit_code,
                log,
                build_result.failure_message.clone(),
            );
        }
    }
    let _ = save_build_state(&state, &build_context.build_dir);

    handle_build_result(
        &build_result,
        &resolved,
        &build_context,
        backend_name,
        &args.strategy,
        ctx,
        Some(metrics),
    )
}

fn handle_build_result(
    build_result: &loom_core::plugin::backend::BuildResult,
    resolved: &loom_core::resolve::resolver::ResolvedProject,
    build_context: &BuildContext,
    backend_name: &str,
    strategy: &str,
    ctx: &GlobalContext,
    metrics: Option<BuildMetrics>,
) -> Result<(), LoomError> {
    // Generate and save build report
    let effective = resolved.effective_target();
    let part = effective
        .as_ref()
        .map(|t| t.part.as_str())
        .unwrap_or("(virtual)");
    let version = effective
        .as_ref()
        .and_then(|t| t.version.as_deref())
        .unwrap_or("unknown");
    let mut report = BuildReport::from_build_result(
        &resolved.project.project.name,
        backend_name,
        version,
        part,
        strategy,
        build_result,
        &resolved.workspace_root,
    );
    if let Some(m) = metrics {
        report.metrics = m;
    }
    let _ = report.write_to_file(&report_path(&build_context.build_dir));

    if ctx.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    }

    if build_result.success {
        if !ctx.quiet {
            ui::summary_pass("Build passed", report.metrics.duration_secs);
            if let Some(bit) = &build_result.bitstream_path {
                ui::summary_detail("Bitstream", &bit.display().to_string());
            }
            let (report_count, checkpoint_count) = count_build_artifacts(&build_context.build_dir);
            if report_count > 0 {
                ui::summary_detail(
                    "Reports",
                    &format!(
                        "{} files in {}",
                        report_count,
                        build_context.build_dir.display()
                    ),
                );
            }
            if checkpoint_count > 0 {
                ui::summary_detail(
                    "Checkpoints",
                    &format!(
                        "{} files in {}",
                        checkpoint_count,
                        build_context.build_dir.display()
                    ),
                );
            }
        }
        Ok(())
    } else if build_result.failure_phase.as_deref() == Some("interrupted") {
        if !ctx.quiet {
            ui::summary_fail("Build interrupted", "progress saved");
            if !build_result.phases_completed.is_empty() {
                ui::summary_detail("Completed", &build_result.phases_completed.join(", "));
            }
        }
        Err(LoomError::Interrupted)
    } else {
        let log = build_result.log_paths.first().cloned();
        Err(LoomError::BuildFailed {
            phase: build_result
                .failure_phase
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            log_path: log.unwrap_or_else(|| build_context.build_dir.join("build.log")),
        })
    }
}

/// Count report (.rpt) and checkpoint (.dcp) files in the build directory.
fn count_build_artifacts(build_dir: &std::path::Path) -> (usize, usize) {
    let entries: Vec<_> = std::fs::read_dir(build_dir)
        .into_iter()
        .flatten()
        .flatten()
        .collect();
    let reports = entries
        .iter()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "rpt"))
        .count();
    let checkpoints = entries
        .iter()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "dcp"))
        .count();
    (reports, checkpoints)
}
