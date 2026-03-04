use clap::Args;

use loom_core::assemble::assemble_filesets;
use loom_core::build::context::BuildContext;
use loom_core::build::validate_pre_build;
use loom_core::error::LoomError;
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

    /// Parallel jobs (parsed but ignored in Phase 1)
    #[arg(short = 'j', long)]
    pub jobs: Option<usize>,
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

    // -- BUILD --
    if !ctx.quiet {
        let target = resolved.project.target.as_ref().unwrap();
        eprintln!(
            "  Building {} on {} with {} ...",
            resolved.project.project.name, target.part, target.backend
        );
    }

    let scripts = backend.generate_build_scripts(&resolved, &filesets, &build_context)?;
    let build_result = backend.execute_build(&scripts, &build_context)?;

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
