use std::path::PathBuf;

use crate::manifest::GeneratorDecl;

/// A generator node in the execution DAG.
/// Enriched from the manifest declaration with resolved paths.
#[derive(Debug, Clone)]
pub struct GeneratorNode {
    /// Unique ID within the build: "<component_name>::<generator_name>"
    pub id: String,
    /// Source component/project name (for error attribution).
    pub source: String,
    /// The original manifest declaration.
    pub decl: GeneratorDecl,
    /// Absolute paths to input files.
    pub resolved_inputs: Vec<PathBuf>,
    /// Absolute paths to expected output files.
    pub resolved_outputs: Vec<PathBuf>,
    /// Build directory for this generator's outputs.
    pub output_dir: PathBuf,
    /// Base directory of the component/project owning this generator.
    pub base_dir: PathBuf,
}

impl GeneratorNode {
    /// Build a GeneratorNode from a declaration, resolving paths relative to `base_dir`.
    pub fn from_decl(
        decl: GeneratorDecl,
        source: &str,
        base_dir: &std::path::Path,
        build_dir: &std::path::Path,
    ) -> Self {
        let id = format!("{}::{}", source, decl.name);
        let resolved_inputs = decl
            .inputs
            .iter()
            .map(|p| {
                if p.is_absolute() {
                    p.clone()
                } else {
                    base_dir.join(p)
                }
            })
            .collect();
        let resolved_outputs = decl
            .outputs
            .iter()
            .map(|p| {
                if p.is_absolute() {
                    p.clone()
                } else {
                    base_dir.join(p)
                }
            })
            .collect();
        let output_dir = build_dir.join("generate").join(&decl.name);

        Self {
            id,
            source: source.to_string(),
            decl,
            resolved_inputs,
            resolved_outputs,
            output_dir,
            base_dir: base_dir.to_path_buf(),
        }
    }

    /// The input paths relevant for cache key computation.
    pub fn cache_key_inputs(&self) -> &[PathBuf] {
        &self.resolved_inputs
    }

    /// Check if any declared output overlaps with another node's inputs.
    pub fn outputs_overlap_with_inputs(&self, other: &GeneratorNode) -> bool {
        self.resolved_outputs
            .iter()
            .any(|out| other.resolved_inputs.iter().any(|inp| out == inp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::GeneratorDecl;

    fn make_decl(name: &str, inputs: Vec<&str>, outputs: Vec<&str>) -> GeneratorDecl {
        GeneratorDecl {
            name: name.to_string(),
            plugin: "command".to_string(),
            command: Some("echo test".to_string()),
            command_windows: None,
            inputs: inputs.into_iter().map(PathBuf::from).collect(),
            outputs: outputs.into_iter().map(PathBuf::from).collect(),
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: None,
        }
    }

    #[test]
    fn test_from_decl_resolves_paths() {
        let decl = make_decl("gen", vec!["input.yaml"], vec!["out/gen.sv"]);
        let base = PathBuf::from("/workspace/comp_a");
        let build = PathBuf::from("/workspace/.build");

        let node = GeneratorNode::from_decl(decl, "comp_a", &base, &build);
        assert_eq!(node.id, "comp_a::gen");
        assert_eq!(
            node.resolved_inputs[0],
            PathBuf::from("/workspace/comp_a/input.yaml")
        );
        assert_eq!(
            node.resolved_outputs[0],
            PathBuf::from("/workspace/comp_a/out/gen.sv")
        );
    }

    #[test]
    fn test_outputs_overlap_detection() {
        let decl_a = make_decl("a", vec![], vec!["generated/foo.sv"]);
        let decl_b = make_decl("b", vec!["generated/foo.sv"], vec![]);
        let base = PathBuf::from("/ws/comp");
        let build = PathBuf::from("/ws/.build");

        let node_a = GeneratorNode::from_decl(decl_a, "comp", &base, &build);
        let node_b = GeneratorNode::from_decl(decl_b, "comp", &base, &build);

        assert!(node_a.outputs_overlap_with_inputs(&node_b));
        assert!(!node_b.outputs_overlap_with_inputs(&node_a));
    }

    #[test]
    fn test_no_overlap_different_paths() {
        let decl_a = make_decl("a", vec![], vec!["out_a.sv"]);
        let decl_b = make_decl("b", vec!["input_b.yaml"], vec![]);
        let base = PathBuf::from("/ws/comp");
        let build = PathBuf::from("/ws/.build");

        let node_a = GeneratorNode::from_decl(decl_a, "comp", &base, &build);
        let node_b = GeneratorNode::from_decl(decl_b, "comp", &base, &build);

        assert!(!node_a.outputs_overlap_with_inputs(&node_b));
    }
}
