# Task 15: CLI Command Implementations

**Prerequisites:** Task 14 complete (all tasks 01-14 must be done)
**Goal:** Implement all Phase 1 CLI commands end-to-end. After this task, `loom build` successfully runs the full pipeline on the `simple_project` fixture.

## Spec Reference
`system_plan.md` §12.2, §15 Phase 1 Build Pipeline

## Backend Dispatch

The build pipeline selects a backend based on `[target].backend` in the project manifest. Phase 1 only supports `"vivado"`, but this pattern prepares for Quartus (Phase 4) and yosys (Phase 5).

```rust
// In loom-cli or loom-core, e.g. crates/loom-cli/src/backend_registry.rs
use loom_core::plugin::backend::BackendPlugin;
use loom_core::error::LoomError;

pub fn get_backend(name: &str) -> Result<Box<dyn BackendPlugin>, LoomError> {
    match name {
        "vivado" => Ok(Box::new(loom_vivado::VivadoBackend)),
        // Phase 4: "quartus" => Ok(Box::new(loom_quartus::QuartusBackend)),
        // Phase 5: "yosys" => Ok(Box::new(loom_yosys::YosysNextpnrBackend)),
        _ => Err(LoomError::ToolNotFound {
            tool: name.to_string(),
            message: format!(
                "Unknown backend '{}'. Supported backends: vivado. \
                 Check your project.toml [target].backend setting.",
                name
            ),
        }),
    }
}
```

## Commands to Implement

### 1. `loom build`

```rust
// commands/build.rs
use std::path::Path;
use loom_core::{
    resolve::workspace::{find_workspace_root, discover_members, load_all_components, find_project},
    resolve::resolver::{resolve_project, WorkspaceDependencySource},
    resolve::lockfile::{load_lockfile, generate_lockfile, write_lockfile, check_staleness, LockfileStatus},
    assemble::fileset::assemble_filesets,
    build::{context::BuildContext, validate::validate_pre_build},
    error::LoomError,
};
use crate::backend_registry::get_backend;

pub fn run(args: BuildArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir()
        .map_err(|e| LoomError::Io { path: ".".into(), source: e })?;

    // ── Phase 1: RESOLVE ──────────────────────────────────────────────────────
    if !ctx.quiet { eprintln!("  Resolving workspace..."); }

    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let members = discover_members(&workspace_root, &ws_manifest)?;
    let all_components = load_all_components(&members)?;

    // Find project: explicit arg → auto-detect from CWD
    let project_name = args.project.clone().or_else(|| {
        detect_project_from_cwd(&cwd, &members)
    });

    let project_name = project_name.ok_or_else(|| LoomError::ProjectNotFound {
        name: "<auto-detect>".to_string(),
    })?;

    let (project_root, project_manifest) = find_project(&members, &project_name)?;

    // Validate the project manifest
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

    // Check lockfile
    let source = WorkspaceDependencySource::new(all_components);
    let resolved = resolve_project(project_manifest, project_root, workspace_root.clone(), &source)?;

    let lockfile_path = workspace_root.join("loom.lock");
    match load_lockfile(&workspace_root)? {
        None => {
            if !ctx.quiet { eprintln!("  Generating lockfile..."); }
            let lockfile = generate_lockfile(&resolved, &members)?;
            write_lockfile(&lockfile, &workspace_root)?;
        }
        Some(lockfile) => {
            match check_staleness(&lockfile, &resolved, &members) {
                LockfileStatus::Valid => {
                    if ctx.verbose > 0 { eprintln!("  Lockfile is valid."); }
                }
                LockfileStatus::Stale(reasons) => {
                    return Err(LoomError::LockfileStale {
                        reasons: reasons.join("\n  - "),
                    });
                }
                LockfileStatus::Missing => unreachable!(), // We handled None above
            }
        }
    }

    if !ctx.quiet {
        eprintln!("  Resolved {} component(s).", resolved.resolved_components.len());
    }

    // ── Phase 2: GENERATE — skipped in Phase 1 ────────────────────────────────

    // ── Phase 3: ASSEMBLE ─────────────────────────────────────────────────────
    if !ctx.quiet { eprintln!("  Assembling file-set..."); }
    let filesets = assemble_filesets(&resolved)?;

    if ctx.verbose > 0 {
        eprintln!("    {} source file(s), {} constraint file(s)",
            filesets.synth_files.len(), filesets.constraint_files.len());
    }

    // ── Phase 4: VALIDATE ─────────────────────────────────────────────────────
    if !ctx.quiet { eprintln!("  Validating..."); }

    let build_context = BuildContext::new(resolved.clone(), workspace_root.clone());
    let backend_name = resolved.project.target.as_ref()
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

    // ── Phase 5: BUILD ────────────────────────────────────────────────────────
    if !ctx.quiet {
        let target = resolved.project.target.as_ref().unwrap();
        eprintln!("  Building {} on {} with {} ...",
            resolved.project.project.name,
            target.part,
            target.backend);
    }

    let scripts = backend.generate_build_scripts(&resolved, &filesets, &build_context)?;
    let build_result = backend.execute_build(&scripts, &build_context)?;

    // ── Phase 6-7: EXTRACT + REPORT ───────────────────────────────────────────
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
        return Err(LoomError::BuildFailed {
            phase: build_result.failure_phase.unwrap_or_else(|| "unknown".to_string()),
            log_path: log.unwrap_or_else(|| build_context.build_dir.join("build.log")),
        });
    }
}

/// Try to detect the project from the current working directory.
/// If CWD contains a project.toml, use that project's name.
fn detect_project_from_cwd(
    cwd: &Path,
    members: &[loom_core::resolve::workspace::MemberPath],
) -> Option<String> {
    // Check if CWD is inside a project directory
    for member in members {
        if member.kind == loom_core::resolve::workspace::MemberKind::Project {
            if cwd.starts_with(&member.path) || cwd == member.path {
                let manifest_path = member.path.join("project.toml");
                if let Ok(m) = loom_core::manifest::load_project_manifest(&manifest_path) {
                    return Some(m.project.name);
                }
            }
        }
    }
    None
}
```

