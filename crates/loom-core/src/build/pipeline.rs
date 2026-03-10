use std::path::{Path, PathBuf};

use crate::assemble::assemble_filesets;
use crate::build::checkpoint::{load_build_state, save_build_state, BuildState};
use crate::build::context::BuildContext;
use crate::build::progress::BuildEvent;
use crate::build::report::{report_path, BuildMetrics, BuildReport};
use crate::build::validate_pre_build;
use crate::error::LoomError;
use crate::generate::execute::{merge_generated_files, run_generate_phase};
use crate::generate::registry::PluginRegistry;
use crate::plugin::backend::{BackendPlugin, BuildOptions, BuildResult, Diagnostic};
use crate::resolve::lockfile::{
    check_staleness, generate_lockfile, load_lockfile, write_lockfile, LockfileStatus,
};
use crate::resolve::{
    discover_members, find_workspace_root, load_all_components, resolve_project,
    resolve_project_selection, WorkspaceDependencySource,
};

/// Configuration for a pipeline run (corresponds to CLI flags, but decoupled from clap).
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Project name (None = auto-detect from cwd).
    pub project_name: Option<String>,
    /// Build strategy.
    pub strategy: String,
    /// Build profile (simple or dimensional).
    pub profile: Option<String>,
    /// Show plan without building.
    pub dry_run: bool,
    /// Resume from last checkpoint.
    pub resume: bool,
    /// Stop after this build phase.
    pub stop_after: Option<String>,
    /// Start at this build phase (skip earlier phases).
    pub start_at: Option<String>,
    /// Reference checkpoint for incremental build.
    pub reference: Option<PathBuf>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            project_name: None,
            strategy: "default".to_string(),
            profile: None,
            dry_run: false,
            resume: false,
            stop_after: None,
            start_at: None,
            reference: None,
        }
    }
}

/// Events emitted during pipeline execution for progress reporting.
#[derive(Debug, Clone)]
pub enum PipelineEvent {
    /// A pipeline phase started (resolve, generate, assemble, validate, build, report).
    PhaseStarted(String),
    /// A pipeline phase completed with elapsed time.
    PhaseCompleted { phase: String, elapsed_secs: f64 },
    /// Resolve completed — number of components resolved.
    ResolveCompleted { component_count: usize },
    /// Assemble completed — file and constraint counts.
    AssembleCompleted {
        synth_file_count: usize,
        constraint_file_count: usize,
    },
    /// Generate phase completed — execution/cache stats.
    GenerateCompleted { executed: usize, cached: usize },
    /// Pre-build validation produced diagnostics.
    ValidationResult { diagnostics: Vec<Diagnostic> },
    /// Lockfile was generated or updated.
    LockfileGenerated,
    /// Dry run plan — scripts that would be executed.
    DryRunPlan { scripts: Vec<PathBuf> },
    /// Backend-level build event (phases, timing, utilization, etc.).
    BuildEvent(BuildEvent),
    /// Generate phase warning.
    GenerateWarning(String),
}

/// Result of a successful pipeline run.
pub struct PipelineResult {
    /// The build report (metrics, status, etc.).
    pub report: BuildReport,
    /// The final build state (for checkpoint/resume).
    pub build_state: BuildState,
    /// Backend build result.
    pub build_result: BuildResult,
}

/// Resolved context after the RESOLVE phase, before building.
pub struct ResolvedContext {
    pub project_name: String,
    pub backend_name: String,
    pub part: String,
    pub backend_version: String,
    pub component_count: usize,
    pub synth_file_count: usize,
    pub constraint_file_count: usize,
}

