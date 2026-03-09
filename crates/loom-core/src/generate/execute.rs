use std::path::{Path, PathBuf};

use crate::assemble::fileset::{AssembledFile, AssembledFilesets, FileLanguage};
use crate::build::context::BuildContext;
use crate::error::LoomError;
use crate::generate::cache::{CacheEntry, CacheService};
use crate::generate::dag::GeneratorDag;
use crate::generate::node::GeneratorNode;
use crate::generate::registry::PluginRegistry;
use crate::manifest::GeneratorDecl;
use crate::plugin::backend::{Diagnostic, DiagnosticSeverity};
use crate::resolve::platform::{substitute_platform_params, ResolvedPlatform};
use crate::resolve::resolver::ResolvedProject;

/// Collect all generator declarations from resolved components and the project.
/// Returns (base_dir, generator_decl, source_name) tuples.
///
/// If a platform is resolved, `${platform.*}` references in command strings
/// and config values are substituted.
pub fn collect_generators(resolved: &ResolvedProject) -> Vec<(PathBuf, GeneratorDecl, String)> {
    let mut generators = Vec::new();
    for comp in &resolved.resolved_components {
        for gen in &comp.manifest.generators {
            let mut decl = gen.clone();
            if let Some(ref platform) = resolved.platform {
                substitute_generator_decl(&mut decl, platform);
            }
            generators.push((
                comp.source_path.clone(),
                decl,
                comp.manifest.component.name.clone(),
            ));
        }
    }
    for gen in &resolved.project.generators {
        let mut decl = gen.clone();
        if let Some(ref platform) = resolved.platform {
            substitute_generator_decl(&mut decl, platform);
        }
        generators.push((
            resolved.project_root.clone(),
            decl,
            resolved.project.project.name.clone(),
        ));
    }
    generators
}

/// Substitute `${platform.*}` references in a generator declaration's fields.
fn substitute_generator_decl(decl: &mut GeneratorDecl, platform: &ResolvedPlatform) {
    if let Some(ref cmd) = decl.command {
        if let Ok(substituted) = substitute_platform_params(cmd, platform) {
            decl.command = Some(substituted);
        }
    }
    if let Some(ref cmd) = decl.command_windows {
        if let Ok(substituted) = substitute_platform_params(cmd, platform) {
            decl.command_windows = Some(substituted);
        }
    }
    if let Some(ref config) = decl.config {
        decl.config = Some(substitute_toml_value(config, platform));
    }
}

