# Phase 4: Reporting, CI, Hooks, Quartus Backend

**Prerequisites:** Phase 3 complete
**Goal:** Build metrics are tracked over time. CI integration is seamless. Hook contract is fully specified. Quartus backend validates vendor-agnosticism.

## Spec Reference
`system_plan.md` §9 (Metrics and Reporting), §10.5 (Hook System), §10.6 (Quartus backend), §13.3 (loom env shell)

---

## Tasks

### Task 01: Reporter Plugin Interface + Built-in Reporters

**Spec §9.5, §10.3.4**

Define `ReporterPlugin` trait:
```rust
pub trait ReporterPlugin: Send + Sync {
    fn plugin_name(&self) -> &str;
    fn format_report(&self, report: &BuildReport, options: &toml::Value) -> Result<ReporterOutput, LoomError>;
}

pub struct ReporterOutput {
    pub content: Vec<u8>,
    pub suggested_filename: Option<String>,
    pub content_type: String,
}
```

**Built-in reporters:**

`ConsoleReporter` — human-readable terminal tables:
```
Build: my_design (xczu7ev-ffvc1156-2-e) — PASSED (2305s)

  Timing:
    WNS:  0.142ns  ✓
    WHS:  0.021ns  ✓

  Utilization:
    LUT:   42.3%  ██████████░░░░░░░░░░
    FF:    28.1%  ██████░░░░░░░░░░░░░░
    BRAM:  65.0%  █████████████░░░░░░░
    DSP:   12.5%  ██░░░░░░░░░░░░░░░░░░
```

`JsonReporter` — raw JSON report.

`GitHubActionsReporter` — GitHub Actions annotations:
```
::notice title=Build Passed::my_design: WNS=0.142ns, LUT=42.3%, FF=28.1%
```

`JUnitReporter` — JUnit XML for CI systems.

### Task 02: Hierarchical Metrics Diff

**Command:** `loom report --diff <git-ref>`

1. Load current build report (`report.json`)
2. Checkout `git-ref` and load its report (or retrieve from git objects)
3. Diff at each metric path
4. Output diff table:
```
  Metric                   Current    Previous    Change
  timing.summary.wns        0.142      0.089       +0.053  ↓ (degraded)
  utilization.summary.lut  42.3%      39.8%       +2.5%   ↑ (increased)
  build.total_duration      2305s      2180s       +125s
```

**Storage:** Save `report.json` in `.build/<project>/<strategy>/` after each build. For git-ref comparison, store reports in a separate history dir or use git stash/worktree.

### Task 03: Full Hook System

**Spec §10.5**

Hook execution at lifecycle points: `pre_generate`, `post_generate`, `pre_build`, `post_build`, `post_report`.

**Contract:**
- `LOOM_CONTEXT_FILE` env var points to JSON context file (spec §10.5.4)
- Exit 0 = success; Exit 1 = fail (halts build); Exit 2 = warning (continues)
- Stdout JSON merged into build report under `hooks.<name>`
- Timeouts: default 300s; configurable per hook
- `allow_failure = true` only valid for `post_build` and `post_report`

**Implementation:**
```rust
pub struct HookRunner {
    pub hooks: HashMap<String, HookConfig>,  // lifecycle_point → config
    pub context_file_path: PathBuf,
}

impl HookRunner {
    pub fn run_hook(
        &self,
        lifecycle: &str,
        build_context: &BuildContext,
        build_result: Option<&BuildResult>,
    ) -> Result<Option<serde_json::Value>, LoomError> {
        // Write context JSON file
        // Set env vars
        // Spawn hook process with timeout
        // Parse stdout as JSON if valid
        // Handle exit codes 0/1/2
    }
}
```

**Context file** written to `.build/<project>/<strategy>/hook_context.json` with full schema from spec §10.5.4.

### Task 04: loom env shell

**Spec §13.3**

`loom env shell` spawns a subshell with the correct tool environment:

