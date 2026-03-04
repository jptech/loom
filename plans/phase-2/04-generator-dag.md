# Phase 2 / Task 04: Generator DAG

**Prerequisites:** Phase 2 Tasks 01, 02, 03
**Goal:** Build a topologically ordered DAG of generator nodes, with automatic dependency detection via input/output overlap and explicit `depends_on` ordering.

## Spec Reference
`system_plan.md` §6.2 (Execution Model), §6.3 (Generator-to-Generator Dependencies)

## File to Implement
`crates/loom-core/src/generate/dag.rs`

## Key Logic

1. Collect all generators from all resolved components and the project
2. For each generator, resolve input/output paths to absolute paths
3. For each pair (A, B): if any output of A is in B's inputs → A must run before B
4. Add explicit `depends_on` edges
5. Topological sort → execution order
6. Detect `outputs_unknown` generators and emit a warning

## Implementation Sketch

```rust
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::toposort;
use std::path::PathBuf;
use crate::generate::node::GeneratorNode;
use crate::error::LoomError;

pub struct GeneratorDag {
    nodes: Vec<GeneratorNode>,
    /// Execution order (indices into `nodes`)
    execution_order: Vec<usize>,
    /// Whether any generator has outputs_unknown = true
    pub has_unknown_outputs: bool,
}

impl GeneratorDag {
    /// Build the DAG from all generator nodes.
    pub fn build(nodes: Vec<GeneratorNode>) -> Result<Self, LoomError> {
        let n = nodes.len();
        let mut graph: DiGraph<usize, ()> = DiGraph::new();

        // Add all nodes to graph
        let node_indices: Vec<NodeIndex> = (0..n).map(|i| graph.add_node(i)).collect();

        // Build a name → index map for depends_on lookups
        let name_to_idx: std::collections::HashMap<&str, usize> = nodes.iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect();

        // Add edges: output/input overlap detection
        for i in 0..n {
            for j in 0..n {
                if i == j { continue; }
                if nodes[i].outputs_overlap_with_inputs(&nodes[j]) {
                    // i must run before j
                    graph.add_edge(node_indices[i], node_indices[j], ());
                }
            }

            // Add explicit depends_on edges
            for dep_name in &nodes[i].decl.depends_on {
                if let Some(&dep_idx) = name_to_idx.get(dep_name.as_str()) {
                    graph.add_edge(node_indices[dep_idx], node_indices[i], ());
                } else {
                    return Err(LoomError::Internal(format!(
                        "Generator '{}' depends_on '{}' which doesn't exist",
                        nodes[i].id, dep_name
                    )));
                }
            }
        }

        // Topological sort
        let sorted = toposort(&graph, None)
            .map_err(|cycle| {
                let idx = cycle.node_id();
                let node_data = graph[idx];
                LoomError::Internal(format!(
                    "Circular generator dependency detected involving generator index {}",
                    node_data
                ))
            })?;

        let execution_order: Vec<usize> = sorted.iter().map(|&ni| graph[ni]).collect();

        let has_unknown_outputs = nodes.iter().any(|n| n.decl.outputs_unknown);

        Ok(Self { nodes, execution_order, has_unknown_outputs })
    }

    /// Iterate generators in execution order.
    pub fn execution_order(&self) -> impl Iterator<Item = &GeneratorNode> {
        self.execution_order.iter().map(|&i| &self.nodes[i])
    }

    /// Check if two generators can run in parallel (no ordering dependency).
    pub fn can_run_parallel(&self, a_id: &str, b_id: &str) -> bool {
        // Two generators are parallel if neither depends on the other
        // (For Phase 2: just check execution_order positions)
        let pos_a = self.execution_order.iter().position(|&i| self.nodes[i].id == a_id);
        let pos_b = self.execution_order.iter().position(|&i| self.nodes[i].id == b_id);
        // If both exist and are adjacent in topo order without direct dependency, they're parallel.
        // Simplified: assume consecutive means potentially parallel (actual parallelism in Task 05)
        pos_a.is_some() && pos_b.is_some()
    }
}
```

## Tests

```rust
#[test]
fn test_dag_output_input_detection() {
    // Create generator A with output "generated/foo.sv"
    // Create generator B with input "generated/foo.sv"
    // Verify A comes before B in execution order
}

#[test]
fn test_dag_explicit_depends_on() {
    // Create generators X and Y where Y depends_on = ["X"]
    // Verify X before Y in execution order
}

#[test]
fn test_dag_cycle_detection() {
    // Create A → B → A cycle via depends_on
    // Verify error
}

#[test]
fn test_dag_independent_nodes() {
    // Create A and B with no relationship
    // Both should appear in execution order (order doesn't matter)
}
```

## Done When

- Input/output overlap creates correct ordering
- `depends_on` creates correct ordering
- Cycles are detected and reported
- `has_unknown_outputs` is set correctly
