# Phase 5: Simulation, yosys Backend, Advanced Build

**Prerequisites:** Phase 4 complete
**Goal:** Verification is first-class. Timing closure is assisted. OSS toolchain support broadens adoption.

## Spec Reference
`system_plan.md` §10.3.3 (SimulatorPlugin), §7.3.1 (Incremental Build), §8.2 (Strategy Sweeps), §10.6 (yosys backend)

---

## Tasks

### Task 01: SimulatorPlugin Interface

**Spec §10.3.3**

Define `SimulatorPlugin` trait in `crates/loom-core/src/plugin/simulator.rs`:

```rust
pub trait SimulatorPlugin: Send + Sync {
    fn plugin_name(&self) -> &str;
    fn capabilities(&self) -> SimulatorCapabilities;
    fn check_environment(&self, required_version: Option<&str>) -> Result<EnvironmentStatus, LoomError>;
    fn compile(&self, filesets: &ResolvedFilesets, options: &SimOptions, context: &BuildContext) -> Result<CompileResult, LoomError>;
    fn elaborate(&self, compile_result: &CompileResult, top_module: &str, options: &SimOptions, context: &BuildContext) -> Result<ElaborateResult, LoomError>;
    fn simulate(&self, elaborate_result: &ElaborateResult, options: &SimOptions, context: &BuildContext) -> Result<SimResult, LoomError>;
    fn extract_results(&self, sim_result: &SimResult) -> Result<SimReport, LoomError>;
    fn merge_coverage(&self, coverage_dbs: &[PathBuf], output: &Path) -> Result<CoverageReport, LoomError>;
}
```

`SimulatorCapabilities` (full spec §10.3.3):
```rust
pub struct SimulatorCapabilities {
    pub systemverilog_full: bool,
    pub vhdl: bool,
    pub mixed_language: bool,
    pub uvm: bool,
    pub fork_join: bool,
    pub force_release: bool,
    pub bind_statements: bool,
    pub code_coverage: bool,
    pub functional_coverage: bool,
    pub assertion_coverage: bool,
    pub compilation_model: String,  // "event_driven" | "cycle_accurate" | "formal"
    pub supports_gui: bool,
    pub supports_save_restore: bool,
    pub typical_compile_speed: String,
    pub typical_sim_speed: String,
}
```

**Test-simulator compatibility:** When `loom sim` runs a test, check `test.requires` against `simulator.capabilities()`. Incompatible → skip with clear message.

### Task 02: Vivado Simulator (xsim) Backend

Initial simulator implementation targeting Vivado's built-in simulator.

```
crates/loom-xsim/
├── src/
│   ├── lib.rs        (XsimBackend)
│   ├── compile.rs    (xvlog/xvhdl invocation)
│   ├── elaborate.rs  (xelab invocation)
│   ├── simulate.rs   (xsim invocation)
│   └── env_check.rs
```

**Capabilities:** Full SystemVerilog, no UVM library by default, event-driven.

### Task 03: Verilator Backend

```
crates/loom-verilator/
```

**Key difference:** Verilator transpiles to C++, then compiles C++. The `compile()` step does both. `elaborate()` is a no-op.

**Capabilities:** Limited SV (no fork/join, no force/release), no UVM, no event-driven (cycle-accurate).

Use for fast smoke tests (`tags = ["smoke"]` with no `requires`).

### Task 04: loom sim Command

```
loom sim                        Run default testbench
loom sim --top <testbench>      Specific testbench
loom sim --tool <simulator>     Choose simulator
loom sim --suite <name>         Run a test suite
loom sim --filter "axi_*"       Pattern filter
loom sim --regression           All tests, summary report
loom sim --check-compat         Check test/simulator compatibility
```

**Test discovery:** Read `[[tests]]` blocks from all component manifests and project manifest.
**Test suites:** Defined by `[test_suites]` in project/workspace manifest.
**Test-level dependency resolution:** Each test case gets its own mini-resolution (component + sim deps + test-only deps).

