use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use semver::{Version, VersionReq};

use crate::error::LoomError;
use crate::manifest::{ComponentManifest, DependencySpec, ProjectManifest};

use super::graph::{DependencyGraph, NodeId};

/// Effective target information derived from either `[target]` or a resolved platform.
#[derive(Debug, Clone)]
pub struct EffectiveTarget {
    pub part: String,
    pub backend: String,
    pub version: Option<String>,
}

/// The output of dependency resolution.
#[derive(Debug, Clone)]
pub struct ResolvedProject {
    pub project: ProjectManifest,
    pub project_root: PathBuf,
    pub workspace_root: PathBuf,
    /// Topologically ordered: dependencies before dependents.
    pub resolved_components: Vec<ResolvedComponent>,
    /// Resolved platform, if project specifies one.
    pub platform: Option<super::platform::ResolvedPlatform>,
    /// Active profile name, if any.
    pub active_profile: Option<String>,
    /// Selected variant per component (component_name -> variant_name).
    pub variant_selections: std::collections::HashMap<String, String>,
}

impl ResolvedProject {
    /// Get effective target from `[target]` block or resolved platform.
    /// Returns `None` only if neither is available (virtual platform with no part).
    pub fn effective_target(&self) -> Option<EffectiveTarget> {
        if let Some(ref target) = self.project.target {
            return Some(EffectiveTarget {
                part: target.part.clone(),
                backend: target.backend.clone(),
                version: target.version.clone(),
            });
        }
        if let Some(ref platform) = self.platform {
            if let Some(ref part) = platform.part {
                return Some(EffectiveTarget {
                    part: part.clone(),
                    backend: platform.backend.clone().unwrap_or_else(|| "vivado".to_string()),
                    version: platform.backend_version.clone(),
                });
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedComponent {
    pub manifest: ComponentManifest,
    pub source_path: PathBuf,
    pub resolved_version: Version,
}

/// A workspace-local dependency source.
pub struct WorkspaceDependencySource {
    components: Vec<(PathBuf, ComponentManifest)>,
}

impl WorkspaceDependencySource {
    pub fn new(components: Vec<(PathBuf, ComponentManifest)>) -> Self {
        Self { components }
    }

    /// Resolve a dependency name + version constraint to a component in the workspace.
    pub fn resolve(
        &self,
        name: &str,
        constraint: &VersionReq,
    ) -> Result<Option<(&PathBuf, &ComponentManifest)>, LoomError> {
        let matches: Vec<_> = self
            .components
            .iter()
            .filter(|(_, m)| {
                let comp_name = &m.component.name;
                comp_name == name || (comp_name.rsplit('/').next() == Some(name))
            })
            .collect();

        match matches.len() {
            0 => Ok(None),
            1 => {
                let (path, manifest) = &matches[0];
                let version = Version::parse(&manifest.component.version).map_err(|_| {
                    LoomError::InvalidVersion {
                        component: manifest.component.name.clone(),
                        version: manifest.component.version.clone(),
                    }
                })?;
                if constraint.matches(&version) {
                    Ok(Some((path, manifest)))
                } else {
                    Err(LoomError::VersionNotSatisfied {
                        dependency: name.to_owned(),
                        required: constraint.to_string(),
                        found: version.to_string(),
                        found_in: path.clone(),
                    })
                }
            }
            _ => Err(LoomError::AmbiguousDependency {
                name: name.to_owned(),
                candidates: matches
                    .iter()
                    .map(|(_, m)| m.component.name.clone())
                    .collect(),
            }),
        }
    }
}

/// Resolve all dependencies for a project using workspace components.
pub fn resolve_project(
    project: ProjectManifest,
    project_root: PathBuf,
    workspace_root: PathBuf,
    source: &WorkspaceDependencySource,
) -> Result<ResolvedProject, LoomError> {
    let mut graph = DependencyGraph::new();
    let root_id = graph.add_project();

    let mut visited = HashSet::new();
    resolve_dependencies_recursive(
        root_id,
        &project.dependencies,
        source,
        &mut graph,
        &mut visited,
    )?;

    let ordered = graph.topological_sort()?;

    // Reverse so leaf deps come first (petgraph toposort returns dependents before dependencies)
    let mut resolved_components: Vec<_> = ordered
        .into_iter()
        .filter_map(|node_id| graph.get_component(node_id))
        .map(|(path, manifest)| {
            let version = Version::parse(&manifest.component.version).unwrap();
            ResolvedComponent {
                manifest: manifest.clone(),
                source_path: path.clone(),
                resolved_version: version,
            }
        })
        .collect();
    resolved_components.reverse();

    Ok(ResolvedProject {
        project,
        project_root,
        workspace_root,
        resolved_components,
        platform: None,
        active_profile: None,
        variant_selections: HashMap::new(),
    })
}

fn resolve_dependencies_recursive(
    parent_id: NodeId,
    deps: &HashMap<String, DependencySpec>,
    source: &WorkspaceDependencySource,
    graph: &mut DependencyGraph,
    visited: &mut HashSet<String>,
) -> Result<(), LoomError> {
    for (name, spec) in deps {
        let constraint =
            VersionReq::parse(spec.version_string()).map_err(|_| LoomError::InvalidVersionReq {
                dependency: name.clone(),
                constraint: spec.version_string().to_owned(),
            })?;

        match source.resolve(name, &constraint)? {
            None => {
                return Err(LoomError::DependencyNotFound {
                    name: name.clone(),
                    constraint: constraint.to_string(),
                })
            }
            Some((path, manifest)) => {
                let child_id = graph.add_or_get_component(path, manifest);
                graph.add_edge(parent_id, child_id)?;

                // Only recurse if we haven't visited this component yet
                let comp_name = manifest.component.name.clone();
                if visited.insert(comp_name) {
                    let child_deps = manifest.dependencies.clone();
                    resolve_dependencies_recursive(child_id, &child_deps, source, graph, visited)?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::workspace::{
        discover_members, find_project, find_workspace_root, load_all_components,
    };

    fn fixture_path(name: &str) -> PathBuf {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        manifest_dir.join("../../tests/fixtures").join(name)
    }

    #[test]
    fn test_resolve_simple_project() {
        let fixture = fixture_path("simple_project");
        let (root, ws_manifest) = find_workspace_root(&fixture).unwrap();
        let members = discover_members(&root, &ws_manifest).unwrap();
        let all_components = load_all_components(&members).unwrap();
        let (project_root, project_manifest) = find_project(&members, Some("my_design")).unwrap();

        let source = WorkspaceDependencySource::new(all_components);
        let resolved = resolve_project(project_manifest, project_root, root, &source).unwrap();

        assert_eq!(resolved.resolved_components.len(), 1);
        assert_eq!(
            resolved.resolved_components[0].manifest.component.name,
            "testorg/axi_common"
        );
    }

    #[test]
    fn test_resolve_multi_component() {
        let fixture = fixture_path("multi_component");
        let (root, ws_manifest) = find_workspace_root(&fixture).unwrap();
        let members = discover_members(&root, &ws_manifest).unwrap();
        let all_components = load_all_components(&members).unwrap();
        let (project_root, project_manifest) = find_project(&members, Some("top_project")).unwrap();

        let source = WorkspaceDependencySource::new(all_components);
        let resolved = resolve_project(project_manifest, project_root, root, &source).unwrap();

        // top_project -> comp_a -> comp_b, so 2 components
        assert_eq!(resolved.resolved_components.len(), 2);

        // comp_b should come before comp_a (leaf deps first)
        let names: Vec<_> = resolved
            .resolved_components
            .iter()
            .map(|c| c.manifest.component.name.as_str())
            .collect();
        let comp_b_idx = names.iter().position(|n| *n == "testorg/comp_b").unwrap();
        let comp_a_idx = names.iter().position(|n| *n == "testorg/comp_a").unwrap();
        assert!(
            comp_b_idx < comp_a_idx,
            "comp_b should come before comp_a in topological order"
        );
    }

    #[test]
    fn test_detect_cycle() {
        let fixture = fixture_path("cycle_detection");
        let (root, ws_manifest) = find_workspace_root(&fixture).unwrap();
        let members = discover_members(&root, &ws_manifest).unwrap();
        let all_components = load_all_components(&members).unwrap();

        // Create a fake project that depends on cycle_a
        let project: ProjectManifest = toml::from_str(
            r#"
[project]
name = "cycle_test"
top_module = "top"
[target]
part = "xc7a35t"
backend = "vivado"
[dependencies]
cycle_a = ">=1.0.0"
"#,
        )
        .unwrap();

        let source = WorkspaceDependencySource::new(all_components);
        let result = resolve_project(project, fixture.clone(), root, &source);
        assert!(result.is_err());
        match result.unwrap_err() {
            LoomError::DependencyCycle { .. } => {}
            e => panic!("Expected DependencyCycle, got: {}", e),
        }
    }

    #[test]
    fn test_missing_dependency() {
        let project: ProjectManifest = toml::from_str(
            r#"
[project]
name = "test"
top_module = "top"
[target]
part = "xc7a35t"
backend = "vivado"
[dependencies]
nonexistent = ">=1.0.0"
"#,
        )
        .unwrap();

        let source = WorkspaceDependencySource::new(vec![]);
        let result = resolve_project(
            project,
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            &source,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            LoomError::DependencyNotFound { name, .. } => {
                assert_eq!(name, "nonexistent");
            }
            e => panic!("Expected DependencyNotFound, got: {}", e),
        }
    }
}
