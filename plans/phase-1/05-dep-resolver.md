# Task 05: Dependency Resolver

**Prerequisites:** Task 04 complete
**Goal:** Given a project and all workspace components, build a topologically-ordered dependency graph, detect cycles and version conflicts, and produce a `ResolvedProject`.

## Spec Reference
`system_plan.md` §3.5 (Dependency Resolution), §3.5.2 (Resolution Architecture), §15 Phase 1 Core Data Types

## File to Implement
`crates/loom-core/src/resolve/resolver.rs`
`crates/loom-core/src/resolve/graph.rs`

## Key Types

```rust
// resolver.rs
use std::path::PathBuf;
use std::collections::HashMap;
use semver::{Version, VersionReq};
use crate::manifest::{ComponentManifest, ProjectManifest, DependencySpec};
use crate::error::LoomError;

/// The output of dependency resolution — the full resolved project.
#[derive(Debug, Clone)]
pub struct ResolvedProject {
    pub project: ProjectManifest,
    pub project_root: PathBuf,           // directory containing project.toml
    pub workspace_root: PathBuf,
    /// Topologically ordered: dependencies before dependents.
    /// Index 0 = leaf dependencies with no deps of their own.
    pub resolved_components: Vec<ResolvedComponent>,
}

#[derive(Debug, Clone)]
pub struct ResolvedComponent {
    pub manifest: ComponentManifest,
    pub source_path: PathBuf,            // directory containing component.toml
    pub resolved_version: Version,
}

/// A workspace-local dependency source.
pub struct WorkspaceDependencySource {
    /// All workspace components: (directory, manifest)
    components: Vec<(PathBuf, ComponentManifest)>,
}

impl WorkspaceDependencySource {
    pub fn new(components: Vec<(PathBuf, ComponentManifest)>) -> Self {
        Self { components }
    }

    /// Resolve a dependency name + version constraint to a component in the workspace.
    /// `name` can be short name ("axi_common") or namespaced ("org/axi_common").
    /// Returns None if not found, Err if version constraint not satisfied.
    pub fn resolve(
        &self,
        name: &str,
        constraint: &VersionReq,
    ) -> Result<Option<(PathBuf, &ComponentManifest)>, LoomError> {
        // Match by short name (last part of "org/name") or full name
        let matches: Vec<_> = self.components.iter()
            .filter(|(_, m)| {
                let comp_name = &m.component.name;
                comp_name == name ||
                comp_name.rsplit('/').next().map_or(false, |short| short == name)
            })
            .collect();

        match matches.len() {
            0 => Ok(None),
            1 => {
                let (path, manifest) = matches[0];
                let version = Version::parse(&manifest.component.version)
                    .map_err(|_| LoomError::InvalidVersion {
                        component: manifest.component.name.clone(),
                        version: manifest.component.version.clone(),
                    })?;
                if constraint.matches(&version) {
                    Ok(Some((path.clone(), manifest)))
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
                candidates: matches.iter().map(|(_, m)| m.component.name.clone()).collect(),
            }),
        }
    }
}
```

## Resolver Algorithm

```rust
/// Resolve all dependencies for a project using workspace components.
/// Returns a ResolvedProject with topologically sorted components.
pub fn resolve_project(
    project: ProjectManifest,
    project_root: PathBuf,
    workspace_root: PathBuf,
    source: &WorkspaceDependencySource,
) -> Result<ResolvedProject, LoomError> {
    // Build the dependency graph by walking deps transitively
    let mut graph = DependencyGraph::new();

    // Add the project as root node
    let root_id = graph.add_project(&project);

    // Recursively resolve deps
    resolve_dependencies_recursive(root_id, &project.dependencies, source, &mut graph)?;

    // Topological sort — errors on cycles
    let ordered = graph.topological_sort()?;

    let resolved_components = ordered.into_iter()
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

    Ok(ResolvedProject {
        project,
        project_root,
        workspace_root,
        resolved_components,
    })
}

fn resolve_dependencies_recursive(
    parent_id: NodeId,
    deps: &HashMap<String, DependencySpec>,
    source: &WorkspaceDependencySource,
    graph: &mut DependencyGraph,
) -> Result<(), LoomError> {
    for (name, spec) in deps {
        let constraint = VersionReq::parse(spec.version_string())
            .map_err(|_| LoomError::InvalidVersionReq {
                dependency: name.clone(),
                constraint: spec.version_string().to_owned(),
            })?;

        match source.resolve(name, &constraint)? {
            None => return Err(LoomError::DependencyNotFound {
                name: name.clone(),
                constraint: constraint.to_string(),
            }),
            Some((path, manifest)) => {
                let child_id = graph.add_or_get_component(path, manifest);
                graph.add_edge(parent_id, child_id)?;

                // Recurse into this component's dependencies
                resolve_dependencies_recursive(
                    child_id,
                    &manifest.dependencies,
                    source,
                    graph,
                )?;
            }
        }
    }
    Ok(())
}
```

