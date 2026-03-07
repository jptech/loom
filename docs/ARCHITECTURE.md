# Loom Architecture

This document describes the internal architecture of Loom. For user-facing usage documentation, see the [README](../README.md).

## Three-layer design

Loom is organized into three conceptual layers:

```
┌─────────────────────────────────────────────────────┐
│  Layer 2: Convenience                               │
│  IP catalog, strategy sweeps, metrics, registry     │
├─────────────────────────────────────────────────────┤
│  Layer 1: Tool Plugins                              │
│  Vivado, Quartus, yosys, Radiant, simulators        │
├─────────────────────────────────────────────────────┤
│  Layer 0: Vendor-Agnostic Core                      │
│  Manifests, resolution, assembly, build DAG, CLI    │
└─────────────────────────────────────────────────────┘
```

**Layer 0** knows nothing about specific FPGA vendors. It parses TOML manifests, resolves dependencies, assembles ordered filesets, and orchestrates the build pipeline. All vendor-specific behavior is behind trait interfaces.

**Layer 1** implements backend and simulator traits for specific toolchains. Each backend lives in its own crate and can be compiled independently.

**Layer 2** provides higher-level features built on top of the core and plugins: IP management, build report comparison, package registry, and CI integration.

## Crate structure

```
crates/
├── loom-cli/           Binary. CLI entry point (clap). Dispatches to command modules.
├── loom-core/          Library. All framework logic (~80% of the codebase).
├── loom-vivado/        Library. Vivado synthesis backend. (preliminary)
├── loom-yosys/         Library. yosys + nextpnr open-source backend. (preliminary, ice40)
├── loom-quartus/       Library. Quartus Prime synthesis backend. (planned)
├── loom-radiant/       Library. Lattice Radiant backend. (planned)
├── loom-xsim/          Library. Xilinx Vivado Simulator. (preliminary)
├── loom-verilator/     Library. Verilator cycle-accurate simulator. (preliminary)
├── loom-icarus/        Library. Icarus Verilog simulator. (preliminary)
├── loom-questa/        Library. Siemens Questa/ModelSim simulator. (planned)
├── loom-vcs/           Library. Synopsys VCS simulator. (planned)
└── loom-xcelium/       Library. Cadence Xcelium simulator. (planned)
```

All crates share workspace-level dependencies defined in the root `Cargo.toml`. Backend and simulator crates depend only on `loom-core` (for trait definitions and shared types). The `loom-cli` crate depends on all of them.

## loom-core modules

```
loom-core/src/
├── lib.rs                  Module root and re-exports
├── error.rs                LoomError enum with exit code mapping
│
├── manifest/               TOML manifest parsing and validation
│   ├── component.rs        ComponentManifest, FileSet, DependencySpec, ComponentVariant
│   ├── project.rs          ProjectManifest, TargetSpec, BuildConfig, profiles
│   ├── workspace.rs        WorkspaceManifest, WorkspaceSettings, ResolutionConfig
│   ├── platform.rs         PlatformManifest, ClockDef, PlatformConstraints
│   ├── generator.rs        GeneratorDecl
│   ├── test.rs             TestDecl, TestSuiteDecl, TestSuiteReport
│   └── common.rs           Shared parsing helpers
│
├── resolve/                Dependency resolution
│   ├── workspace.rs        Workspace discovery, member enumeration
│   ├── resolver.rs         Dependency graph construction, cycle detection
│   ├── lockfile.rs         loom.lock generation and staleness checking
│   ├── graph.rs            petgraph-backed dependency graph
│   ├── platform.rs         Platform resolution and parameter substitution
│   └── registry.rs         Remote package registry source
│
├── assemble/               File-set assembly
│   ├── fileset.rs          File collection, constraint ordering, language detection
│   ├── ordering.rs         Topological file ordering (dependencies first)
│   └── template.rs         Constraint template preprocessing
│
├── generate/               Code generation framework
│   ├── dag.rs              Generator DAG with topological execution order
│   ├── node.rs             Generator node (inputs, outputs, plugin reference)
│   ├── cache.rs            SHA-256 based cache keys for incremental generation
│   ├── execute.rs          Generator execution with caching
│   └── plugins/
│       ├── command.rs       Shell command generator (runs arbitrary commands)
│       └── python.rs        Python plugin loader (PyO3)
│
├── build/                  Build pipeline
│   ├── pipeline.rs         Phase orchestration (RESOLVE → BUILD)
│   ├── context.rs          BuildContext (shared state passed to backends)
│   ├── validate.rs         Pre-build validation (files exist, tool available)
│   ├── checkpoint.rs       Build state persistence for --resume
│   ├── hooks.rs            Lifecycle hooks (pre_build, post_build, etc.)
│   └── report.rs           Build metrics and report serialization
│
├── sim/                    Simulation test runner
│   ├── compat.rs           Simulator compatibility checking
│   ├── discovery.rs        Test discovery from component manifests
│   └── runner.rs           Sequential/parallel test execution (thread::scope)
│
└── plugin/                 Plugin trait definitions
    ├── backend.rs           BackendPlugin, BackendCapabilities, BuildResult
    ├── simulator.rs         SimulatorPlugin, SimulatorCapabilities
    ├── generator.rs         GeneratorPlugin trait
    └── reporter.rs          ReporterPlugin (console, JSON, GitHub Actions, JUnit)
```