### 2. `loom clean`

```rust
// commands/clean.rs
pub fn run(args: CleanArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir()
        .map_err(|e| LoomError::Io { path: ".".into(), source: e })?;
    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let build_dir = workspace_root.join(
        ws_manifest.settings.build_dir.as_deref().unwrap_or(".build")
    );

    if args.all {
        if build_dir.exists() {
            std::fs::remove_dir_all(&build_dir)
                .map_err(|e| LoomError::Io { path: build_dir.clone(), source: e })?;
            if !ctx.quiet { eprintln!("  Removed {}", build_dir.display()); }
        } else {
            if !ctx.quiet { eprintln!("  Nothing to clean."); }
        }
    } else {
        // Clean just the current project
        let members = discover_members(&workspace_root, &ws_manifest)?;
        let project_name = args.project.clone()
            .or_else(|| detect_project_from_cwd(&cwd, &members))
            .ok_or_else(|| LoomError::ProjectNotFound { name: "<auto-detect>".to_string() })?;

        let project_build_dir = build_dir.join(&project_name);
        if project_build_dir.exists() {
            std::fs::remove_dir_all(&project_build_dir)
                .map_err(|e| LoomError::Io { path: project_build_dir.clone(), source: e })?;
            if !ctx.quiet { eprintln!("  Removed {}", project_build_dir.display()); }
        } else {
            if !ctx.quiet { eprintln!("  Nothing to clean for project '{}'.", project_name); }
        }
    }
    Ok(())
}
```

### 3. `loom deps tree`

```rust
// commands/deps.rs
pub fn run(cmd: DepsCommands, ctx: &GlobalContext) -> Result<(), LoomError> {
    match cmd {
        DepsCommands::Tree(args) => run_tree(args, ctx),
        DepsCommands::Update(args) => run_update(args, ctx),
    }
}

fn run_tree(args: DepsTreeArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    // Resolve the project and print a tree
    let (resolved, _) = setup_resolved_project(args.project, ctx)?;

    if ctx.json {
        let tree: Vec<_> = resolved.resolved_components.iter().map(|c| {
            serde_json::json!({
                "name": c.manifest.component.name,
                "version": c.resolved_version.to_string(),
                "path": c.source_path.display().to_string(),
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&tree).unwrap_or_default());
    } else {
        println!("{} v{}", resolved.project.project.name, "0.0.0");
        for (i, comp) in resolved.resolved_components.iter().enumerate() {
            let is_last = i == resolved.resolved_components.len() - 1;
            let prefix = if is_last { "└──" } else { "├──" };
            println!("  {} {} v{}", prefix,
                comp.manifest.component.name,
                comp.resolved_version);
        }
    }
    Ok(())
}

fn run_update(_args: DepsUpdateArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    // Re-resolve and regenerate lockfile
    let cwd = std::env::current_dir()
        .map_err(|e| LoomError::Io { path: ".".into(), source: e })?;
    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let members = discover_members(&workspace_root, &ws_manifest)?;
    let all_components = load_all_components(&members)?;

    // Re-resolve all projects
    if !ctx.quiet { eprintln!("  Re-resolving dependencies..."); }

    // Find the first project to generate a comprehensive lockfile
    // (Phase 7: will handle all projects in workspace)
    for member in members.iter().filter(|m| m.kind == loom_core::resolve::workspace::MemberKind::Project) {
        let (_, project_manifest) = find_project(&members, &member.path.file_name().unwrap().to_string_lossy())?;
        let source = WorkspaceDependencySource::new(all_components.clone());
        let resolved = resolve_project(project_manifest, member.path.clone(), workspace_root.clone(), &source)?;
        let lockfile = generate_lockfile(&resolved, &members)?;
        write_lockfile(&lockfile, &workspace_root)?;
        if !ctx.quiet { eprintln!("  Lockfile updated."); }
        break;  // Phase 1: update from first project
    }

    Ok(())
}
```