/// Recursively substitute `${platform.*}` in all string values within a TOML value.
fn substitute_toml_value(value: &toml::Value, platform: &ResolvedPlatform) -> toml::Value {
    match value {
        toml::Value::String(s) => match substitute_platform_params(s, platform) {
            Ok(substituted) => toml::Value::String(substituted),
            Err(_) => value.clone(),
        },
        toml::Value::Table(t) => {
            let mut new_table = toml::map::Map::new();
            for (k, v) in t {
                new_table.insert(k.clone(), substitute_toml_value(v, platform));
            }
            toml::Value::Table(new_table)
        }
        toml::Value::Array(arr) => toml::Value::Array(
            arr.iter()
                .map(|v| substitute_toml_value(v, platform))
                .collect(),
        ),
        other => other.clone(),
    }
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

/// Run pre-flight validation on all generators before executing any of them.
///
/// Checks three categories in one pass:
/// 1. **Plugin instantiation** — can the plugin be looked up in the registry?
/// 2. **Config validation** — does `validate_config()` report any errors?
/// 3. **Environment check** — does `check_environment()` find required tools?
/// 4. **Input file existence** — do source-tree inputs exist on disk?
///    (Inputs that are outputs of upstream generators are skipped.)
///
/// All errors are collected and returned together so the user sees every
/// problem at once instead of fixing them one by one.
fn validate_generators_preflight(
    dag: &GeneratorDag,
    registry: &PluginRegistry,
) -> Result<Vec<Diagnostic>, LoomError> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // Collect all resolved output paths in the DAG so we can distinguish
    // source-tree inputs from generated inputs.
    let all_generated_outputs: std::collections::HashSet<&std::path::Path> = dag
        .execution_order()
        .flat_map(|n| n.resolved_outputs.iter().map(|p| p.as_path()))
        .collect();

    // Track which plugins we've already environment-checked (by plugin name)
    // to avoid redundant checks when multiple generators use the same plugin.
    let mut env_checked: std::collections::HashSet<String> = std::collections::HashSet::new();

    for node in dag.execution_order() {
        let gen_label = &node.id;

        // 1. Plugin instantiation
        let plugin = match registry.get(&node.decl) {
            Ok(p) => p,
            Err(e) => {
                diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!("Generator '{}': {}", gen_label, e),
                    source_path: None,
                    line: None,
                });
                continue; // Can't validate further without a plugin
            }
        };

        // 2. Config validation
        let exec_config = build_exec_config(node);
        match plugin.validate_config(&exec_config) {
            Ok(diags) => {
                for d in diags {
                    diagnostics.push(Diagnostic {
                        severity: d.severity,
                        message: format!("Generator '{}': {}", gen_label, d.message),
                        source_path: d.source_path,
                        line: d.line,
                    });
                }
            }
            Err(e) => {
                diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!("Generator '{}': config validation failed: {}", gen_label, e),
                    source_path: None,
                    line: None,
                });
            }
        }

        // 3. Environment check (once per plugin type, keyed by decl.plugin)
        if env_checked.insert(node.decl.plugin.clone()) {
            match plugin.check_environment() {
                Ok(diags) => {
                    for d in diags {
                        diagnostics.push(Diagnostic {
                            severity: d.severity,
                            message: format!("Generator '{}': {}", gen_label, d.message),
                            source_path: d.source_path,
                            line: d.line,
                        });
                    }
                }
                Err(e) => {
                    diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::Error,
                        message: format!(
                            "Generator '{}': environment check failed: {}",
                            gen_label, e
                        ),
                        source_path: None,
                        line: None,
                    });
                }
            }
        }

        // 4. Input file existence (skip inputs that are generated by upstream generators)
        for input in &node.resolved_inputs {
            if !all_generated_outputs.contains(input.as_path()) && !input.exists() {
                diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!(
                        "Generator '{}': input file not found: {}",
                        gen_label,
                        input.display()
                    ),
                    source_path: Some(input.clone()),
                    line: None,
                });
            }
        }
    }

    Ok(diagnostics)
}

