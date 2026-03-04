use std::collections::HashMap;
use std::path::PathBuf;

use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};

use crate::error::LoomError;
use crate::manifest::ComponentManifest;

pub type NodeId = NodeIndex;

pub struct DependencyGraph {
    graph: DiGraph<NodeData, ()>,
    name_to_node: HashMap<String, NodeId>,
    project_node: Option<NodeId>,
}

enum NodeData {
    Project,
    Component {
        path: PathBuf,
        manifest: ComponentManifest,
    },
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            name_to_node: HashMap::new(),
            project_node: None,
        }
    }

    pub fn add_project(&mut self) -> NodeId {
        let id = self.graph.add_node(NodeData::Project);
        self.project_node = Some(id);
        id
    }

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
        self.name_to_node
            .insert(manifest.component.name.clone(), id);
        id
    }

    pub fn add_edge(&mut self, from: NodeId, to: NodeId) -> Result<(), LoomError> {
        if !self.graph.contains_edge(from, to) {
            self.graph.add_edge(from, to, ());
        }
        Ok(())
    }

    pub fn topological_sort(&self) -> Result<Vec<NodeId>, LoomError> {
        toposort(&self.graph, None).map_err(|cycle| {
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