### 4. `loom env check`

```rust
// commands/env.rs
pub fn run(cmd: EnvCommands, ctx: &GlobalContext) -> Result<(), LoomError> {
    match cmd {
        EnvCommands::Check => run_check(ctx),
    }
}

fn run_check(ctx: &GlobalContext) -> Result<(), LoomError> {
    // Phase 1: check all known backends. Phase 4+: configurable via args.
    let backend = get_backend("vivado")?;
    let status = backend.check_environment(None)?;

    if ctx.json {
        let json = serde_json::json!({
            "backend": status.tool_name,
            "path": status.tool_path.display().to_string(),
            "version": status.version,
            "version_ok": status.version_matches,
            "license_ok": status.license_ok,
            "license_detail": status.license_detail,
            "warnings": status.warnings,
        });
        println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
    } else {
        println!("Backend: {}", status.tool_name);
        println!("  Path:    {}", status.tool_path.display());
        println!("  Version: {}", status.version);
        if let Some(req) = &status.required_version {
            let ok_str = if status.version_matches { "✓" } else { "✗" };
            println!("  Required: {} {}", req, ok_str);
        }
        let license_str = if status.license_ok { "✓ OK" } else { "✗ FAILED" };
        println!("  License: {}", license_str);
        if let Some(detail) = &status.license_detail {
            println!("    ({})", detail);
        }
        for warning in &status.warnings {
            println!("  warning: {}", warning);
        }
    }

    if status.is_ok() { Ok(()) } else {
        Err(LoomError::ToolVersionMismatch {
            required: status.required_version.unwrap_or_default(),
            found: status.version,
        })
    }
}
```

### 5. `loom lint`

```rust
// commands/lint.rs
pub fn run(args: LintArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir()
        .map_err(|e| LoomError::Io { path: ".".into(), source: e })?;
    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let members = discover_members(&workspace_root, &ws_manifest)?;

    let mut error_count = 0;
    let mut warning_count = 0;

    // Lint all components
    for member in members.iter().filter(|m| m.kind == loom_core::resolve::workspace::MemberKind::Component) {
        let manifest_path = member.path.join("component.toml");
        match loom_core::manifest::load_component_manifest(&manifest_path) {
            Err(e) => {
                eprintln!("  error: {}: {}", manifest_path.display(), e);
                error_count += 1;
            }
            Ok(manifest) => {
                for err in manifest.validate() {
                    eprintln!("  error: {}: {}", manifest_path.display(), err);
                    error_count += 1;
                }
            }
        }
    }

    // Lint all projects
    for member in members.iter().filter(|m| m.kind == loom_core::resolve::workspace::MemberKind::Project) {
        let manifest_path = member.path.join("project.toml");
        match loom_core::manifest::load_project_manifest(&manifest_path) {
            Err(e) => {
                eprintln!("  error: {}: {}", manifest_path.display(), e);
                error_count += 1;
            }
            Ok(manifest) => {
                for err in manifest.validate() {
                    eprintln!("  error: {}: {}", manifest_path.display(), err);
                    error_count += 1;
                }
            }
        }
    }

    if !ctx.quiet {
        if error_count == 0 && warning_count == 0 {
            println!("  All manifests are valid.");
        } else {
            println!("  {} error(s), {} warning(s).", error_count, warning_count);
        }
    }

    if error_count > 0 {
        Err(LoomError::ValidationFailed { error_count })
    } else {
        Ok(())
    }
}
```

## Integration Test

Create an integration test that runs the full pipeline on the `simple_project` fixture (without Vivado — test that all phases up to BUILD succeed with a mock backend):

```rust
// tests/integration/build_pipeline.rs
// Uses the simple_project fixture to test the full pipeline
// Build succeeds when Vivado is available (marked #[ignore] otherwise)
```

## Done When

- `cargo build --bin loom` succeeds
- `loom lint` passes on `tests/fixtures/simple_project/`
- `loom deps tree` shows the correct tree for `simple_project`
- `loom env check` shows Vivado status (or actionable error if not installed)
- `loom clean` removes `.build/` directory
- `loom build` (with Vivado installed) runs the full pipeline
- All error cases produce the correct exit code (0, 1, 2, 3, or 4)

## Phase 1 Complete Verification

After Task 15, run this end-to-end check:

```bash
# From inside tests/fixtures/simple_project/
cd tests/fixtures/simple_project
loom lint              # should print "All manifests are valid."
loom deps tree         # should show my_design → axi_common
loom env check         # should show Vivado status
loom build             # should build (if Vivado is installed) or fail with exit code 3
```