/// Execute the generate phase: build DAG, run generators in order, return produced files.
///
/// Before executing any generator, a pre-flight validation pass checks all
/// configs, tool environments, and input files. If any errors are found,
/// execution is aborted and all problems are reported together.
///
/// If `on_event` is provided, it will be called for each generator start/finish
/// to allow real-time UI updates (spinners, progress lines, etc.).
pub fn run_generate_phase(
    resolved: &ResolvedProject,
    context: &BuildContext,
    registry: &PluginRegistry,
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

    // ── Pre-flight validation ──────────────────────────────────────
    let preflight = validate_generators_preflight(&dag, registry)?;
    let errors: Vec<&Diagnostic> = preflight
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect();
    if !errors.is_empty() {
        let mut msg = format!(
            "Generator pre-flight check failed ({} error{}):\n",
            errors.len(),
            if errors.len() == 1 { "" } else { "s" }
        );
        for e in &errors {
            msg.push_str(&format!("  - {}\n", e.message));
        }
        return Err(LoomError::Internal(msg));
    }

    let cache = CacheService::new(&context.build_dir);
    let mut warnings = Vec::new();

    // Refined outputs_unknown invalidation: only invalidate downstream generators,
    // not the entire cache
    if dag.has_unknown_outputs {
        warnings.push(
            "One or more generators have outputs_unknown=true. \
             Downstream generator caches will be invalidated."
                .to_string(),
        );
        // We handle selective invalidation below via forced_rerun set
    }

    // Collect indices that must be force-rerun (downstream of outputs_unknown generators)
    let mut forced_rerun: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for idx in dag.unknown_output_indices() {
        forced_rerun.insert(idx);
        forced_rerun.extend(dag.downstream_of(idx));
    }

    let mut executed = 0;
    let mut cached_count = 0;
    let mut produced_files: Vec<(PathBuf, String, String)> = Vec::new();
    let mut generator_statuses: Vec<GeneratorStatus> = Vec::new();
    // Track output hashes per generator ID for transitive cache invalidation
    let mut output_hashes: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    let exec_order: Vec<(usize, String)> = dag
        .execution_order()
        .map(|n| {
            let idx = dag.index_of(&n.id).unwrap();
            (idx, n.id.clone())
        })
        .collect();

    for (node_idx, _node_id) in &exec_order {
        let node = dag.node(*node_idx);
        let plugin = registry.get(&node.decl)?;

        // Compute cache key including upstream output hashes for transitive invalidation
        let input_hashes = if !node.resolved_inputs.is_empty() {
            cache
                .hash_input_files(&node.resolved_inputs)
                .unwrap_or_default()
        } else {
            vec![]
        };

        // Collect upstream hashes as extra context
        let upstream_indices = dag.upstream_of(*node_idx);
        let mut extra_context: Vec<(String, String)> = Vec::new();
        for &up_idx in &upstream_indices {
            let up_node = dag.node(up_idx);
            if let Some(hash) = output_hashes.get(&up_node.id) {
                extra_context.push((format!("upstream:{}", up_node.id), hash.clone()));
            }
        }
        let extra_refs: Vec<(&str, &str)> = extra_context
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let cache_key = cache.compute_cache_key(
            &node.decl.plugin,
            node.decl.config.as_ref(),
            &input_hashes,
            &extra_refs,
        );

        // Check cache (skip if forced rerun)
        let is_forced = forced_rerun.contains(node_idx);
        if node.decl.cacheable && !node.decl.outputs_unknown && !is_forced {
            if let Some(entry) = cache.get(&cache_key)? {
                let output_count = entry.produced_files.len();
                for file in &entry.produced_files {
                    produced_files.push((
                        file.clone(),
                        node.source.clone(),
                        node.decl.fileset.clone(),
                    ));
                }
                // Hash cached outputs for downstream transitive invalidation
                let hash = hash_output_files(&entry.produced_files);
                output_hashes.insert(node.id.clone(), hash);

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

        // Ensure output directory exists before running the generator
        std::fs::create_dir_all(&node.output_dir).map_err(|e| LoomError::Io {
            path: node.output_dir.clone(),
            source: e,
        })?;

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

        // Hash outputs for downstream transitive invalidation
        let hash = hash_output_files(&result.produced_files);
        output_hashes.insert(node.id.clone(), hash);

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

/// Hash a list of output files into a single deterministic hash string.
///
/// Files are sorted before hashing for order independence. Files that don't
/// exist on disk are silently skipped (this handles the case where outputs
/// haven't been written yet during initial DAG construction).
fn hash_output_files(files: &[PathBuf]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut sorted: Vec<_> = files.to_vec();
    sorted.sort();
    for path in &sorted {
        if let Ok(content) = std::fs::read(path) {
            hasher.update(path.to_string_lossy().as_bytes());
            hasher.update(b":");
            hasher.update(&content);
            hasher.update(b"\0");
        }
    }
    hex::encode(hasher.finalize())
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

    // Pass output directory so plugins know where to write
    table.insert(
        "output_dir".to_string(),
        toml::Value::String(node.output_dir.to_string_lossy().into_owned()),
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
///
/// Supports `"synth"` and `"sim"` filesets. Files targeting `"sim"` are added
/// to the sim_files list (for testbenches, test vectors, etc.). Unknown fileset
/// names default to `"synth"` with a warning.
pub fn merge_generated_files(
    filesets: &mut AssembledFilesets,
    produced: &[(PathBuf, String, String)],
) {
    for (path, source, fileset) in produced {
        let language =
            FileLanguage::from_extension(path.extension().and_then(|e| e.to_str()).unwrap_or(""));
        let file = AssembledFile {
            path: path.clone(),
            source_component: source.clone(),
            language,
        };
        match fileset.as_str() {
            "synth" => filesets.synth_files.push(file),
            "sim" => filesets.sim_files.push(file),
            other => {
                // Unknown fileset — default to synth
                eprintln!(
                    "Warning: unknown fileset '{}' for generated file {:?}, defaulting to synth",
                    other, path
                );
                filesets.synth_files.push(file);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assemble::fileset::{AssembledConstraint, AssembledFile, FileLanguage};
    use crate::manifest::platform::ClockDef;
    use std::collections::HashMap;

    // ── merge_generated_files ────────────────────────────────────────

    fn empty_filesets() -> AssembledFilesets {
        AssembledFilesets {
            synth_files: vec![],
            sim_files: vec![],
            constraint_files: vec![],
            defines: vec![],
        }
    }

    #[test]
    fn test_merge_synth_files() {
        let mut fs = empty_filesets();
        let produced = vec![(
            PathBuf::from("/build/gen/regs.sv"),
            "comp_a".to_string(),
            "synth".to_string(),
        )];
        merge_generated_files(&mut fs, &produced);
        assert_eq!(fs.synth_files.len(), 1);
        assert_eq!(fs.sim_files.len(), 0);
        assert_eq!(fs.synth_files[0].path, PathBuf::from("/build/gen/regs.sv"));
        assert_eq!(fs.synth_files[0].source_component, "comp_a");
        assert_eq!(fs.synth_files[0].language, FileLanguage::SystemVerilog);
    }

    #[test]
    fn test_merge_sim_files() {
        let mut fs = empty_filesets();
        let produced = vec![(
            PathBuf::from("/build/gen/test_vectors.svh"),
            "alu".to_string(),
            "sim".to_string(),
        )];
        merge_generated_files(&mut fs, &produced);
        assert_eq!(fs.synth_files.len(), 0);
        assert_eq!(fs.sim_files.len(), 1);
        assert_eq!(
            fs.sim_files[0].path,
            PathBuf::from("/build/gen/test_vectors.svh")
        );
        assert_eq!(fs.sim_files[0].source_component, "alu");
    }

    #[test]
    fn test_merge_mixed_filesets() {
        let mut fs = empty_filesets();
        let produced = vec![
            (
                PathBuf::from("/build/gen/regs.sv"),
                "comp".to_string(),
                "synth".to_string(),
            ),
            (
                PathBuf::from("/build/gen/tb_regs.sv"),
                "comp".to_string(),
                "sim".to_string(),
            ),
            (
                PathBuf::from("/build/gen/other.sv"),
                "comp".to_string(),
                "synth".to_string(),
            ),
        ];
        merge_generated_files(&mut fs, &produced);
        assert_eq!(fs.synth_files.len(), 2);
        assert_eq!(fs.sim_files.len(), 1);
    }

    #[test]
    fn test_merge_unknown_fileset_defaults_to_synth() {
        let mut fs = empty_filesets();
        let produced = vec![(
            PathBuf::from("/build/gen/data.mem"),
            "comp".to_string(),
            "custom_fileset".to_string(),
        )];
        merge_generated_files(&mut fs, &produced);
        // Unknown fileset falls back to synth
        assert_eq!(fs.synth_files.len(), 1);
        assert_eq!(fs.sim_files.len(), 0);
    }

    #[test]
    fn test_merge_empty_produced() {
        let mut fs = empty_filesets();
        merge_generated_files(&mut fs, &[]);
        assert_eq!(fs.synth_files.len(), 0);
        assert_eq!(fs.sim_files.len(), 0);
    }

    #[test]
    fn test_merge_preserves_existing_files() {
        let mut fs = empty_filesets();
        fs.synth_files.push(AssembledFile {
            path: PathBuf::from("/src/existing.sv"),
            source_component: "comp".to_string(),
            language: FileLanguage::SystemVerilog,
        });
        fs.sim_files.push(AssembledFile {
            path: PathBuf::from("/src/existing_tb.sv"),
            source_component: "comp".to_string(),
            language: FileLanguage::SystemVerilog,
        });

        let produced = vec![(
            PathBuf::from("/build/gen/new.sv"),
            "comp".to_string(),
            "synth".to_string(),
        )];
        merge_generated_files(&mut fs, &produced);
        assert_eq!(fs.synth_files.len(), 2);
        assert_eq!(fs.sim_files.len(), 1);
    }

    // ── build_exec_config ────────────────────────────────────────────

    #[test]
    fn test_build_exec_config_includes_output_dir() {
        let decl = GeneratorDecl {
            name: "gen".to_string(),
            plugin: "command".to_string(),
            command: Some("echo test".to_string()),
            command_windows: None,
            inputs: vec![],
            outputs: vec![PathBuf::from("out.sv")],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: None,
        };
        let node = GeneratorNode::from_decl(
            decl,
            "comp",
            &PathBuf::from("/ws/comp"),
            &PathBuf::from("/ws/.build"),
        );

        let config = build_exec_config(&node);
        let table = config.as_table().unwrap();

        // Must contain output_dir
        assert!(table.contains_key("output_dir"));
        let output_dir = table["output_dir"].as_str().unwrap();
        assert!(output_dir.contains(".build/generate/comp--gen"));

        // Must contain working_dir (base_dir)
        assert!(table.contains_key("working_dir"));
        assert_eq!(table["working_dir"].as_str().unwrap(), "/ws/comp");

        // Must contain command
        assert_eq!(table["command"].as_str().unwrap(), "echo test");

        // Outputs should be absolute paths in the build dir
        let outputs = table["outputs"].as_array().unwrap();
        assert_eq!(outputs.len(), 1);
        let out_path = outputs[0].as_str().unwrap();
        assert!(out_path.contains(".build/generate/comp--gen/out.sv"));
    }

    #[test]
    fn test_build_exec_config_merges_decl_config() {
        let mut config_table = toml::map::Map::new();
        config_table.insert(
            "custom_key".to_string(),
            toml::Value::String("custom_value".to_string()),
        );

        let decl = GeneratorDecl {
            name: "gen".to_string(),
            plugin: "command".to_string(),
            command: Some("echo test".to_string()),
            command_windows: None,
            inputs: vec![],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: Some(toml::Value::Table(config_table)),
        };
        let node = GeneratorNode::from_decl(
            decl,
            "comp",
            &PathBuf::from("/ws/comp"),
            &PathBuf::from("/ws/.build"),
        );

        let config = build_exec_config(&node);
        let table = config.as_table().unwrap();
        assert_eq!(table["custom_key"].as_str().unwrap(), "custom_value");
    }

    // ── hash_output_files ────────────────────────────────────────────

    #[test]
    fn test_hash_output_files_deterministic() {
        let tmp = tempfile::TempDir::new().unwrap();
        let f1 = tmp.path().join("a.sv");
        let f2 = tmp.path().join("b.sv");
        std::fs::write(&f1, "module a; endmodule").unwrap();
        std::fs::write(&f2, "module b; endmodule").unwrap();

        let h1 = hash_output_files(&[f1.clone(), f2.clone()]);
        let h2 = hash_output_files(&[f1.clone(), f2.clone()]);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_output_files_order_independent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let f1 = tmp.path().join("a.sv");
        let f2 = tmp.path().join("b.sv");
        std::fs::write(&f1, "module a; endmodule").unwrap();
        std::fs::write(&f2, "module b; endmodule").unwrap();

        let h_ab = hash_output_files(&[f1.clone(), f2.clone()]);
        let h_ba = hash_output_files(&[f2, f1]);
        assert_eq!(h_ab, h_ba, "Hash should be order-independent");
    }

    #[test]
    fn test_hash_output_files_changes_with_content() {
        let tmp = tempfile::TempDir::new().unwrap();
        let f = tmp.path().join("gen.sv");

        std::fs::write(&f, "module v1; endmodule").unwrap();
        let h1 = hash_output_files(&[f.clone()]);

        std::fs::write(&f, "module v2; endmodule").unwrap();
        let h2 = hash_output_files(&[f]);
        assert_ne!(h1, h2, "Hash should change when file content changes");
    }

    #[test]
    fn test_hash_output_files_empty() {
        let h1 = hash_output_files(&[]);
        let h2 = hash_output_files(&[]);
        assert_eq!(h1, h2);
        assert!(!h1.is_empty());
    }

    #[test]
    fn test_hash_output_files_missing_file_skipped() {
        let h = hash_output_files(&[PathBuf::from("/nonexistent/path.sv")]);
        // Should produce a valid hash, not panic
        assert!(!h.is_empty());
    }

    // ── substitute_generator_decl / substitute_toml_value ────────────

    fn make_test_platform() -> ResolvedPlatform {
        ResolvedPlatform {
            name: "zcu104".to_string(),
            part: Some("xczu7ev".to_string()),
            board: None,
            backend: Some("vivado".to_string()),
            backend_version: Some("2023.2".to_string()),
            virtual_platform: false,
            clocks: {
                let mut m = HashMap::new();
                m.insert(
                    "sys_clk".to_string(),
                    ClockDef {
                        frequency_mhz: 125.0,
                        period_ns: 8.0,
                        pin: Some("H9".to_string()),
                        standard: Some("LVDS".to_string()),
                        description: None,
                    },
                );
                m
            },
            params: {
                let mut m = HashMap::new();
                m.insert("data_width".to_string(), toml::Value::Integer(64));
                m
            },
            constraint_files: vec![],
            variant_tags: vec![],
            platform_root: PathBuf::from("/platforms/zcu104"),
        }
    }

    #[test]
    fn test_substitute_generator_command() {
        let platform = make_test_platform();
        let mut decl = GeneratorDecl {
            name: "gen".to_string(),
            plugin: "command".to_string(),
            command: Some("gen_regs --freq ${platform.clocks.sys_clk.frequency_mhz}".to_string()),
            command_windows: None,
            inputs: vec![],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: None,
        };
        substitute_generator_decl(&mut decl, &platform);
        assert_eq!(decl.command.unwrap(), "gen_regs --freq 125");
    }

    #[test]
    fn test_substitute_generator_command_windows() {
        let platform = make_test_platform();
        let mut decl = GeneratorDecl {
            name: "gen".to_string(),
            plugin: "command".to_string(),
            command: None,
            command_windows: Some("gen.exe --part ${platform.part}".to_string()),
            inputs: vec![],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: None,
        };
        substitute_generator_decl(&mut decl, &platform);
        assert_eq!(decl.command_windows.unwrap(), "gen.exe --part xczu7ev");
    }

    #[test]
    fn test_substitute_generator_config() {
        let platform = make_test_platform();
        let config_toml: toml::Value = toml::from_str(
            r#"
            width = "${platform.params.data_width}"
            name = "${platform.name}"
            nested.freq = "${platform.clocks.sys_clk.frequency_mhz}"
            "#,
        )
        .unwrap();

        let mut decl = GeneratorDecl {
            name: "gen".to_string(),
            plugin: "command".to_string(),
            command: None,
            command_windows: None,
            inputs: vec![],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: Some(config_toml),
        };
        substitute_generator_decl(&mut decl, &platform);

        let config = decl.config.unwrap();
        assert_eq!(config.get("width").unwrap().as_str().unwrap(), "64");
        assert_eq!(config.get("name").unwrap().as_str().unwrap(), "zcu104");
        let nested = config.get("nested").unwrap().as_table().unwrap();
        assert_eq!(nested.get("freq").unwrap().as_str().unwrap(), "125");
    }

    #[test]
    fn test_substitute_no_platform_params_is_noop() {
        let platform = make_test_platform();
        let mut decl = GeneratorDecl {
            name: "gen".to_string(),
            plugin: "command".to_string(),
            command: Some("echo hello".to_string()),
            command_windows: None,
            inputs: vec![],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: None,
        };
        substitute_generator_decl(&mut decl, &platform);
        assert_eq!(decl.command.unwrap(), "echo hello");
    }

    #[test]
    fn test_substitute_toml_array() {
        let platform = make_test_platform();
        let value: toml::Value = toml::from_str(
            r#"
            items = ["${platform.name}", "literal", "${platform.part}"]
            "#,
        )
        .unwrap();

        let result = substitute_toml_value(&value, &platform);
        let items = result.get("items").unwrap().as_array().unwrap();
        assert_eq!(items[0].as_str().unwrap(), "zcu104");
        assert_eq!(items[1].as_str().unwrap(), "literal");
        assert_eq!(items[2].as_str().unwrap(), "xczu7ev");
    }

    #[test]
    fn test_substitute_toml_preserves_non_strings() {
        let platform = make_test_platform();
        let value: toml::Value = toml::from_str(
            r#"
            count = 42
            enabled = true
            ratio = 3.14
            "#,
        )
        .unwrap();

        let result = substitute_toml_value(&value, &platform);
        assert_eq!(result.get("count").unwrap().as_integer().unwrap(), 42);
        assert!(result.get("enabled").unwrap().as_bool().unwrap());
        assert!((result.get("ratio").unwrap().as_float().unwrap() - 3.14).abs() < f64::EPSILON);
    }

    #[test]
    fn test_substitute_invalid_param_preserved() {
        // Invalid platform references should be preserved as-is (no panic)
        let platform = make_test_platform();
        let mut decl = GeneratorDecl {
            name: "gen".to_string(),
            plugin: "command".to_string(),
            command: Some("echo ${platform.nonexistent}".to_string()),
            command_windows: None,
            inputs: vec![],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: None,
        };
        // Should not panic — the substitution fails but the original is preserved
        substitute_generator_decl(&mut decl, &platform);
        // Command should be unchanged since substitution errored
        assert_eq!(decl.command.unwrap(), "echo ${platform.nonexistent}");
    }

    // ── validate_generators_preflight ────────────────────────────────

    fn make_test_node(
        source: &str,
        name: &str,
        inputs: Vec<&str>,
        outputs: Vec<&str>,
    ) -> GeneratorNode {
        let decl = GeneratorDecl {
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
        };
        GeneratorNode::from_decl(
            decl,
            source,
            &PathBuf::from("/ws/comp"),
            &PathBuf::from("/ws/.build"),
        )
    }

    #[test]
    fn test_preflight_passes_for_valid_command_generators() {
        let node = make_test_node("comp", "gen_a", vec![], vec!["out.sv"]);
        let dag = GeneratorDag::build(vec![node]).unwrap();
        let registry = PluginRegistry::with_builtins();

        let diags = validate_generators_preflight(&dag, &registry).unwrap();
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect();
        assert!(errors.is_empty(), "Expected no errors: {:?}", errors);
    }

    #[test]
    fn test_preflight_catches_unknown_plugin() {
        let mut node = make_test_node("comp", "gen_a", vec![], vec![]);
        node.decl.plugin = "nonexistent_plugin".to_string();

        let dag = GeneratorDag::build(vec![node]).unwrap();
        let registry = PluginRegistry::with_builtins();

        let diags = validate_generators_preflight(&dag, &registry).unwrap();
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("nonexistent_plugin"));
    }

    #[test]
    fn test_preflight_catches_missing_input_file() {
        // Input is an absolute path that doesn't exist and isn't generated
        let node = make_test_node(
            "comp",
            "gen_a",
            vec!["/nonexistent/path/input.yaml"],
            vec!["out.sv"],
        );
        let dag = GeneratorDag::build(vec![node]).unwrap();
        let registry = PluginRegistry::with_builtins();

        let diags = validate_generators_preflight(&dag, &registry).unwrap();
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect();
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].message.contains("input file not found"),
            "Got: {}",
            errors[0].message
        );
    }

    #[test]
    fn test_preflight_skips_generated_inputs() {
        // Node A outputs foo.sv; Node B takes A's output as input.
        // Pre-flight should NOT report B's input as missing because A generates it.
        let node_a = make_test_node("comp", "a", vec![], vec!["foo.sv"]);
        let a_output = node_a.resolved_outputs[0].to_string_lossy().to_string();

        let node_b_decl = GeneratorDecl {
            name: "b".to_string(),
            plugin: "command".to_string(),
            command: Some("echo test".to_string()),
            command_windows: None,
            inputs: vec![PathBuf::from(&a_output)],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec!["a".to_string()],
            cacheable: true,
            outputs_unknown: false,
            config: None,
        };
        let node_b = GeneratorNode::from_decl(
            node_b_decl,
            "comp",
            &PathBuf::from("/ws/comp"),
            &PathBuf::from("/ws/.build"),
        );

        let dag = GeneratorDag::build(vec![node_a, node_b]).unwrap();
        let registry = PluginRegistry::with_builtins();

        let diags = validate_generators_preflight(&dag, &registry).unwrap();
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "Generated input should not be flagged as missing: {:?}",
            errors
        );
    }

    #[test]
    fn test_preflight_reports_multiple_errors() {
        // Two generators with problems: one unknown plugin, one missing input
        let mut node_a = make_test_node("comp", "a", vec![], vec![]);
        node_a.decl.plugin = "fake_plugin".to_string();

        let node_b = make_test_node("comp", "b", vec!["/nonexistent/input.yaml"], vec!["out.sv"]);

        let dag = GeneratorDag::build(vec![node_a, node_b]).unwrap();
        let registry = PluginRegistry::with_builtins();

        let diags = validate_generators_preflight(&dag, &registry).unwrap();
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect();
        // Should have at least 2 errors (unknown plugin + missing input)
        assert!(
            errors.len() >= 2,
            "Expected at least 2 errors, got {}: {:?}",
            errors.len(),
            errors
        );
    }

    #[test]
    fn test_preflight_with_existing_input_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let input_file = tmp.path().join("input.yaml");
        std::fs::write(&input_file, "data: true").unwrap();

        let decl = GeneratorDecl {
            name: "gen".to_string(),
            plugin: "command".to_string(),
            command: Some("echo test".to_string()),
            command_windows: None,
            inputs: vec![input_file.clone()],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: None,
        };
        let node = GeneratorNode::from_decl(decl, "comp", tmp.path(), &PathBuf::from("/ws/.build"));

        let dag = GeneratorDag::build(vec![node]).unwrap();
        let registry = PluginRegistry::with_builtins();

        let diags = validate_generators_preflight(&dag, &registry).unwrap();
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "Existing input should pass: {:?}",
            errors
        );
    }
}
