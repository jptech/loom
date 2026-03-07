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

/// Status of a single generator after execution.
#[derive(Debug, Clone)]
pub struct GeneratorStatus {
    /// Generator display name.
    pub name: String,
    /// Source component/project.
    pub source: String,
    /// Whether this generator was a cache hit.
    pub cached: bool,
    /// Execution time in seconds (None if cached).
    pub elapsed_secs: Option<f64>,
    /// Number of files produced.
    pub output_count: usize,
}

/// Result of running the generate phase.
pub struct GeneratePhaseResult {
    /// Number of generators executed.
    pub executed: usize,
    /// Number of generators skipped (cached).
    pub cached: usize,
    /// Files produced by generators, to add to filesets.
    pub produced_files: Vec<(PathBuf, String, String)>, // (path, source_component, fileset)
    /// Per-generator status for display.
    pub generators: Vec<GeneratorStatus>,
    /// Warnings emitted.
    pub warnings: Vec<String>,
}

/// Events emitted during the generate phase for real-time UI updates.
pub enum GenerateEvent {
    /// A generator is about to start executing.
    Started { name: String },
    /// A generator finished (ran or cached).
    Finished { status: GeneratorStatus },
}

/// Execute the generate phase: build DAG, run generators in order, return produced files.
///
/// If `on_event` is provided, it will be called for each generator start/finish
/// to allow real-time UI updates (spinners, progress lines, etc.).
pub fn run_generate_phase(
    resolved: &ResolvedProject,
    context: &BuildContext,
    get_plugin: &dyn Fn(&str) -> Option<Box<dyn GeneratorPlugin>>,
    on_event: Option<&dyn Fn(GenerateEvent)>,
) -> Result<GeneratePhaseResult, LoomError> {
    let collected = collect_generators(resolved);
    if collected.is_empty() {
        return Ok(GeneratePhaseResult {
            executed: 0,
            cached: 0,
            produced_files: vec![],
            generators: vec![],
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
    let mut cached_count = 0;
    let mut produced_files: Vec<(PathBuf, String, String)> = Vec::new();
    let mut generator_statuses: Vec<GeneratorStatus> = Vec::new();

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
                let output_count = entry.produced_files.len();
                for file in &entry.produced_files {
                    produced_files.push((
                        file.clone(),
                        node.source.clone(),
                        node.decl.fileset.clone(),
                    ));
                }
                let status = GeneratorStatus {
                    name: node.decl.name.clone(),
                    source: node.source.clone(),
                    cached: true,
                    elapsed_secs: None,
                    output_count,
                };
                if let Some(cb) = on_event {
                    cb(GenerateEvent::Finished {
                        status: status.clone(),
                    });
                }
                generator_statuses.push(status);
                cached_count += 1;
                continue;
            }
        }

        if let Some(cb) = on_event {
            cb(GenerateEvent::Started {
                name: node.decl.name.clone(),
            });
        }

        // Build a config Value that includes the command and outputs for the plugin
        let exec_config = build_exec_config(node);
        let start = std::time::Instant::now();
        let result = plugin.execute(&exec_config, context)?;
        let elapsed = start.elapsed().as_secs_f64();

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

        let output_count = result.produced_files.len();
        for file in &result.produced_files {
            produced_files.push((file.clone(), node.source.clone(), node.decl.fileset.clone()));
        }

        let status = GeneratorStatus {
            name: node.decl.name.clone(),
            source: node.source.clone(),
            cached: false,
            elapsed_secs: Some(elapsed),
            output_count,
        };
        if let Some(cb) = on_event {
            cb(GenerateEvent::Finished {
                status: status.clone(),
            });
        }
        generator_statuses.push(status);

        executed += 1;
    }

    Ok(GeneratePhaseResult {
        executed,
        cached: cached_count,
        produced_files,
        generators: generator_statuses,
        warnings,
    })
}

/// Build a toml::Value config for the generator plugin to consume.
fn build_exec_config(node: &GeneratorNode) -> toml::Value {
    let mut table = toml::map::Map::new();

    if let Some(cmd) = node.decl.effective_command() {
        table.insert("command".to_string(), toml::Value::String(cmd.to_string()));
    }

    // Pass component base directory so the command runs from the right place
    table.insert(
        "working_dir".to_string(),
        toml::Value::String(node.base_dir.to_string_lossy().into_owned()),
    );

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
