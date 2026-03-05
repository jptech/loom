use std::collections::HashMap;

use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};

use crate::error::LoomError;
use crate::generate::node::GeneratorNode;

#[derive(Debug)]
pub struct GeneratorDag {
    nodes: Vec<GeneratorNode>,
    /// Execution order (indices into `nodes`).
    execution_order: Vec<usize>,
    /// Whether any generator has outputs_unknown = true.
    pub has_unknown_outputs: bool,
}

impl GeneratorDag {
    /// Build the DAG from all generator nodes.
    pub fn build(nodes: Vec<GeneratorNode>) -> Result<Self, LoomError> {
        let n = nodes.len();
        let mut graph: DiGraph<usize, ()> = DiGraph::new();

        let node_indices: Vec<NodeIndex> = (0..n).map(|i| graph.add_node(i)).collect();

        // Build a name → index map for depends_on lookups
        let name_to_idx: HashMap<&str, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect();

        // Add edges from output/input overlap and explicit depends_on
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                if nodes[i].outputs_overlap_with_inputs(&nodes[j]) {
                    graph.add_edge(node_indices[i], node_indices[j], ());
                }
            }

            for dep_name in &nodes[i].decl.depends_on {
                // Try full ID first, then by generator name
                let dep_idx = if let Some(&idx) = name_to_idx.get(dep_name.as_str()) {
                    Some(idx)
                } else {
                    nodes
                        .iter()
                        .enumerate()
                        .find(|(_, n)| n.decl.name == *dep_name)
                        .map(|(idx, _)| idx)
                };

                match dep_idx {
                    Some(idx) => {
                        graph.add_edge(node_indices[idx], node_indices[i], ());
                    }
                    None => {
                        return Err(LoomError::Internal(format!(
                            "Generator '{}' depends_on '{}' which doesn't exist",
                            nodes[i].id, dep_name
                        )));
                    }
                }
            }
        }

        let sorted = toposort(&graph, None).map_err(|cycle| {
            let idx = cycle.node_id();
            let node_data = graph[idx];
            LoomError::Internal(format!(
                "Circular generator dependency detected involving '{}'",
                nodes[node_data].id
            ))
        })?;

        let execution_order: Vec<usize> = sorted.iter().map(|&ni| graph[ni]).collect();
        let has_unknown_outputs = nodes.iter().any(|n| n.decl.outputs_unknown);

        Ok(Self {
            nodes,
            execution_order,
            has_unknown_outputs,
        })
    }

    /// Iterate generators in execution order.
    pub fn execution_order(&self) -> impl Iterator<Item = &GeneratorNode> {
        self.execution_order.iter().map(|&i| &self.nodes[i])
    }

    /// Get the number of generators.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::GeneratorDecl;
    use std::path::PathBuf;

    fn make_node(
        source: &str,
        name: &str,
        inputs: Vec<&str>,
        outputs: Vec<&str>,
        depends_on: Vec<&str>,
    ) -> GeneratorNode {
        let base = PathBuf::from("/ws/comp");
        let build = PathBuf::from("/ws/.build");
        let decl = GeneratorDecl {
            name: name.to_string(),
            plugin: "command".to_string(),
            command: Some("echo test".to_string()),
            command_windows: None,
            inputs: inputs.into_iter().map(PathBuf::from).collect(),
            outputs: outputs.into_iter().map(PathBuf::from).collect(),
            fileset: "synth".to_string(),
            depends_on: depends_on.into_iter().map(String::from).collect(),
            cacheable: true,
            outputs_unknown: false,
            config: None,
        };
        GeneratorNode::from_decl(decl, source, &base, &build)
    }

    #[test]
    fn test_dag_output_input_detection() {
        let node_a = make_node("comp", "a", vec![], vec!["generated/foo.sv"], vec![]);
        let node_b = make_node("comp", "b", vec!["generated/foo.sv"], vec![], vec![]);

        let dag = GeneratorDag::build(vec![node_a, node_b]).unwrap();
        let order: Vec<&str> = dag.execution_order().map(|n| n.id.as_str()).collect();

        let pos_a = order.iter().position(|&id| id == "comp::a").unwrap();
        let pos_b = order.iter().position(|&id| id == "comp::b").unwrap();
        assert!(pos_a < pos_b, "A must run before B");
    }

    #[test]
    fn test_dag_explicit_depends_on() {
        let node_x = make_node("comp", "x", vec![], vec![], vec![]);
        let node_y = make_node("comp", "y", vec![], vec![], vec!["x"]);

        let dag = GeneratorDag::build(vec![node_x, node_y]).unwrap();
        let order: Vec<&str> = dag.execution_order().map(|n| n.id.as_str()).collect();

        let pos_x = order.iter().position(|&id| id == "comp::x").unwrap();
        let pos_y = order.iter().position(|&id| id == "comp::y").unwrap();
        assert!(pos_x < pos_y, "X must run before Y");
    }

    #[test]
    fn test_dag_cycle_detection() {
        let node_a = make_node("comp", "a", vec![], vec![], vec!["b"]);
        let node_b = make_node("comp", "b", vec![], vec![], vec!["a"]);

        let result = GeneratorDag::build(vec![node_a, node_b]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Circular"), "Error: {}", err);
    }

    #[test]
    fn test_dag_independent_nodes() {
        let node_a = make_node("comp", "a", vec!["x.yaml"], vec!["a.sv"], vec![]);
        let node_b = make_node("comp", "b", vec!["y.yaml"], vec!["b.sv"], vec![]);

        let dag = GeneratorDag::build(vec![node_a, node_b]).unwrap();
        assert_eq!(dag.len(), 2);
        let order: Vec<&str> = dag.execution_order().map(|n| n.id.as_str()).collect();
        assert!(order.contains(&"comp::a"));
        assert!(order.contains(&"comp::b"));
    }

    #[test]
    fn test_dag_unknown_outputs_flag() {
        let mut node = make_node("comp", "a", vec![], vec![], vec![]);
        node.decl.outputs_unknown = true;

        let dag = GeneratorDag::build(vec![node]).unwrap();
        assert!(dag.has_unknown_outputs);
    }

    #[test]
    fn test_dag_missing_dependency() {
        let node = make_node("comp", "a", vec![], vec![], vec!["nonexistent"]);
        let result = GeneratorDag::build(vec![node]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("doesn't exist"));
    }
}
