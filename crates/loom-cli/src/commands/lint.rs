use std::collections::HashSet;

use clap::Args;

use loom_core::error::LoomError;
use loom_core::resolve::{
    discover_members, find_workspace_root, load_all_components, resolve_project,
    resolve_project_selection, MemberKind, WorkspaceDependencySource,
};

use crate::ui::{self, Icon};
use crate::GlobalContext;

#[derive(Args)]
pub struct LintArgs {
    /// Project name (default: lint all members)
    #[arg(short = 'p', long)]
    pub project: Option<String>,
}

pub fn run(args: LintArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;
    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let members = discover_members(&workspace_root, &ws_manifest)?;

    let mut error_count = 0;

    if let Some(ref _project_name) = args.project {
        // Scoped lint: validate only the named project and its dependencies
        let all_components = load_all_components(&members)?;
        let (project_root, project_manifest) = resolve_project_selection(
            &members,
            args.project.as_deref(),
            None,
            ws_manifest.settings.default_project.as_deref(),
        )?;

        // Lint the project manifest
        let project_path = project_root.join("project.toml");
        for err in project_manifest.validate() {
            ui::status(
                Icon::Cross,
                "Error",
                &format!("{}: {}", project_path.display(), err),
            );
            error_count += 1;
        }

        // Resolve deps and lint just those components
        let source = WorkspaceDependencySource::new(all_components);
        match resolve_project(project_manifest, project_root, workspace_root, &source) {
            Ok(resolved) => {
                let dep_names: HashSet<_> = resolved
                    .resolved_components
                    .iter()
                    .map(|c| c.manifest.component.name.clone())
                    .collect();

                for member in members.iter().filter(|m| m.kind == MemberKind::Component) {
                    let manifest_path = member.path.join("component.toml");
                    match loom_core::manifest::load_component_manifest(&manifest_path) {
                        Err(e) => {
                            // Only report if it's a dependency
                            let dir_name = member
                                .path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("");
                            if dep_names.iter().any(|n| n.ends_with(dir_name)) {
                                ui::status(
                                    Icon::Cross,
                                    "Error",
                                    &format!("{}: {}", manifest_path.display(), e),
                                );
                                error_count += 1;
                            }
                        }
                        Ok(manifest) => {
                            if dep_names.contains(&manifest.component.name) {
                                for err in manifest.validate() {
                                    ui::status(
                                        Icon::Cross,
                                        "Error",
                                        &format!("{}: {}", manifest_path.display(), err),
                                    );
                                    error_count += 1;
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                ui::status(
                    Icon::Cross,
                    "Error",
                    &format!("dependency resolution: {}", e),
                );
                error_count += 1;
            }
        }
    } else {
        // Lint all components
        for member in members.iter().filter(|m| m.kind == MemberKind::Component) {
            let manifest_path = member.path.join("component.toml");
            match loom_core::manifest::load_component_manifest(&manifest_path) {
                Err(e) => {
                    ui::status(
                        Icon::Cross,
                        "Error",
                        &format!("{}: {}", manifest_path.display(), e),
                    );
                    error_count += 1;
                }
                Ok(manifest) => {
                    for err in manifest.validate() {
                        ui::status(
                            Icon::Cross,
                            "Error",
                            &format!("{}: {}", manifest_path.display(), err),
                        );
                        error_count += 1;
                    }
                }
            }
        }

        // Lint all projects
        for member in members.iter().filter(|m| m.kind == MemberKind::Project) {
            let manifest_path = member.path.join("project.toml");
            match loom_core::manifest::load_project_manifest(&manifest_path) {
                Err(e) => {
                    ui::status(
                        Icon::Cross,
                        "Error",
                        &format!("{}: {}", manifest_path.display(), e),
                    );
                    error_count += 1;
                }
                Ok(manifest) => {
                    for err in manifest.validate() {
                        ui::status(
                            Icon::Cross,
                            "Error",
                            &format!("{}: {}", manifest_path.display(), err),
                        );
                        error_count += 1;
                    }
                }
            }
        }
    }

    if !ctx.quiet {
        if error_count == 0 {
            ui::status(Icon::Check, "Lint", "all manifests are valid");
        } else {
            ui::status(Icon::Cross, "Lint", &format!("{} error(s)", error_count));
        }
    }

    if error_count > 0 {
        Err(LoomError::ValidationFailed { error_count })
    } else {
        Ok(())
    }
}