/// Execute the full build pipeline: RESOLVE → GENERATE → ASSEMBLE → VALIDATE → BUILD → REPORT.
///
/// The `backend` must be provided by the caller (since backend crates are not dependencies
/// of loom-core). The `progress` callback receives pipeline events for UI rendering.
pub fn run_pipeline(
    config: &PipelineConfig,
    backend: &dyn BackendPlugin,
    cwd: &Path,
    progress: Option<&(dyn Fn(PipelineEvent) + Send + Sync)>,
) -> Result<PipelineResult, LoomError> {
    let emit = |event: PipelineEvent| {
        if let Some(cb) = progress {
            cb(event);
        }
    };

    // ── RESOLVE ──────────────────────────────────────────────────────
    let resolve_start = std::time::Instant::now();
    emit(PipelineEvent::PhaseStarted("resolve".to_string()));

    let (workspace_root, ws_manifest) = find_workspace_root(cwd)?;
    let members = discover_members(&workspace_root, &ws_manifest)?;
    let all_components = load_all_components(&members)?;

    let (project_root, project_manifest) = resolve_project_selection(
        &members,
        config.project_name.as_deref(),
        Some(cwd),
        ws_manifest.settings.default_project.as_deref(),
    )?;

    let errors = project_manifest.validate();
    if !errors.is_empty() {
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
            let lockfile = generate_lockfile(&resolved, &members)?;
            write_lockfile(&lockfile, &workspace_root)?;
            emit(PipelineEvent::LockfileGenerated);
        }
        Some(lockfile) => match check_staleness(&lockfile, &resolved, &members) {
            LockfileStatus::Valid => {}
            LockfileStatus::Stale(reasons) => {
                return Err(LoomError::LockfileStale { reasons });
            }
            LockfileStatus::Missing => unreachable!(),
        },
    }

    let component_count = resolved.resolved_components.len();
    emit(PipelineEvent::ResolveCompleted { component_count });
    emit(PipelineEvent::PhaseCompleted {
        phase: "resolve".to_string(),
        elapsed_secs: resolve_start.elapsed().as_secs_f64(),
    });

    // ── EARLY ENVIRONMENT CHECK ──────────────────────────────────────
    // Fail fast: verify the backend tool is available before running
    // generators or assembling files. This avoids wasting time on
    // code generation only to discover the synthesis tool is missing.
    match backend.check_environment(
        resolved
            .effective_target()
            .as_ref()
            .and_then(|t| t.version.as_deref()),
    ) {
        Ok(env_status) if !env_status.is_ok() => {
            let mut reasons = Vec::new();
            if !env_status.version_matches {
                if let Some(required) = &env_status.required_version {
                    reasons.push(format!(
                        "Version mismatch: required {}, found {}",
                        required, env_status.version
                    ));
                }
            }
            if !env_status.license_ok {
                reasons.push("License check failed".to_string());
            }
            return Err(LoomError::ToolNotFound {
                tool: env_status.tool_name,
                message: reasons.join("; "),
            });
        }
        Err(e) => return Err(e),
        Ok(_) => {} // Tool is available, continue
    }

    // ── GENERATE ─────────────────────────────────────────────────────
    let generate_start = std::time::Instant::now();
    emit(PipelineEvent::PhaseStarted("generate".to_string()));

    let mut build_context = BuildContext::new(resolved.clone(), workspace_root.clone());
    build_context.strategy = config.strategy.clone();

    let registry = PluginRegistry::with_builtins();
    let gen_result = run_generate_phase(&resolved, &build_context, &registry, None)?;
    for w in &gen_result.warnings {
        emit(PipelineEvent::GenerateWarning(w.clone()));
    }
    emit(PipelineEvent::GenerateCompleted {
        executed: gen_result.executed,
        cached: gen_result.cached,
    });
    emit(PipelineEvent::PhaseCompleted {
        phase: "generate".to_string(),
        elapsed_secs: generate_start.elapsed().as_secs_f64(),
    });

    // ── ASSEMBLE ─────────────────────────────────────────────────────
    let assemble_start = std::time::Instant::now();
    emit(PipelineEvent::PhaseStarted("assemble".to_string()));

    let mut filesets = assemble_filesets(&resolved)?;
    merge_generated_files(&mut filesets, &gen_result.produced_files);

    emit(PipelineEvent::AssembleCompleted {
        synth_file_count: filesets.synth_files.len(),
        constraint_file_count: filesets.constraint_files.len(),
    });
    emit(PipelineEvent::PhaseCompleted {
        phase: "assemble".to_string(),
        elapsed_secs: assemble_start.elapsed().as_secs_f64(),
    });

    // ── VALIDATE ─────────────────────────────────────────────────────
    let validate_start = std::time::Instant::now();
    emit(PipelineEvent::PhaseStarted("validate".to_string()));

    let validation = validate_pre_build(&resolved, &filesets, &build_context, backend)?;
    emit(PipelineEvent::ValidationResult {
        diagnostics: validation.diagnostics.clone(),
    });

    if validation.has_errors() {
        return Err(LoomError::ValidationFailed {
            error_count: validation.errors().len(),
        });
    }

    emit(PipelineEvent::PhaseCompleted {
        phase: "validate".to_string(),
        elapsed_secs: validate_start.elapsed().as_secs_f64(),
    });

    // ── DRY RUN ──────────────────────────────────────────────────────
    let scripts = backend.generate_build_scripts(&resolved, &filesets, &build_context)?;

    if config.dry_run {
        emit(PipelineEvent::DryRunPlan {
            scripts: scripts.clone(),
        });

        let state = BuildState::new(String::new(), backend.plugin_name().to_string());
        let target = resolved.project.target.as_ref().unwrap();
        let report = BuildReport::from_build_result(
            &resolved.project.project.name,
            backend.plugin_name(),
            target.version.as_deref().unwrap_or("unknown"),
            &target.part,
            &config.strategy,
            &BuildResult {
                success: true,
                exit_code: 0,
                log_paths: vec![],
                bitstream_path: None,
                phases_completed: vec![],
                failure_phase: None,
                failure_message: None,
            },
            &workspace_root,
        );

        return Ok(PipelineResult {
            report,
            build_state: state,
            build_result: BuildResult {
                success: true,
                exit_code: 0,
                log_paths: vec![],
                bitstream_path: None,
                phases_completed: vec![],
                failure_phase: None,
                failure_message: None,
            },
        });
    }

    // ── BUILD ────────────────────────────────────────────────────────
    let build_options = BuildOptions {
        resume: config.resume,
        stop_after: config.stop_after.clone(),
        start_at: config.start_at.clone(),
        dry_run: false,
    };

    // Check for resume from checkpoint
    if config.resume {
        if let Some(state) = load_build_state(&build_context.build_dir)? {
            if let Some((phase, checkpoint)) = state.resume_checkpoint() {
                let build_result =
                    backend.resume_build(checkpoint, phase, &build_options, &build_context)?;

                let target = resolved.project.target.as_ref().unwrap();
                let report = BuildReport::from_build_result(
                    &resolved.project.project.name,
                    backend.plugin_name(),
                    target.version.as_deref().unwrap_or("unknown"),
                    &target.part,
                    &config.strategy,
                    &build_result,
                    &workspace_root,
                );
                let _ = report.write_to_file(&report_path(&build_context.build_dir));

                return Ok(PipelineResult {
                    report,
                    build_state: state,
                    build_result,
                });
            }
        }
    }

    emit(PipelineEvent::PhaseStarted("build".to_string()));
    let build_start = std::time::Instant::now();

    // Progress bridge: forward backend BuildEvents through our PipelineEvent wrapper
    let captured_metrics = std::sync::Mutex::new(BuildMetrics::default());
    let progress_bridge = |event: BuildEvent| {
        // Capture metrics as they arrive
        match &event {
            BuildEvent::UtilizationAvailable(util) => {
                captured_metrics.lock().unwrap().utilization = Some(util.clone());
            }
            BuildEvent::TimingAvailable { timing, .. } => {
                captured_metrics.lock().unwrap().timing = Some(timing.clone());
            }
            _ => {}
        }
        emit(PipelineEvent::BuildEvent(event));
    };

    let progress_ref: Option<&(dyn Fn(BuildEvent) + Send + Sync)> = if progress.is_some() {
        Some(&progress_bridge)
    } else {
        None
    };

    let build_result = backend.execute_build(&scripts, &build_context, progress_ref)?;

    let total_secs = build_start.elapsed().as_secs_f64();
    let mut metrics = captured_metrics.into_inner().unwrap();
    metrics.duration_secs = Some(total_secs);

    emit(PipelineEvent::PhaseCompleted {
        phase: "build".to_string(),
        elapsed_secs: total_secs,
    });

    // ── SAVE STATE ───────────────────────────────────────────────────
    let mut state = BuildState::new(String::new(), backend.plugin_name().to_string());
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

    // ── REPORT ───────────────────────────────────────────────────────
    let target = resolved.project.target.as_ref().unwrap();
    let mut report = BuildReport::from_build_result(
        &resolved.project.project.name,
        backend.plugin_name(),
        target.version.as_deref().unwrap_or("unknown"),
        &target.part,
        &config.strategy,
        &build_result,
        &workspace_root,
    );
    report.metrics = metrics;
    let _ = report.write_to_file(&report_path(&build_context.build_dir));

    if !build_result.success {
        // Return error but still provide the result through the error
        return Err(LoomError::BuildFailed {
            phase: build_result
                .failure_phase
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            log_path: build_result
                .log_paths
                .first()
                .cloned()
                .unwrap_or_else(|| build_context.build_dir.join("build.log")),
        });
    }

    Ok(PipelineResult {
        report,
        build_state: state,
        build_result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_config_default() {
        let config = PipelineConfig::default();
        assert_eq!(config.strategy, "default");
        assert!(!config.dry_run);
        assert!(!config.resume);
        assert!(config.project_name.is_none());
    }
}
