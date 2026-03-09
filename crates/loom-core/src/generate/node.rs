use std::path::PathBuf;

use crate::manifest::GeneratorDecl;

/// A generator node in the execution DAG.
///
/// Enriched from the manifest declaration with resolved paths. Input paths
/// resolve relative to the component's `base_dir` (source tree), while output
/// paths resolve relative to `output_dir` (build directory) so generated files
/// never pollute the source tree.
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
    /// Build a GeneratorNode from a declaration, resolving paths.
    ///
    /// - **Inputs** resolve relative to `base_dir` (component source directory).
    /// - **Outputs** resolve relative to `.build/generate/<sanitized_id>/`.
    /// - The sanitized ID replaces `/` with `__` and `::` with `--` to produce
    ///   a filesystem-safe directory name.
    pub fn from_decl(
        decl: GeneratorDecl,
        source: &str,
        base_dir: &std::path::Path,
        build_dir: &std::path::Path,
    ) -> Self {
        let id = format!("{}::{}", source, decl.name);
        // Sanitize ID for use as a directory name
        let sanitized_id = id.replace('/', "__").replace("::", "--");
        let output_dir = build_dir.join("generate").join(&sanitized_id);

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
        // Outputs resolve relative to output_dir (build directory), not source tree
        let resolved_outputs = decl
            .outputs
            .iter()
            .map(|p| {
                if p.is_absolute() {
                    p.clone()
                } else {
                    output_dir.join(p)
                }
            })
            .collect();

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
        // Inputs resolve relative to base_dir (source tree)
        assert_eq!(
            node.resolved_inputs[0],
            PathBuf::from("/workspace/comp_a/input.yaml")
        );
        // Outputs resolve relative to output_dir (build directory)
        assert_eq!(
            node.resolved_outputs[0],
            PathBuf::from("/workspace/.build/generate/comp_a--gen/out/gen.sv")
        );
        assert_eq!(
            node.output_dir,
            PathBuf::from("/workspace/.build/generate/comp_a--gen")
        );
    }

    #[test]
    fn test_outputs_overlap_detection() {
        // Node A outputs to build dir; node B's input must use absolute path
        // matching A's output location for overlap detection to work
        let build = PathBuf::from("/ws/.build");
        let base = PathBuf::from("/ws/comp");

        let decl_a = make_decl("a", vec![], vec!["foo.sv"]);
        let node_a = GeneratorNode::from_decl(decl_a, "comp", &base, &build);
        // A's output resolves to /ws/.build/generate/comp--a/foo.sv
        let a_output = node_a.resolved_outputs[0].to_string_lossy().to_string();

        // B takes A's resolved output as an absolute input
        let decl_b = make_decl("b", vec![&a_output], vec![]);
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

    #[test]
    fn test_namespaced_source_sanitization() {
        // org/comp_name becomes org__comp_name in the sanitized directory
        let decl = make_decl("gen", vec![], vec!["out.sv"]);
        let base = PathBuf::from("/ws/comp");
        let build = PathBuf::from("/ws/.build");

        let node = GeneratorNode::from_decl(decl, "soc/regmap_core", &base, &build);
        assert_eq!(node.id, "soc/regmap_core::gen");
        assert_eq!(
            node.output_dir,
            PathBuf::from("/ws/.build/generate/soc__regmap_core--gen")
        );
        assert_eq!(
            node.resolved_outputs[0],
            PathBuf::from("/ws/.build/generate/soc__regmap_core--gen/out.sv")
        );
    }

    #[test]
    fn test_absolute_paths_pass_through() {
        let decl = make_decl(
            "gen",
            vec!["/absolute/input.yaml"],
            vec!["/absolute/output.sv"],
        );
        let base = PathBuf::from("/ws/comp");
        let build = PathBuf::from("/ws/.build");

        let node = GeneratorNode::from_decl(decl, "comp", &base, &build);
        assert_eq!(
            node.resolved_inputs[0],
            PathBuf::from("/absolute/input.yaml")
        );
        assert_eq!(
            node.resolved_outputs[0],
            PathBuf::from("/absolute/output.sv")
        );
    }

    #[test]
    fn test_multiple_outputs_resolve_to_build_dir() {
        let decl = make_decl(
            "regmap",
            vec!["spec.json"],
            vec!["regs.sv", "regs_pkg.sv", "sub/nested.sv"],
        );
        let base = PathBuf::from("/ws/comp");
        let build = PathBuf::from("/ws/.build");

        let node = GeneratorNode::from_decl(decl, "comp", &base, &build);
        let out_dir = PathBuf::from("/ws/.build/generate/comp--regmap");
        assert_eq!(node.resolved_outputs[0], out_dir.join("regs.sv"));
        assert_eq!(node.resolved_outputs[1], out_dir.join("regs_pkg.sv"));
        assert_eq!(node.resolved_outputs[2], out_dir.join("sub/nested.sv"));
    }

    #[test]
    fn test_empty_inputs_and_outputs() {
        let decl = make_decl("gen", vec![], vec![]);
        let base = PathBuf::from("/ws/comp");
        let build = PathBuf::from("/ws/.build");

        let node = GeneratorNode::from_decl(decl, "comp", &base, &build);
        assert!(node.resolved_inputs.is_empty());
        assert!(node.resolved_outputs.is_empty());
        assert!(node.cache_key_inputs().is_empty());
    }
}
