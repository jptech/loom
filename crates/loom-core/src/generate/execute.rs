use std::path::{Path, PathBuf};

use crate::assemble::fileset::{AssembledFile, AssembledFilesets, FileLanguage};
use crate::build::context::BuildContext;
use crate::error::LoomError;
use crate::generate::cache::{CacheEntry, CacheService};
use crate::generate::dag::GeneratorDag;
use crate::generate::node::GeneratorNode;
use crate::manifest::GeneratorDecl;
use crate::plugin::generator::GeneratorPlugin;
use crate::resolve::resolver::ResolvedProject;

/// Collect all generator declarations from resolved components and the project.
/// Returns (base_dir, generator_decl, source_name) tuples.
pub fn collect_generators(resolved: &ResolvedProject) -> Vec<(PathBuf, GeneratorDecl, String)> {
    let mut generators = Vec::new();
    for comp in &resolved.resolved_components {
        for gen in &comp.manifest.generators {
            generators.push((
                comp.source_path.clone(),
                gen.clone(),
                comp.manifest.component.name.clone(),
            ));
        }
    }
    for gen in &resolved.project.generators {
        generators.push((
            resolved.project_root.clone(),
            gen.clone(),
            resolved.project.project.name.clone(),
        ));
    }
    generators
}

/// Build generator nodes from collected declarations.
pub fn build_generator_nodes(
    generators: &[(PathBuf, GeneratorDecl, String)],
    build_dir: &Path,
) -> Vec<GeneratorNode> {
    generators
        .iter()
        .map(|(base, decl, source)| GeneratorNode::from_decl(decl.clone(), source, base, build_dir))
        .collect()
}

/// Result of running the generate phase.
pub struct GeneratePhaseResult {
    /// Number of generators executed.
    pub executed: usize,
    /// Number of generators skipped (cached).
    pub cached: usize,
    /// Files produced by generators, to add to filesets.
    pub produced_files: Vec<(PathBuf, String, String)>, // (path, source_component, fileset)
    /// Warnings emitted.
    pub warnings: Vec<String>,
}

/// Execute the generate phase: build DAG, run generators in order, return produced files.
pub fn run_generate_phase(
    resolved: &ResolvedProject,
    context: &BuildContext,
    get_plugin: &dyn Fn(&str) -> Option<Box<dyn GeneratorPlugin>>,
    quiet: bool,
) -> Result<GeneratePhaseResult, LoomError> {
    let collected = collect_generators(resolved);
    if collected.is_empty() {
        return Ok(GeneratePhaseResult {
            executed: 0,
            cached: 0,
            produced_files: vec![],
            warnings: vec![],
        });
    }

    let nodes = build_generator_nodes(&collected, &context.build_dir);
    let dag = GeneratorDag::build(nodes)?;

    let cache = CacheService::new(&context.build_dir);
    let mut warnings = Vec::new();

    if dag.has_unknown_outputs {
        warnings.push(
            "One or more generators have outputs_unknown=true. \
             Incremental builds are disabled for this project."
                .to_string(),
        );
        cache.invalidate_all()?;
    }

    let mut executed = 0;
    let mut cached = 0;
    let mut produced_files: Vec<(PathBuf, String, String)> = Vec::new();

    for node in dag.execution_order() {
        let plugin = get_plugin(&node.decl.plugin).ok_or_else(|| {
            LoomError::Internal(format!(
                "No generator plugin found for '{}'",
                node.decl.plugin
            ))
        })?;

        // Compute cache key
        let input_hashes = if !node.resolved_inputs.is_empty() {
            cache
                .hash_input_files(&node.resolved_inputs)
                .unwrap_or_default()
        } else {
            vec![]
        };

        let cache_key = cache.compute_cache_key(
            &node.decl.plugin,
            node.decl.config.as_ref(),
            &input_hashes,
            &[],
        );

        // Check cache
        if node.decl.cacheable && !node.decl.outputs_unknown {
            if let Some(entry) = cache.get(&cache_key)? {
                if !quiet {
                    eprintln!("    {} (cached)", node.id);
                }
                for file in &entry.produced_files {
                    produced_files.push((
                        file.clone(),
                        node.source.clone(),
                        node.decl.fileset.clone(),
                    ));
                }
                cached += 1;
                continue;
            }
        }

        if !quiet {
            eprintln!("    Running generator: {}", node.id);
        }

        // Build a config Value that includes the command and outputs for the plugin
        let exec_config = build_exec_config(node);
        let result = plugin.execute(&exec_config, context)?;

        // Verify declared outputs exist
        for expected in &node.resolved_outputs {
            if !expected.exists() {
                return Err(LoomError::Internal(format!(
                    "Generator '{}' did not produce expected output: {}",
                    node.id,
                    expected.display()
                )));
            }
        }

        // Store cache entry
        if node.decl.cacheable && !node.decl.outputs_unknown {
            let entry = CacheEntry {
                cache_key: cache_key.clone(),
                generator_id: node.id.clone(),
                created_at: chrono::Utc::now().to_rfc3339(),
                produced_files: result.produced_files.clone(),
            };
            cache.put(&entry)?;
        }

        for file in &result.produced_files {
            produced_files.push((file.clone(), node.source.clone(), node.decl.fileset.clone()));
        }

        executed += 1;
    }

    Ok(GeneratePhaseResult {
        executed,
        cached,
        produced_files,
        warnings,
    })
}

/// Build a toml::Value config for the generator plugin to consume.
fn build_exec_config(node: &GeneratorNode) -> toml::Value {
    let mut table = toml::map::Map::new();

    if let Some(cmd) = node.decl.effective_command() {
        table.insert("command".to_string(), toml::Value::String(cmd.to_string()));
    }

    if !node.decl.outputs.is_empty() {
        let outputs: Vec<toml::Value> = node
            .resolved_outputs
            .iter()
            .map(|p| toml::Value::String(p.to_string_lossy().into_owned()))
            .collect();
        table.insert("outputs".to_string(), toml::Value::Array(outputs));
    }

    if let Some(toml::Value::Table(t)) = &node.decl.config {
        for (k, v) in t {
            table.insert(k.clone(), v.clone());
        }
    }

    toml::Value::Table(table)
}

/// Merge generated files into an existing AssembledFilesets.
pub fn merge_generated_files(
    filesets: &mut AssembledFilesets,
    produced: &[(PathBuf, String, String)],
) {
    for (path, source, fileset) in produced {
        if fileset == "synth" {
            let language = FileLanguage::from_extension(
                path.extension().and_then(|e| e.to_str()).unwrap_or(""),
            );
            filesets.synth_files.push(AssembledFile {
                path: path.clone(),
                source_component: source.clone(),
                language,
            });
        }
    }
}