## Dependency Graph (`graph.rs`)

Use `petgraph` to detect cycles:

```rust
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::toposort;
use std::path::PathBuf;
use crate::manifest::ComponentManifest;
use crate::error::LoomError;

pub type NodeId = NodeIndex;

pub struct DependencyGraph {
    graph: DiGraph<NodeData, ()>,
    /// Map from component name to its node index (for deduplication)
    name_to_node: std::collections::HashMap<String, NodeId>,
    /// The project root node
    project_node: Option<NodeId>,
}

enum NodeData {
    Project,
    Component { path: PathBuf, manifest: ComponentManifest },
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            name_to_node: Default::default(),
            project_node: None,
        }
    }

    pub fn add_project(&mut self, _project: &ProjectManifest) -> NodeId {
        let id = self.graph.add_node(NodeData::Project);
        self.project_node = Some(id);
        id
    }

    /// Returns existing node if component already added (deduplication).
    pub fn add_or_get_component(
        &mut self,
        path: &PathBuf,
        manifest: &ComponentManifest,
    ) -> NodeId {
        if let Some(&existing) = self.name_to_node.get(&manifest.component.name) {
            return existing;
        }
        let id = self.graph.add_node(NodeData::Component {
            path: path.clone(),
            manifest: manifest.clone(),
        });
        self.name_to_node.insert(manifest.component.name.clone(), id);
        id
    }

    pub fn add_edge(&mut self, from: NodeId, to: NodeId) -> Result<(), LoomError> {
        // Check if edge already exists to avoid duplicates
        if !self.graph.contains_edge(from, to) {
            self.graph.add_edge(from, to, ());
        }
        Ok(())
    }

    pub fn topological_sort(&self) -> Result<Vec<NodeId>, LoomError> {
        toposort(&self.graph, None).map_err(|cycle| {
            // Find the component name involved in the cycle
            let node_idx = cycle.node_id();
            let name = match &self.graph[node_idx] {
                NodeData::Component { manifest, .. } => manifest.component.name.clone(),
                NodeData::Project => "<project>".to_string(),
            };
            LoomError::DependencyCycle { component: name }
        })
    }

    pub fn get_component(&self, id: NodeId) -> Option<(&PathBuf, &ComponentManifest)> {
        match &self.graph[id] {
            NodeData::Component { path, manifest } => Some((path, manifest)),
            NodeData::Project => None,
        }
    }
}
```

## Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::workspace::{find_workspace_root, discover_members, load_all_components, find_project};

    fn fixture_path(name: &str) -> std::path::PathBuf {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        manifest_dir.join("../../tests/fixtures").join(name)
    }

    #[test]
    fn test_resolve_simple_project() {
        let fixture = fixture_path("simple_project");
        let (root, ws_manifest) = find_workspace_root(&fixture).unwrap();
        let members = discover_members(&root, &ws_manifest).unwrap();
        let all_components = load_all_components(&members).unwrap();
        let (project_root, project_manifest) = find_project(&members, "my_design").unwrap();

        let source = WorkspaceDependencySource::new(all_components);
        let resolved = resolve_project(
            project_manifest, project_root, root, &source
        ).unwrap();

        // my_design depends on axi_common
        assert_eq!(resolved.resolved_components.len(), 1);
        assert_eq!(resolved.resolved_components[0].manifest.component.name, "testorg/axi_common");
    }

    #[test]
    fn test_detect_cycle() {
        // Use multi_component fixture which has a deliberate cycle for testing
        // (if the fixture has comp_a → comp_b → comp_a)
        // This test verifies we get DependencyCycle error
    }

    #[test]
    fn test_missing_dependency() {
        // Create a manifest with a dep that doesn't exist in workspace
        // Verify DependencyNotFound error
    }
}
```

## Error Variants to Add

```rust
DependencyNotFound { name: String, constraint: String },
DependencyCycle { component: String },
VersionNotSatisfied { dependency: String, required: String, found: String, found_in: PathBuf },
AmbiguousDependency { name: String, candidates: Vec<String> },
InvalidVersion { component: String, version: String },
InvalidVersionReq { dependency: String, constraint: String },
```

## Done When

- `cargo test -p loom-core` passes
- `resolve_project()` produces a topologically sorted list of components for `simple_project`
- Cycle detection returns `LoomError::DependencyCycle` for a cyclic fixture
- Missing dependency returns `LoomError::DependencyNotFound`
