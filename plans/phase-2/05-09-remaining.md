# Phase 2 / Tasks 05-09: Generate Phase, Constraint Templating, Checkpoints, Dry Run, Build Report

These tasks implement the remaining Phase 2 build pipeline features.

---

## Task 05: Generate Phase Execution

**File:** `crates/loom-core/src/build/pipeline.rs`

The generate phase:
1. Builds the `GeneratorDag` from all resolved generators
2. For each node in execution order:
   a. Hash all input files
   b. Compute cache key
   c. If cache hit AND cacheable → skip (log "cached")
   d. If `outputs_unknown = true` → warn, always run
   e. Execute the plugin
   f. Verify declared outputs exist
   g. Store cache entry
3. Add generated files to the appropriate filesets before ASSEMBLE

**Warning for outputs_unknown:**
```
Warning: Generator "legacy_ip_setup" has outputs_unknown=true.
Incremental builds are disabled for this project.
```

**Generator resolution from manifests:**
```rust
// Collect all generators: components first (topological order), then project
pub fn collect_generators(resolved: &ResolvedProject) -> Vec<GeneratorDecl> {
    let mut generators = Vec::new();
    for comp in &resolved.resolved_components {
        for gen in &comp.manifest.generators {
            generators.push((comp.source_path.clone(), gen.clone()));
        }
    }
    for gen in &resolved.project.generators {
        generators.push((resolved.project_root.clone(), gen.clone()));
    }
    generators
}
```

**Key: files added to filesets.** After generation, the generator's `fileset` field determines which fileset the produced files join. The `AssembledFilesets` must be updated before Phase 5.

---

## Task 06: Constraint Templating

**File:** `crates/loom-core/src/assemble/template.rs`

Spec §3.3.1. Files ending in `.xdc.tpl`, `.sdc.tpl`, `.lpf.tpl`, `.pcf.tpl` are preprocessed.

**Template syntax:** `{{parameter.path}}` — dot-notation into a context dictionary.

**Context (Phase 2, without platforms):**
```rust
pub struct TemplateContext {
    pub project: HashMap<String, toml::Value>,  // project parameters
    pub component: HashMap<String, toml::Value>, // component metadata
    // Phase 3+: platform parameters
}
```

**Processing:**
```rust
pub fn preprocess_constraint_template(
    template_path: &Path,
    context: &TemplateContext,
    output_path: &Path,  // write processed file here
) -> Result<(), LoomError> {
    let content = std::fs::read_to_string(template_path)?;
    let processed = replace_template_vars(&content, context)?;
    std::fs::write(output_path, processed)?;
    Ok(())
}

fn replace_template_vars(input: &str, ctx: &TemplateContext) -> Result<String, LoomError> {
    // Find all {{...}} patterns, look up in context, replace
    // Error if a referenced variable doesn't exist in context
    let re = regex::Regex::new(r"\{\{([^}]+)\}\}").unwrap();
    // ... regex replacement
}
```

**In ASSEMBLE phase:** For each constraint with a `.tpl` extension:
1. Check cache (template + context hash → cached processed file)
2. If not cached: preprocess to `.build/<project>/templates/<name.xdc>`
3. Use processed path instead of template path

---

## Task 07: Build Checkpoints and Resume

**File:** `crates/loom-vivado/src/checkpoint.rs`

**Build state file** at `.build/<project>/<strategy>/build_state.json`.

### Types

```rust
#[derive(Serialize, Deserialize, Debug)]
pub struct BuildState {
    pub cache_key: String,
    pub backend: String,
    pub phases_completed: Vec<String>,
    pub phases_failed: Vec<String>,
    pub checkpoints: HashMap<String, PathBuf>,
    pub failure: Option<FailureInfo>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FailureInfo {
    pub phase: String,
    pub exit_code: i32,
    pub log: PathBuf,
    pub summary: Option<String>,
}
```

### Backend Plugin Changes

