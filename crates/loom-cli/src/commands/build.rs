use clap::Args;

use loom_core::assemble::assemble_filesets;
use loom_core::build::checkpoint::{load_build_state, save_build_state, BuildState};
use loom_core::build::context::BuildContext;
use loom_core::build::report::{report_path, BuildReport};
use loom_core::build::validate_pre_build;
use loom_core::error::LoomError;
use loom_core::plugin::backend::BuildOptions;
use loom_core::resolve::lockfile::{
    check_staleness, generate_lockfile, load_lockfile, write_lockfile, LockfileStatus,
};
use loom_core::resolve::{
    discover_members, find_project, find_workspace_root, load_all_components, resolve_project,
    MemberKind, MemberPath, WorkspaceDependencySource,
};

use crate::backend_registry::get_backend;
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
}

pub fn run(args: BuildArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;

    // -- RESOLVE --
    if !ctx.quiet {
        eprintln!("  Resolving workspace...");
    }

    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let members = discover_members(&workspace_root, &ws_manifest)?;
    let all_components = load_all_components(&members)?;

    let project_name = args
        .project
        .clone()
        .or_else(|| detect_project_from_cwd(&cwd, &members));

    let (project_root, project_manifest) = match &project_name {
        Some(name) => find_project(&members, Some(name))?,
        None => find_project(&members, None)?,
    };

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
    let resolved = resolve_project(
        project_manifest,
        project_root,
        workspace_root.clone(),
        &source,
    )?;

    // Check lockfile
    match load_lockfile(&workspace_root)? {
        None => {
            if !ctx.quiet {
                eprintln!("  Generating lockfile...");
            }
            let lockfile = generate_lockfile(&resolved, &members)?;
            write_lockfile(&lockfile, &workspace_root)?;
        }
        Some(lockfile) => match check_staleness(&lockfile, &resolved, &members) {
            LockfileStatus::Valid => {
                if ctx.verbose > 0 {
                    eprintln!("  Lockfile is valid.");
                }
            }
            LockfileStatus::Stale(reasons) => {
                return Err(LoomError::LockfileStale { reasons });
            }
            LockfileStatus::Missing => unreachable!(),
        },
    }

    if !ctx.quiet {
        eprintln!(
            "  Resolved {} component(s).",
            resolved.resolved_components.len()
        );
    }

    // -- ASSEMBLE --
    if !ctx.quiet {
        eprintln!("  Assembling file-set...");
    }
    let filesets = assemble_filesets(&resolved)?;

    if ctx.verbose > 0 {
        eprintln!(
            "    {} source file(s), {} constraint file(s)",
            filesets.synth_files.len(),
            filesets.constraint_files.len()
        );
    }

    // -- VALIDATE --
    if !ctx.quiet {
        eprintln!("  Validating...");
    }

    let build_context = BuildContext::new(resolved.clone(), workspace_root.clone());
    let backend_name = resolved
        .project
        .target
        .as_ref()
        .map(|t| t.backend.as_str())
        .unwrap_or("vivado");
    let backend = get_backend(backend_name)?;
    let validation = validate_pre_build(&resolved, &filesets, &build_context, backend.as_ref())?;

    if validation.has_errors() {
        for diag in validation.errors() {
            eprintln!("  error: {}", diag.message);
            if let Some(path) = &diag.source_path {
                eprintln!("    at: {}", path.display());
            }
        }
        return Err(LoomError::ValidationFailed {
            error_count: validation.errors().len(),
        });
    }

    for diag in validation.warnings() {
        eprintln!("  warning: {}", diag.message);
    }

    // -- DRY RUN --
    if args.dry_run {
        let target = resolved.project.target.as_ref().unwrap();
        if ctx.json {
            let plan = serde_json::json!({
                "project": resolved.project.project.name,
                "target": { "part": target.part, "backend": target.backend },
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
            eprintln!();
            eprintln!(
                "  Phase 1: RESOLVE ({} components)",
                resolved.resolved_components.len()
            );
            eprintln!(
                "  Phase 3: ASSEMBLE ({} source files, {} constraint files)",
                filesets.synth_files.len(),
                filesets.constraint_files.len()
            );
            eprintln!(
                "  Phase 4: VALIDATE ({} errors, {} warnings)",
                validation.errors().len(),
                validation.warnings().len()
            );
            eprintln!();
            eprintln!("  Phase 5: BUILD (would execute -- dry run)");
            eprintln!("    Target:   {}", target.part);
            eprintln!("    Strategy: {}", args.strategy);
            eprintln!("    Backend:  {}", target.backend);
            if let Some(ref stop) = args.stop_after {
                eprintln!("    Stop after: {}", stop);
            }
            if let Some(ref start) = args.start_at {
                eprintln!("    Start at:   {}", start);
            }
            eprintln!();
            eprintln!("  Dry run complete. Use \"loom build\" to execute.");
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
                    eprintln!("  Resuming from {} checkpoint...", phase);
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
                );
            }
        }
        if !ctx.quiet {
            eprintln!("  No checkpoint found, starting fresh build...");
        }
    }

    if !ctx.quiet {
        let target = resolved.project.target.as_ref().unwrap();
        eprintln!(
            "  Building {} on {} with {} ...",
            resolved.project.project.name, target.part, target.backend
        );
    }

    let scripts = backend.generate_build_scripts(&resolved, &filesets, &build_context)?;
    let build_result = backend.execute_build(&scripts, &build_context)?;

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
    )
}

fn handle_build_result(
    build_result: &loom_core::plugin::backend::BuildResult,
    resolved: &loom_core::resolve::resolver::ResolvedProject,
    build_context: &BuildContext,
    backend_name: &str,
    strategy: &str,
    ctx: &GlobalContext,
) -> Result<(), LoomError> {
    // Generate and save build report
    let target = resolved.project.target.as_ref().unwrap();
    let report = BuildReport::from_build_result(
        &resolved.project.project.name,
        backend_name,
        target.version.as_deref().unwrap_or("unknown"),
        &target.part,
        strategy,
        build_result,
        &resolved.workspace_root,
    );
    let _ = report.write_to_file(&report_path(&build_context.build_dir));

    if ctx.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    }

    if build_result.success {
        if !ctx.quiet {
            eprintln!("  Build PASSED.");
            if let Some(bit) = &build_result.bitstream_path {
                eprintln!("  Bitstream: {}", bit.display());
            }
        }
        Ok(())
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

fn detect_project_from_cwd(cwd: &std::path::Path, members: &[MemberPath]) -> Option<String> {
    for member in members {
        if member.kind == MemberKind::Project
            && (cwd.starts_with(&member.path) || cwd == member.path)
        {
            let manifest_path = member.path.join("project.toml");
            if let Ok(m) = loom_core::manifest::load_project_manifest(&manifest_path) {
                return Some(m.project.name);
            }
        }
    }
    None
}