### Task 05: Strategy Sweeps

**Spec §8.2**

`loom build --sweep` runs all declared `[build.strategies.*]` in parallel.

```rust
// Parallel strategy execution using rayon or tokio
let results: Vec<Result<BuildResult, _>> = strategies
    .par_iter()
    .map(|(name, config)| run_strategy(name, config, &resolved, &filesets))
    .collect();

// Select best passing result via backend.select_strategy_result()
```

Each strategy writes to its own subdirectory: `.build/<project>/aggressive/`, `.build/<project>/default/`.

### Task 06: Incremental Build with Reference Checkpoints

**Spec §7.3.1**

The Vivado backend uses reference `.dcp` checkpoints for incremental synthesis.

**Reference checkpoint selection priority:**
1. `--reference <path>` CLI flag
2. Most recent successful checkpoint for same project + strategy + part
3. Most recent failed checkpoint (if failure was after synthesis)
4. None → full build

**In `generate_build_scripts()` (Vivado):**
```tcl
# If reference checkpoint available:
synth_design -top radar_top -part xczu7ev-ffvc1156-2-e \
    -incremental_rebuild {.build/radar_processor/default/post_synth.dcp}
```

**Configuration:** `incremental = false` in strategy config disables incremental.

### Task 07: yosys + nextpnr Backend

**Spec §10.6**

Two-tool pipeline: yosys synthesizes, nextpnr places and routes.

```
crates/loom-yosys/
├── src/
│   ├── lib.rs        (YosysNextpnrBackend)
│   ├── synth.rs      (yosys invocation, architecture-specific synth commands)
│   ├── pnr.rs        (nextpnr invocation per architecture)
│   ├── pack.rs       (icepack/ecppack bitstream packing)
│   └── env_check.rs  (find yosys, nextpnr variants)
```

**Supported architectures:** ice40, ECP5, Gowin (nextpnr variant per arch).

**yosys script for iCE40:**
```
read_verilog -sv /path/to/top.sv
synth_ice40 -top top -json top.json
```

**nextpnr invocation:**
```
nextpnr-ice40 --lp8k --package sg48 \
    --json top.json --pcf constraints/pins.pcf \
    --asc top.asc
```

**icepack:**
```
icepack top.asc top.bit
```

**Capabilities:**
```rust
BackendCapabilities {
    supports_ooc: false,
    supports_incremental: false,
    supports_ip_generation: false,
    supports_block_design: false,
    checkpoint_format: "json",
    constraint_formats: vec!["pcf", "lpf"],
    sub_phases: vec!["synthesis", "place", "route", "bitstream"],
}
```

**CI value:** yosys can syntax-check/elaborate designs targeting any vendor. Run `yosys -p "read_verilog -sv ...;elaborate"` as a fast pre-build check in CI, even for Vivado projects.

### Task 08: vivado_bd Generator Plugin

**Spec §6 generator plugin table**

The `vivado_bd` generator manages Vivado block design (`.bd`) files by regenerating them from a canonical Tcl export. This avoids checking in binary `.bd` files and eliminates Vivado GUI state pollution.

**Workflow:**
1. User exports block design to Tcl via Vivado GUI: `write_bd_tcl -force design.tcl`
2. The canonical Tcl script is committed to version control
3. `vivado_bd` generator re-creates the `.bd` from Tcl at build time

```toml
[[generators]]
name = "system_bd"
plugin = "vivado_bd"
inputs = ["bd/system.tcl"]
outputs = ["bd/system/system.bd"]
[generators.config]
bd_tcl = "bd/system.tcl"
```

**Implementation:** Python plugin in `loom-vivado-backend` package. Invokes Vivado in batch mode:
```tcl
source {bd/system.tcl}
regenerate_bd_layout
validate_bd_design
save_bd_design
```

**Cache key:** hash of the canonical Tcl script + Vivado version + target part.
