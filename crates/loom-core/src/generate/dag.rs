use std::collections::{HashMap, HashSet};

use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::Dfs;
use petgraph::Direction;

use crate::error::LoomError;
use crate::generate::node::GeneratorNode;

#[derive(Debug)]
pub struct GeneratorDag {
    nodes: Vec<GeneratorNode>,
    /// Execution order (indices into `nodes`).
    execution_order: Vec<usize>,
    /// Whether any generator has outputs_unknown = true.
    pub has_unknown_outputs: bool,
    /// The dependency graph (edges: upstream → downstream).
    graph: DiGraph<usize, ()>,
    /// Map from node index in `nodes` to petgraph NodeIndex.
    node_indices: Vec<NodeIndex>,
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
            graph,
            node_indices,
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

    /// Return indices of all direct upstream dependencies for a given node index.
    pub fn upstream_of(&self, node_idx: usize) -> Vec<usize> {
        self.graph
            .neighbors_directed(self.node_indices[node_idx], Direction::Incoming)
            .map(|ni| self.graph[ni])
            .collect()
    }

    /// Return indices of all nodes reachable downstream from a given node index.
    pub fn downstream_of(&self, node_idx: usize) -> HashSet<usize> {
        let mut result = HashSet::new();
        let mut dfs = Dfs::new(&self.graph, self.node_indices[node_idx]);
        // Skip the start node itself
        dfs.next(&self.graph);
        while let Some(ni) = dfs.next(&self.graph) {
            result.insert(self.graph[ni]);
        }
        result
    }

    /// Get a node by its index in the nodes array.
    pub fn node(&self, idx: usize) -> &GeneratorNode {
        &self.nodes[idx]
    }

    /// Get a node's index by its ID string.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    /// Get the indices of nodes that have outputs_unknown = true.
    pub fn unknown_output_indices(&self) -> Vec<usize> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.decl.outputs_unknown)
            .map(|(i, _)| i)
            .collect()
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
        // Node A outputs foo.sv — resolved to build dir
        let node_a = make_node("comp", "a", vec![], vec!["foo.sv"], vec![]);
        // Node B's input must use the absolute path matching A's resolved output
        let a_output = node_a.resolved_outputs[0].to_string_lossy().to_string();
        let node_b = make_node("comp", "b", vec![&a_output], vec![], vec![]);

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

    #[test]
    fn test_upstream_of() {
        // A → B → C (via depends_on)
        let node_a = make_node("comp", "a", vec![], vec![], vec![]);
        let node_b = make_node("comp", "b", vec![], vec![], vec!["a"]);
        let node_c = make_node("comp", "c", vec![], vec![], vec!["b"]);

        let dag = GeneratorDag::build(vec![node_a, node_b, node_c]).unwrap();

        let idx_a = dag.index_of("comp::a").unwrap();
        let idx_b = dag.index_of("comp::b").unwrap();
        let idx_c = dag.index_of("comp::c").unwrap();

        // A has no upstream
        assert!(dag.upstream_of(idx_a).is_empty());
        // B's upstream is A
        assert_eq!(dag.upstream_of(idx_b), vec![idx_a]);
        // C's upstream is B
        assert_eq!(dag.upstream_of(idx_c), vec![idx_b]);
    }

    #[test]
    fn test_downstream_of() {
        // A → B → C
        let node_a = make_node("comp", "a", vec![], vec![], vec![]);
        let node_b = make_node("comp", "b", vec![], vec![], vec!["a"]);
        let node_c = make_node("comp", "c", vec![], vec![], vec!["b"]);

        let dag = GeneratorDag::build(vec![node_a, node_b, node_c]).unwrap();

        let idx_a = dag.index_of("comp::a").unwrap();
        let idx_b = dag.index_of("comp::b").unwrap();
        let idx_c = dag.index_of("comp::c").unwrap();

        // A's downstream: B and C
        let down_a = dag.downstream_of(idx_a);
        assert!(down_a.contains(&idx_b));
        assert!(down_a.contains(&idx_c));
        assert_eq!(down_a.len(), 2);

        // B's downstream: only C
        let down_b = dag.downstream_of(idx_b);
        assert_eq!(down_b, HashSet::from([idx_c]));

        // C has no downstream
        assert!(dag.downstream_of(idx_c).is_empty());
    }

    #[test]
    fn test_diamond_dependency() {
        // Diamond: A → B, A → C, B → D, C → D
        let node_a = make_node("comp", "a", vec![], vec![], vec![]);
        let node_b = make_node("comp", "b", vec![], vec![], vec!["a"]);
        let node_c = make_node("comp", "c", vec![], vec![], vec!["a"]);
        let node_d = make_node("comp", "d", vec![], vec![], vec!["b", "c"]);

        let dag = GeneratorDag::build(vec![node_a, node_b, node_c, node_d]).unwrap();
        let order: Vec<&str> = dag.execution_order().map(|n| n.id.as_str()).collect();

        let pos_a = order.iter().position(|&id| id == "comp::a").unwrap();
        let pos_b = order.iter().position(|&id| id == "comp::b").unwrap();
        let pos_c = order.iter().position(|&id| id == "comp::c").unwrap();
        let pos_d = order.iter().position(|&id| id == "comp::d").unwrap();

        assert!(pos_a < pos_b);
        assert!(pos_a < pos_c);
        assert!(pos_b < pos_d);
        assert!(pos_c < pos_d);

        // D has two upstreams: B and C
        let idx_d = dag.index_of("comp::d").unwrap();
        let upstream_d = dag.upstream_of(idx_d);
        assert_eq!(upstream_d.len(), 2);

        // A's downstream reaches B, C, D
        let idx_a = dag.index_of("comp::a").unwrap();
        let down_a = dag.downstream_of(idx_a);
        assert_eq!(down_a.len(), 3);
    }

    #[test]
    fn test_index_of() {
        let node_a = make_node("comp", "a", vec![], vec![], vec![]);
        let node_b = make_node("comp", "b", vec![], vec![], vec![]);

        let dag = GeneratorDag::build(vec![node_a, node_b]).unwrap();
        assert!(dag.index_of("comp::a").is_some());
        assert!(dag.index_of("comp::b").is_some());
        assert!(dag.index_of("comp::nonexistent").is_none());
    }

    #[test]
    fn test_unknown_output_indices() {
        let mut node_a = make_node("comp", "a", vec![], vec![], vec![]);
        node_a.decl.outputs_unknown = true;
        let node_b = make_node("comp", "b", vec![], vec![], vec![]);
        let mut node_c = make_node("comp", "c", vec![], vec![], vec![]);
        node_c.decl.outputs_unknown = true;

        let dag = GeneratorDag::build(vec![node_a, node_b, node_c]).unwrap();
        let unknown = dag.unknown_output_indices();
        assert_eq!(unknown.len(), 2);
        // Should include indices for nodes a and c (0 and 2)
        assert!(unknown.contains(&0));
        assert!(unknown.contains(&2));
    }

    #[test]
    fn test_node_accessor() {
        let node_a = make_node("comp", "alpha", vec![], vec![], vec![]);
        let dag = GeneratorDag::build(vec![node_a]).unwrap();
        let n = dag.node(0);
        assert_eq!(n.decl.name, "alpha");
    }
}