```rust
// On Unix
Command::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()))
    .env("PATH", format!("{}/bin:{}", vivado_path, existing_path))
    .env("LM_LICENSE_FILE", license_server)
    .env("PS1", format!("(loom:{}) $ ", project_name))
    .spawn()?.wait()?;
```

The prompt includes the project name. Interactive Vivado works in this shell.

### Task 05: Quartus Backend

**Spec §10.6, §15 Phase 4**

This is the critical "second backend" proving vendor-agnosticism.

**Key differences from Vivado:**
- Scripting: Tcl via `quartus_sh --script build.tcl`
- Fitter combines optimize + place + route into one step
- Constraint format: `.sdc` + `.qsf`
- No OOC synthesis (Quartus Standard); `.qdb` partitions (Quartus Pro)
- IP: Platform Designer, `quartus_ip` generator (uses `qsys-generate`)

**`loom-quartus` crate structure:**
```
crates/loom-quartus/
├── src/
│   ├── lib.rs            (QuartusBackend struct)
│   ├── tcl_gen.rs        (generate Quartus Tcl flow)
│   ├── executor.rs       (spawn quartus_sh, capture logs)
│   ├── env_check.rs      (find quartus_sh, version, license)
│   └── sub_phases.rs     (map generic → Quartus-specific)
```

**Quartus Tcl flow:**
```tcl
# Project setup
package require ::quartus::project
project_new my_design -overwrite

# Source files
set_global_assignment -name SOURCE_FILE /abs/path/rtl/top.sv
set_global_assignment -name SDC_FILE /abs/path/constraints/timing.sdc

# Device
set_global_assignment -name DEVICE 5CSEBA6U23I7

# Compile (Fitter handles place+route)
load_package flow
execute_flow -compile

project_close
```

**Sub-phase mapping for Quartus:**
| Generic | Quartus |
|---|---|
| synthesis | Analysis & Synthesis |
| place | Fitter (placement) |
| route | Fitter (routing) |
| bitstream | Assembler |
| optimize | (N/A — part of Fitter) |

The Quartus backend's `execute_build()` must call `phase_callback.on_phase_complete()` for each Fitter sub-phase to support `--stop-after` and resume. Parse Quartus log for phase transition markers.

**`BackendCapabilities` for Quartus:**
```rust
BackendCapabilities {
    supports_ooc: false,       // Standard edition
    supports_incremental: true, // Rapid recompile mode
    supports_ip_generation: true,
    supports_block_design: true, // Platform Designer
    supports_strategy_sweep: false,
    checkpoint_format: "qdb",   // Quartus Pro; Standard: N/A
    constraint_formats: vec!["sdc", "qsf"],
    sub_phases: vec!["synthesis", "place", "route", "bitstream"],
}
```

### Task 06: BackendCapabilities Model

**Spec §10.6**

Define `BackendCapabilities` struct (see spec for fields). Each backend returns its capabilities from a `capabilities()` method. Framework uses this to:
- Emit diagnostics for unsupported features (e.g., OOC on yosys → "will synthesize in-context")
- Skip sub-phases that don't exist for the backend
- Select incremental mode if supported

### Task 07: Quartus Migration Tools

**Spec §6.6.1 (Phase 4)**

`loom migrate qsf-to-toml <project.qsf>` — parse QSF, extract:
- `set_global_assignment -name DEVICE <part>` → `[target] part`
- `set_global_assignment -name SOURCE_FILE <path>` → `[filesets.synth] files`
- `set_global_assignment -name SDC_FILE <path>` → `[filesets.synth] constraints`
- `set_global_assignment -name IP_FILE <path>` → `[[generators]] plugin = "quartus_ip"`

`loom migrate ip-to-toml <file.ip>` — read Quartus IP parameter file, generate `[[generators]]` block.

### Task 08: Tcl Migration Tools (Vivado)

**Spec §6.6.1**

`loom migrate tcl-audit <script>` — run script in sandboxed Vivado session, instrument file-system access and Vivado API calls, produce report.

`loom migrate tcl-wrap <script>` — generate a declarative generator config from audit results.

Both are Python scripts in `loom-vivado-backend` package.