Add to `BackendPlugin` trait:
```rust
fn resume_build(
    &self,
    checkpoint: &Path,
    from_phase: &str,
    options: &BuildOptions,
    context: &BuildContext,
) -> Result<BuildResult, LoomError>;
```

### Vivado Tcl for Resume

The Vivado backend generates a different Tcl script when resuming:
```tcl
# Resume from post_place.dcp
open_checkpoint {.build/project/default/post_place.dcp}
route_design
write_bitstream -force {.build/project/default/radar_top.bit}
```

### CLI Flags

```rust
// In BuildArgs:
#[arg(long)]
pub resume: bool,
#[arg(long, value_name = "PHASE")]
pub stop_after: Option<String>,
#[arg(long, value_name = "PHASE")]
pub start_at: Option<String>,
```

**Phase mapping (generic → Vivado):**
| Generic | Vivado Tcl |
|---|---|
| synthesis | synth_design |
| optimize | opt_design |
| place | place_design |
| route | route_design |
| phys_optimize | phys_opt_design |
| bitstream | write_bitstream |

---

## Task 08: Dry Run Mode

**CLI Flag:** `--dry-run`

When `--dry-run` is set, execute Phases 1-4 normally, then instead of Phase 5:
1. Print the execution plan
2. Show which generators would run (vs. cached)
3. Show the generated Tcl script path
4. Exit 0

**Output format:**
```
  ✓ Phase 1: RESOLVE (3 components, 2 generators)
  ✓ Phase 2: GENERATE
      ✓ regmap (cached)
      ↻ sys_pll (would run — config changed)
  ✓ Phase 3: ASSEMBLE (12 source files, 3 constraint files)
  ✓ Phase 4: VALIDATE (0 errors, 1 warning)

  Phase 5: BUILD (would execute — dry run)
    Target:   xczu7ev-ffvc1156-2-e
    Strategy: default
    Backend:  vivado 2023.2
    Steps:    synthesis → optimize → place → route → bitstream

    Generated script: .build/my_design/default/build.tcl

  Dry run complete. Use "loom build" to execute.
```

---

## Task 09: JSON Build Report

**File:** `crates/loom-core/src/build/report.rs`

Phase 6 (EXTRACT) queries the Vivado backend for metrics. Phase 7 (REPORT) formats and outputs them.

**BuildReport struct:**
```rust
#[derive(Serialize, Debug)]
pub struct BuildReport {
    pub project: String,
    pub timestamp: String,
    pub tool: ToolInfo,
    pub target: TargetInfo,
    pub strategy: String,
    pub status: BuildStatus,
    pub git: Option<GitInfo>,
    pub metrics: BuildMetrics,
}

#[derive(Serialize, Debug, Default)]
pub struct BuildMetrics {
    pub timing: Option<TimingMetrics>,
    pub utilization: Option<UtilizationMetrics>,
    pub power: Option<PowerMetrics>,
    pub build: Option<BuildTimeMetrics>,
}

#[derive(Serialize, Debug)]
pub struct TimingSummary {
    pub wns: f64,
    pub tns: f64,
    pub whs: f64,
    pub ths: f64,
    pub failing_endpoints: u32,
}
```

**Git info extraction:**
```rust
fn get_git_info(workspace_root: &Path) -> Option<GitInfo> {
    // Run `git rev-parse HEAD` and `git status --porcelain`
    // Parse commit hash and dirty status
}
```

**Vivado metrics extraction (Phase 2 basic):**
Add `extract_metrics()` to `BackendPlugin` trait. Vivado implementation runs a post-build Tcl script:
```tcl
open_checkpoint {post_route.dcp}
set timing [report_timing_summary -return_string]
set util [report_utilization -return_string]
# Parse and output JSON
```

**Phase 2 scope:** Extract timing WNS/TNS/WHS, utilization LUT/FF/BRAM percentages, build duration. More detailed metrics in Phase 4.

**Output:** `--json` flag makes `loom build` output the full `BuildReport` as JSON to stdout.
`loom report` reads the last saved report from `.build/<project>/default/report.json`.