## Build pipeline

The core pipeline is a linear sequence of phases:

```
┌─────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌───────┐    ┌────────┐
│ RESOLVE  │───>│ GENERATE │───>│ ASSEMBLE │───>│ VALIDATE │───>│ BUILD │───>│ REPORT │
└─────────┘    └──────────┘    └──────────┘    └──────────┘    └───────┘    └────────┘
```

### Resolve

1. Walk up from the current directory to find `workspace.toml`.
2. Enumerate workspace members by expanding glob patterns in `members = [...]`.
3. Classify each member as a component, project, or platform.
4. Load and parse all component manifests.
5. Starting from the project's declared dependencies, resolve the full dependency graph using topological sort (via petgraph).
6. Detect cycles and report them as errors.
7. Check the lockfile for staleness; regenerate if dependencies changed.

Key types: `WorkspaceDependencySource`, `ResolvedProject`, `ResolvedComponent`.

### Generate

For each `[[generators]]` entry in the resolved manifests:

1. Build a DAG of generators (some depend on others' outputs).
2. Compute a cache key from: plugin name, inputs (file hashes), config, and command.
3. If the cache key matches a previous run, skip execution.
4. Otherwise, execute the generator (shell command, Python plugin, or built-in).
5. Verify that declared outputs were actually produced.

Key types: `GeneratorDecl`, `GeneratorNode`, `GeneratorDag`.

### Assemble

Collect all HDL files and constraints from resolved components and the project itself:

1. Walk components in topological order (leaf dependencies first).
2. For each component, collect files from the `synth` fileset.
3. Apply variant overlays if a variant was selected for that component.
4. Detect file language (SystemVerilog, Verilog, VHDL) from extension.
5. Collect constraint files and tag them with their scope (`component` or `global`).
6. Sort constraints: component-scoped first, then global.

Key types: `AssembledFilesets`, `AssembledFile`, `AssembledConstraint`, `FileLanguage`.

### Validate

Pre-flight checks before invoking the toolchain:

1. Verify all referenced source files exist on disk.
2. Check that the backend tool is installed and the version matches (if specified).
3. Run backend-specific validation (the `BackendPlugin::validate()` method).
4. Report all errors as a batch (not fail-fast).

### Build

1. Call `BackendPlugin::generate_build_scripts()` to produce tool-specific scripts (Vivado Tcl, Quartus Tcl, yosys commands).
2. Call `BackendPlugin::execute_build()` to run the tool in batch mode.
3. Parse tool output for phase completion, errors, and warnings.
4. Return a `BuildResult` with exit code, log paths, and completed phases.

### Report

Extract build metrics and present them:

1. Parse timing reports, utilization summaries, and build logs.
2. Serialize to the build report format (JSON).
3. Display via the selected reporter (console, JSON, GitHub Actions annotations, JUnit XML).

## Plugin traits

### BackendPlugin

The primary synthesis/implementation interface:

```rust
pub trait BackendPlugin {
    fn plugin_name(&self) -> &str;
    fn capabilities(&self) -> BackendCapabilities;
    fn check_environment(&self, required_version: Option<&str>)
        -> Result<EnvironmentStatus, LoomError>;
    fn validate(&self, project: &ResolvedProject, filesets: &AssembledFilesets,
        context: &BuildContext) -> Result<Vec<Diagnostic>, LoomError>;
    fn generate_build_scripts(&self, project: &ResolvedProject,
        filesets: &AssembledFilesets, context: &BuildContext)
        -> Result<Vec<PathBuf>, LoomError>;
    fn execute_build(&self, scripts: &[PathBuf], context: &BuildContext)
        -> Result<BuildResult, LoomError>;
}
```

`BackendCapabilities` declares what a backend supports:

```rust
pub struct BackendCapabilities {
    pub supports_ooc: bool,             // Out-of-context synthesis
    pub supports_incremental: bool,     // Incremental compilation
    pub supports_ip_generation: bool,   // Vendor IP generation
    pub supports_block_design: bool,    // Block design / schematic
    pub supports_strategy_sweep: bool,  // Multiple optimization strategies
    pub checkpoint_format: Option<String>,  // "dcp", "json", etc.
    pub constraint_formats: Vec<String>,    // "xdc", "sdc", "pcf", etc.
    pub sub_phases: Vec<String>,            // "synthesis", "place", "route", ...
}
```

### SimulatorPlugin

The simulation interface, with a compile → elaborate → simulate lifecycle:

```rust
pub trait SimulatorPlugin {
    fn plugin_name(&self) -> &str;
    fn capabilities(&self) -> SimulatorCapabilities;
    fn check_environment(&self, required_version: Option<&str>)
        -> Result<EnvironmentStatus, LoomError>;
    fn compile(&self, filesets: &AssembledFilesets, options: &SimOptions,
        context: &BuildContext) -> Result<CompileResult, LoomError>;
    fn elaborate(&self, compile_result: &CompileResult, top_module: &str,
        options: &SimOptions, context: &BuildContext)
        -> Result<ElaborateResult, LoomError>;
    fn simulate(&self, elaborate_result: &ElaborateResult, options: &SimOptions,
        context: &BuildContext) -> Result<SimResult, LoomError>;
    fn extract_results(&self, sim_result: &SimResult)
        -> Result<SimReport, LoomError>;
    fn merge_coverage(&self, coverage_dbs: &[PathBuf], output: &Path)
        -> Result<CoverageReport, LoomError>;
}
```

`SimulatorCapabilities` describes language support, coverage capabilities, and performance characteristics.

### ReporterPlugin

Formats build results for different output targets:

```rust
pub trait ReporterPlugin {
    fn name(&self) -> &str;
    fn format_report(&self, report: &BuildReportData) -> String;
}
```

Built-in reporters: `ConsoleReporter`, `JsonReporter`, `GitHubActionsReporter`, `JUnitReporter`.

## Dependency resolution

Resolution uses a recursive depth-first strategy with cycle detection:

1. Start from the project's `[dependencies]` table.
2. For each dependency name, look it up in the workspace's loaded components.
3. Short names (e.g., `uart`) are resolved by matching the suffix of `org/name`. If ambiguous, an error is raised.
4. Semver constraints are checked: `">=1.0.0"` is parsed via the `semver` crate.
5. A `visited` set prevents infinite recursion on cycles. If a cycle is detected, `LoomError::DependencyCycle` is returned.
6. The final graph is topologically sorted using petgraph's `toposort`, then reversed so leaf dependencies come first (needed for correct file ordering).

The lockfile (`loom.lock`) records the exact resolved versions and is checked for staleness on each build.

## Error handling

All fallible operations return `Result<T, LoomError>`. The `LoomError` enum uses `thiserror` for derive macros and maps each variant to an exit code:

| Exit code | Error category | Examples |
|-----------|----------------|----------|
| 0 | Success | — |
| 1 | Build failure | Synthesis failed, simulation failed, validation errors |
| 2 | Configuration | Bad manifest, missing dependency, version conflict, cycle |
| 3 | Environment | Tool not found, version mismatch |
| 4 | Internal | I/O errors, lockfile write failures |

Error messages are designed to be actionable: they describe what happened, provide context, and suggest what to do next.

## Workspace discovery

When a command runs, Loom walks up from the current directory looking for `workspace.toml`. Once found:

1. The `members` globs are expanded (e.g., `lib/*` matches all directories under `lib/`).
2. Each member directory is classified:
   - Contains `component.toml` → Component
   - Contains `project.toml` → Project
   - Contains `platform.toml` → Platform (Phase 3+)
3. All component manifests are loaded into memory.
4. The target project is identified (auto-detected if only one exists, or selected by name).

## Generator caching

Generators use SHA-256 cache keys to avoid redundant execution:

```
cache_key = SHA-256(
    plugin_name,
    command_string,
    config_json,
    sorted_input_file_hashes,
    extra_context
)
```

The cache is stored under `.build/.gen_cache/`. When a generator runs, its cache key is written alongside the outputs. On subsequent builds, if the key matches, the generator is skipped.

## Backend-specific details

### Vivado (loom-vivado)

- Non-project-mode batch execution via `vivado -mode batch -source build.tcl`.
- Generated Tcl handles: `read_verilog -sv` vs. `read_vhdl`, library mapping, absolute forward-slash paths, `-ref` constraint scoping for component-scoped XDC files.
- Supports OOC synthesis, incremental compilation, IP generation, and block designs.
- Checkpoint format: DCP.

### Quartus (loom-quartus)

- Batch execution via `quartus_sh -t build.tcl`.
- QSF-based constraint flow.
- Supports incremental compilation and IP generation.
- Standard installation detection: `intelFPGA_lite/`, `intelFPGA_pro/`.

### yosys + nextpnr (loom-yosys)

- Three-step pipeline: `yosys` (synthesis) → `nextpnr-{arch}` (place & route) → `{arch}pack` (bitstream).
- Architecture auto-detection from part number: `lp8k` → ice40, `LFE5U` → ECP5, `GW1NR` → Gowin.
- Each architecture uses its own nextpnr binary, packer, and constraint format.

### Radiant (loom-radiant)

- Batch execution via `radiantc script build.tcl`.
- Device family detection: iCE40 UltraPlus, CrossLink-NX, CertusPro-NX.
- PDC constraint format.

## Simulation flow

The `loom sim` command uses the `SimulatorPlugin` trait:

```
┌─────────┐    ┌───────────┐    ┌──────────┐    ┌─────────┐
│ COMPILE  │───>│ ELABORATE │───>│ SIMULATE │───>│ EXTRACT │
└─────────┘    └───────────┘    └──────────┘    └─────────┘
```

Some simulators combine phases (Verilator does compile + elaborate in one step; Icarus Verilog similarly). The trait interface accommodates this by having `elaborate()` return a pass-through result when not needed.

Coverage merging is supported for multi-test regression runs: each simulator implements `merge_coverage()` using its native tool (e.g., `vcover merge` for Questa, `urg` for VCS).

### Parallel test execution

When `-j N` is specified (N > 1), tests run concurrently using `std::thread::scope()` with a counting semaphore (`Mutex<usize>` + `Condvar`) to cap concurrency. Each test gets its own build directory (`.build/<project>/default/tests/<test_name>/sim/`) to avoid file conflicts. The `SimulatorPlugin` trait is `Send + Sync` with `&self` methods, so a single simulator instance is safely shared across threads.

In sequential mode (default), per-phase progress is printed in real time. In parallel mode, compact completion events are printed as tests finish, synchronized via a mutex.

## Lifecycle hooks

The hook system runs user-defined shell commands at lifecycle points:

```
pre_generate → post_generate → pre_build → post_build → post_report
```

Hooks are defined in workspace or project configuration and receive build context as JSON on stdin. Exit codes control behavior: 0 = continue, 1 = fail the build, 2 = warn and continue.

## Adding a new backend

To add a new synthesis backend:

1. Create a new crate: `crates/loom-mybackend/`.
2. Add it to workspace members in the root `Cargo.toml`.
3. Implement `BackendPlugin` from `loom_core::plugin::backend`.
4. Add the backend to `loom-cli/src/backend_registry.rs`.
5. Add it as a dependency in `loom-cli/Cargo.toml`.

The minimum implementation requires:
- `plugin_name()` → identifier string
- `capabilities()` → declare what the backend supports
- `check_environment()` → find the tool, check version
- `generate_build_scripts()` → produce tool-specific scripts
- `execute_build()` → run the tool and parse results

## Adding a new simulator

Same pattern as backends, but implement `SimulatorPlugin` from `loom_core::plugin::simulator`:

1. Create `crates/loom-mysim/`.
2. Implement `SimulatorPlugin` (compile, elaborate, simulate, extract_results).
3. Register in `loom-cli/src/commands/sim.rs` in the `get_simulator()` function.

## Key dependencies

| Crate | Purpose |
|-------|---------|
| `clap` | CLI argument parsing (derive macros) |
| `serde` + `toml` | TOML manifest deserialization |
| `petgraph` | Dependency graph and topological sort |
| `semver` | Semantic version parsing and constraint matching |
| `sha2` + `hex` | Cache key computation |
| `thiserror` | Error type derive macros |
| `colored` | Terminal color output |
| `indicatif` | Progress bars and spinners |
| `regex` | Tool version string parsing |
| `walkdir` | Recursive directory traversal |
| `quick-xml` | XCI/XML file parsing for migration tools |
| `chrono` | Timestamps in build reports |
