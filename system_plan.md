# Loom — FPGA Build System Planning Document

**Status:** Draft v7

---

## 0. Reading Guide for Implementation

This document is the complete specification for Loom. It is designed to be consumed by AI coding agents implementing the system phase by phase. Key conventions:

**Phase tags.** Sections marked `[Phase N]` in the heading indicate when that feature is implemented. Features without a tag are Phase 1 (core). When implementing a specific phase, read all sections for that phase and earlier — skip later phases.

**Spec vs. rationale.** Paragraphs explaining *why* a decision was made are kept brief. Code blocks, TOML examples, interface definitions, and data structures are the primary specification — implement to match them exactly.

**Section cross-references.** `§N.N` references point to other sections. Follow them when you need the full spec for a referenced concept.

### 0.1 Phase Scope Matrix

| Feature | Phase | Key Sections |
|---|---|---|
| **Manifest parsing** (component, project, workspace TOML) | 1 | §3.1, §5.1, §11.2 |
| **Dependency resolution** (workspace-scoped) | 1 | §3.5 |
| **Lockfile** generation and staleness detection | 1 | §3.5.1 |
| **File-set assembly** with constraint scoping/ordering | 1 | §3.3 |
| **Build phases 1-7** pipeline skeleton | 1 | §7.1 |
| **Pre-build validation** (Phase 4 checks) | 1 | §7.4 |
| **Vivado backend** (Tcl generation, batch execution, log capture) | 1 | §10.3.2 |
| **Plugin trait definitions** (interfaces only, not loading) | 1 | §10.1, §10.3 |
| **CLI core** (`build`, `clean`, `env check`, `deps tree`, `lint`) | 1 | §12.2 |
| **Basic error formatting** and exit codes | 1 | §12.4 |
| `command` **generator plugin** | 2 | §6.1, §6.2, §6.4 |
| **Generator DAG** with caching and incrementality | 2 | §6.2, §6.3, §7.3 |
| `vivado_ip` **generator** (declarative IP) | 2 | §6.5 |
| **Constraint templating** (`.tpl` preprocessing) | 2 | §3.3.1 |
| **Build checkpoint/resume** (`--resume`, `--stop-after`, `--start-at`) | 2 | §7.5, §7.2.2 |
| **JSON build report** with hierarchical metrics | 2 | §9.1, §9.4 |
| **Dry run** (`--dry-run`) | 2 | §7.6 |
| `loom lsp` (HDL editor integration) | 2 | §12.3 |
| `loom ip upgrade` with property validation | 2 | §6.5.1 |
| `loom migrate xci-to-toml` | 2 | §6.6.1 |
| **PyO3 plugin loading** (Python plugin host) | 2 | §10.2, §7.7.1 |
| **Windows support** (path handling, PowerShell, CI) | 2 | §13.4 |
| **Platform model** (platform.toml, parameter substitution) | 3 | §4 |
| **Virtual platforms** (sim-only) | 3 | §4.1.1 |
| **Component variants** (overlay model, tag-based selection) | 3 | §3.4 |
| **Build profiles** (simple and dimensional) | 3 | §5.2 |
| **OOC synthesis** per-component caching | 3 | §7.2.1 |
| **Scaffolding** (`loom new component/project/platform`) | 3 | §12.2 |
| **Reporter plugins** and metrics diff | 4 | §9.5, §9.2 |
| **Hook system** with full contract | 4 | §10.5 |
| `loom env shell` | 4 | §13.3 |
| **Quartus backend** | 4 | §10.6, §15 |
| `BackendCapabilities` model | 4 | §10.6 |
| **Simulator plugins** with capability model | 5 | §10.3.3 |
| **Strategy sweeps** (parallel multi-strategy) | 5 | §8.2 |
| **Incremental build** (reference checkpoints) | 5 | §7.3.1 |
| **yosys + nextpnr backend** | 5 | §10.6, §15 |
| **Test organization** model | 6 | §16 |
| **Package registry**, Lattice Radiant, ecosystem | 7 | §15 |

### 0.2 Implementation Stack

| Layer | Technology | Notes |
|---|---|---|
| Core binary | Rust | `clap` for CLI, `toml` crate for parsing, `serde` for serialization |
| Plugin host | PyO3 | Embedded Python interpreter, plugins loaded as Python modules |
| Plugin SDK | Python (`loom.plugin`) | ABC-based interfaces, subprocess execution for actual work |
| Backend packages | Python (`pip install loom-vivado-backend`) | Separate packages per vendor |
| Configuration | TOML | `component.toml`, `project.toml`, `platform.toml`, `workspace.toml` |
| Build artifacts | `.build/` directory | Gitignored, per-project/strategy subdirectories |
| Lockfile | `loom.lock` (TOML) | Committed to VCS |

### 0.3 Phase 1 Implementation Boundaries

Phase 1 scope is deliberately minimal. These features are explicitly **OUT** of Phase 1:

- No generators (no Phase 2 GENERATE step — skip straight from RESOLVE to ASSEMBLE)
- No platforms (project specifies `part` and `backend` directly via `[target]` block)
- No profiles, no variants, no OOC synthesis
- No Python plugin loading (Vivado backend is compiled into the binary as Rust)
- No metrics extraction (build produces exit code + log path, not structured metrics)
- No constraint templating (constraint files are static)
- No `--resume`, `--stop-after`, `--dry-run`
- No parallelism (`-j` flag parsed but ignored)

Phase 1 produces: `loom build` → parse manifests → resolve deps → assemble file-set → generate Vivado Tcl → run `vivado -mode batch` → report pass/fail.

---

## 1. Motivation

FPGA projects sit in an awkward gap between software and physical engineering. They're authored as text, managed in version control, and benefit from CI — but they build slowly, target constrained physical devices, produce non-deterministic results, and depend on heavyweight vendor tools with stateful, imperative interfaces. Existing software build systems (Make, CMake, Bazel) map poorly onto these realities, and vendor-provided project management (Vivado `.xpr`, Quartus `.qpf/.qsf`, Lattice `.rdf`) works against reproducibility and automation. The open-source toolchain (yosys, nextpnr) has better composability but no standard project structure or dependency management.

**Loom** is a layered, extensible build framework for FPGA development. The name reflects its purpose: weaving together components, constraints, and generated artifacts into a cohesive design — much like threads into fabric on a loom (and FPGAs are, after all, programmable logic *fabric*). The core is vendor-agnostic and handles project structure, dependency resolution, code generation orchestration, and build DAG management. Vendor-specific behavior (AMD/Xilinx Vivado, Intel/Altera Quartus, Lattice Radiant, yosys/nextpnr) is provided through a backend plugin system, and higher-level conveniences (IP catalog management, strategy sweeping, metrics dashboards) are built as optional layers on top.

### 1.1 Design Principles

- **Declarative project definition.** A human-readable manifest (TOML) is the single source of truth for every project and component. Everything else is derived.
- **Layered abstraction.** The core framework knows nothing about any specific FPGA vendor. Tool-specific knowledge lives in backend plugins. Convenience features are built on top, not baked in.
- **Extensibility as a first-class goal.** Developers must be able to build generators, backends, reporters, and workflow extensions without modifying the core.
- **Monorepo-native.** The framework natively supports workspaces containing shared component libraries and multiple projects. Reuse across projects within a repository is a primary use case.
- **CI-agnostic.** The core is a CLI tool. It returns structured output (exit codes, JSON reports) that any CI system can consume. CI-specific adapters are optional layers.
- **Platform-aware.** Board-level realities (part, clocks, interfaces, pin assignments) are captured in reusable platform definitions. Projects target platforms, not raw parts, enabling parameterization and portability.
- **Excellent developer experience.** The CLI must have fast startup, responsive interaction, and clean formatted output. Every command should feel polished, from scaffolding to build reporting.
- **No HDL parsing.** The framework treats HDL source files as opaque inputs. It does not attempt to parse SystemVerilog, VHDL, or any hardware description language. If value can be derived from parsing (linting, documentation, dependency inference), that belongs in a separate tool.

---

## 2. Architecture Overview

The system is organized into three layers, with a plugin system providing extensibility at each level.

```
┌───────────────────────────────────────────────────────────────┐
│  Layer 2: Convenience Abstractions (optional, opt-in)         │
│  ┌─────────────┐ ┌──────────────┐ ┌────────────────────────┐ │
│  │ IP Catalog   │ │ Block Design │ │ Strategy Sweep /       │ │
│  │ Management   │ │ Generation   │ │ Timing Closure Assist  │ │
│  └─────────────┘ └──────────────┘ └────────────────────────┘ │
├───────────────────────────────────────────────────────────────┤
│  Layer 1: Tool Plugins                                        │
│  ┌─────────────────────────────────┐ ┌───────────────────────┐│
│  │ Synth/Impl Backends             │ │ Simulator Backends    ││
│  │ ┌────────┐ ┌─────────┐         │ │ ┌────────┐ ┌────────┐ ││
│  │ │ Vivado │ │ Quartus │         │ │ │ Questa │ │Verilat.│ ││
│  │ └────────┘ └─────────┘         │ │ └────────┘ └────────┘ ││
│  │ ┌─────────────┐ ┌────────────┐ │ │ ┌────────┐ ┌────────┐ ││
│  │ │yosys+nextpnr│ │ Radiant .. │ │ │ │  VCS   │ │ Icarus │ ││
│  │ └─────────────┘ └────────────┘ │ │ └────────┘ └────────┘ ││
│  └─────────────────────────────────┘ └───────────────────────┘│
├───────────────────────────────────────────────────────────────┤
│  Layer 0: Core Framework (vendor-agnostic)                    │
│  ┌──────────┐ ┌──────────┐ ┌───────────┐ ┌──────────────────┐│
│  │ Manifest │ │ Dep.     │ │ Build DAG │ │ Plugin Manager,  ││
│  │ Parsing, │ │ Resolver │ │ Builder & │ │ CLI, Platform &  ││
│  │ Validate │ │          │ │ Executor  │ │ Variant Resolver ││
│  └──────────┘ └──────────┘ └───────────┘ └──────────────────┘│
└───────────────────────────────────────────────────────────────┘
```

### 2.1 Layer 0 — Core Framework

Vendor-agnostic. Responsible for:

- Parsing and validating component, platform, and project manifests
- Workspace discovery and layout conventions
- Dependency resolution across components (with a resolution service — no hardcoded paths)
- Platform resolution, parameter substitution, and variant selection
- Project profile overlay resolution
- Build DAG construction, including ordering generators and detecting generator-to-generator dependencies
- Cache-key computation and incremental build decisions
- Lifecycle hook execution (pre-build, post-build, etc.)
- CLI skeleton and plugin loading
- Structured output (JSON build reports)

The core defines **interfaces** that plugins implement. It never imports or references any vendor-specific code.

### 2.2 Layer 1 — Tool Plugins

Tool plugins come in two distinct types: **synthesis/implementation backends** and **simulator backends**. These are separate plugin interfaces because the execution models differ fundamentally.

**Synthesis/Implementation backends** translate the resolved project description into tool-specific build scripts, execute the vendor tool in batch mode, extract build artifacts and metrics by querying the vendor tool post-build, and validate the environment. Planned backends:

- **AMD/Xilinx Vivado** — The initial, most fully-featured backend. Non-project-mode Tcl flow.
- **Intel/Altera Quartus Prime** — Pro and Standard editions, with Platform Designer IP support.
- **Lattice Radiant** — For iCE40 UltraPlus, CrossLink-NX, CertusPro-NX.
- **yosys + nextpnr** — Open-source synthesis (yosys) and place-and-route (nextpnr-ice40, nextpnr-ecp5, nextpnr-gowin). For Lattice iCE40/ECP5, Gowin, and experimental architectures.
- **Lattice Diamond** — For older Lattice ECP5 and MachXO families (legacy).

The architecture must support adding new backends without changes to the core. Each backend maps the framework's generic build sub-phases (synthesis → optimize → place → route → bitstream) to its own tool-specific commands.

**Simulator backends** handle the compile → elaborate → simulate pipeline for verification tools (Questa, VCS, Xcelium, Vivado Simulator, Verilator, Icarus). A project may use a different simulator than its synthesis backend — e.g., synthesize with Vivado but simulate with Questa.

### 2.3 Layer 2 — Convenience Abstractions

Optional, higher-level features built on top of the backend interface. These address specific pain points and are opt-in. Some are backend-specific, others are generic:

- Declarative IP configuration (Vivado: generate `.xci` from TOML; Quartus: generate `.ip` from TOML; yosys: N/A)
- Block design lifecycle management (Vivado: `.bd` from canonical Tcl; Quartus: Platform Designer `.qsys`)
- Multi-strategy parallel implementation for timing closure
- Metrics regression tracking across commits

---

## 3. Component Model [Phase 1]

A **component** is the unit of reuse — an RTL module or collection of related modules consumed by projects or other components.

### 3.1 Component Manifest

Each component is defined by a `component.toml` in its directory:

```toml
[component]
name = "acmecorp/axi_async_fifo"    # namespaced: org/component
version = "1.2.0"
description = "Async FIFO with AXI-Stream interface and CDC"

[filesets.synth]
files = [
    "rtl/axi_async_fifo.sv",
    "rtl/cdc_gray_counter.sv",
]
constraints = ["constraints/cdc_false_paths.xdc"]  # or .sdc, .lpf, .pcf per backend
constraint_scope = "component"    # see §3.3

[filesets.sim]
files = ["tb/axi_async_fifo_tb.sv"]
include_synth = true              # sim fileset also includes synth files
defines = ["SIMULATION"]          # preprocessor defines for simulation
compile_options = ["+acc", "-sv"]  # tool-agnostic compile flags (passed through)

[dependencies]
axi_common = ">=1.0.0"           # resolved by the workspace resolution service

[synth]
ooc = false                       # true to enable out-of-context synthesis (see §7.2)
```

#### 3.1.1 Component Namespacing

Component names use an `org/name` namespace format (e.g., `acmecorp/axi_common`). This is enforced from day one to prevent name collisions when the package registry is introduced in Phase 7.

For workspace-local development, the namespace is the organizational prefix — typically a company name, team name, or project umbrella. Within a workspace, dependencies can be referenced by short name (`axi_common`) when unambiguous, or by full namespace (`acmecorp/axi_common`) when disambiguation is needed. The lockfile always records the full namespaced name.

When publishing to a future registry, the namespace becomes the package scope — similar to npm's `@org/package` or Rust's crate naming conventions. Establishing the convention now means no migration cost later.

#### 3.1.2 Simulation Compile Options

Simulation often requires different compilation flags, preprocessor defines, and tool-specific options per testbench or per simulator. The fileset model supports this at multiple levels:

**Component-level** (in `component.toml`): Default defines and compile options for all simulation of this component, as shown above.

**Project-level** (in `project.toml`): Override or extend simulation options for the project's test suite:

```toml
[filesets.sim]
files = ["tb/radar_top_tb.sv"]
include_synth = true
defines = ["SIMULATION", "ASSERTIONS_ON", "COVERAGE_EN"]

# Tool-specific compile options (only applied when using that simulator)
[filesets.sim.tool_options.questa]
compile = ["+acc=rnbp", "-coverage", "-assertdebug"]
elaborate = ["-coverage"]
simulate = ["-coverage", "-assertcover"]

[filesets.sim.tool_options.verilator]
compile = ["--trace", "--coverage", "--assert"]
```

The simulator plugin receives both the generic options and the tool-specific options. Generic options (`defines`, `compile_options`) are translated by the plugin into tool-appropriate flags. Tool-specific options (`tool_options.<name>`) are passed through verbatim.

### 3.2 What the Component Manifest Does Not Contain

- **HDL parameters/generics.** Parameterization is handled within the HDL via instantiation. The build system does not need to know about `DATA_WIDTH` or `DEPTH`. If parameter metadata is useful for documentation or discoverability, that belongs in a separate documentation layer, not in the build manifest. Platform-specific values that must flow into HDL (like DDR data width) use preprocessor defines — see §4.2.1.
- **Port definitions or interface contracts.** The build system treats HDL files as opaque. It does not parse them.
- **Tool-specific settings.** A reusable component should not embed Vivado synthesis directives. If tool-specific overrides are needed, they belong in the consuming project, in a component variant (see §3.4), or in the platform definition (see §4).

### 3.3 Constraint Scoping and Templating [Phase 1 scoping, Phase 2 templating]

Components may carry constraints (e.g., CDC false-path declarations, timing exceptions). The constraint file format is backend-specific — `.xdc` for Vivado, `.sdc` for Quartus, `.lpf` for Lattice Radiant, `.pcf` for ice40/nextpnr — but the framework handles scoping and ordering uniformly regardless of format. The `constraint_scope` field tells the framework how to integrate them:

- `"component"` — The constraint file uses relative references and should be scoped to the component's hierarchy in the design. The backend applies the appropriate scoping mechanism for its tool (e.g., Vivado's `SCOPED_TO_REF`, Quartus's `set_instance_assignment`).
- `"global"` — The constraint file contains global declarations. It is added to the project's constraint set without scoping.
- Default is `"component"` to encourage composable constraints.

The framework's constraint assembly logic collects all component-scoped constraints, applies scoping, orders them before project-level global constraints, and produces the final ordered constraint list for the backend.

#### 3.3.1 Constraint Templating [Phase 2]

Constraint files frequently need platform-specific values — clock periods, pin locations, I/O standards. Forcing every parameterized constraint through a full generator is disproportionately heavy for what is usually simple value substitution. Loom provides a lightweight template mechanism for this case.

Any constraint file with a `.tpl` extension appended (e.g., `.xdc.tpl`, `.sdc.tpl`, `.lpf.tpl`, `.pcf.tpl`) is treated as a template. Template files use `{{parameter}}` syntax for substitution, and the framework preprocesses them before passing them to the backend:

```
# constraints/timing.xdc.tpl  (Vivado/AMD)
create_clock -period {{platform.clocks.sys_clk.period_ns}} -name sys_clk [get_ports sys_clk_p]
```

```
# constraints/timing.sdc.tpl  (Quartus/Intel)
create_clock -period {{platform.clocks.sys_clk.period_ns}} -name sys_clk [get_ports sys_clk_p]
```

```
# constraints/pins.pcf.tpl  (ice40/nextpnr)
set_io sys_clk {{platform.pins.sys_clk}}
set_io led[0] {{platform.pins.led_0}}
```

The template namespace includes:

- `platform.*` — All platform parameters and clock definitions
- `project.*` — Project-level parameters (including profile overrides)
- `component.*` — Component metadata (name, version)

The preprocessed `.xdc` file is written to the build directory and added to the file-set in place of the template. Template outputs are cached — they are only regenerated when the template source or the referenced parameters change.

**When to use templates vs. generators:** Templates are for simple value substitution in constraint files. If the constraint logic itself is conditional or complex (e.g., generating constraints for a variable number of clock domains), use a generator instead. The rule of thumb: if you need `if` or `for`, use a generator.

The template file is declared in the fileset like any other constraint:

```toml
[filesets.synth]
constraints = [
    "constraints/timing.xdc.tpl",     # Vivado: preprocessed by the framework
    "constraints/physical.xdc",        # Vivado: passed through unchanged
]
# For Quartus:  constraints = ["constraints/timing.sdc.tpl", "constraints/pins.qsf"]
# For nextpnr:  constraints = ["constraints/pins.pcf.tpl"]
```

### 3.4 Component Variants [Phase 3]

Some components have vendor-specific or context-specific implementations. For example, a memory controller wrapper might have a Xilinx variant (using MIG) and an Intel variant (using EMIF), or a component might have a "simulation" variant that substitutes behavioral models for synthesis primitives.

Variants are declared as overlays on the base component. The base file-set is always present; a variant can add files, remove files, add constraints, or add generators.

```toml
[component]
name = "memory_ctrl"
version = "1.0.0"
description = "DDR4 memory controller wrapper"

[filesets.synth]
files = ["rtl/memory_ctrl.sv", "rtl/memory_ctrl_arbiter.sv"]

[filesets.sim]
files = ["tb/memory_ctrl_tb.sv"]
include_synth = true

# --- Variant: Xilinx ---
[variants.xilinx]
description = "Xilinx MIG-based implementation"
tags = ["vendor:xilinx"]

[variants.xilinx.filesets.synth]
add_files = ["rtl/xilinx/mig_wrapper.sv"]
add_constraints = ["constraints/xilinx/mig_timing.xdc"]

[[variants.xilinx.generators]]
name = "mig_ip"
plugin = "vivado_ip"
[variants.xilinx.generators.config]
vlnv = "xilinx.com:ip:mig_7series:4.2"
# ...

# --- Variant: intel ---
[variants.intel]
description = "Intel EMIF-based implementation"
tags = ["vendor:intel"]

[variants.intel.filesets.synth]
add_files = ["rtl/intel/emif_wrapper.sv"]
add_constraints = ["constraints/intel/emif_timing.sdc"]

[[variants.intel.generators]]
name = "emif_ip"
plugin = "quartus_ip"
[variants.intel.generators.config]
ip_name = "altera_emif"
properties = { MEM_FORMAT = "DDR4", DQ_WIDTH = "${platform.params.ddr4_data_width}" }

# --- Variant: sim ---
[variants.sim]
description = "Behavioral model for simulation"
tags = ["sim"]

[variants.sim.filesets.synth]
remove_files = ["rtl/xilinx/mig_wrapper.sv", "rtl/intel/emif_wrapper.sv"]
add_files = ["rtl/sim/memory_model.sv"]
```

**Variant selection** is driven by the consuming project or platform. The project manifest specifies which variant to use for each dependency that has variants:

```toml
[dependencies]
memory_ctrl = { version = ">=1.0.0", variant = "xilinx" }
```

Or, more powerfully, the platform definition (see §4) can provide default variant selections based on tags, so that targeting a Xilinx platform automatically selects `vendor:xilinx` variants across all dependencies.

**Variant resolution priority.** When multiple sources could determine the active variant for a component, the framework applies a strict priority order (highest wins):

1. **Project explicit dependency override.** `variant = "xilinx"` in the project's dependency declaration. This is the most specific and always wins.
2. **Profile override.** A build profile can override variant selections for specific dependencies.
3. **Platform `variant_defaults.tags`.** If the platform declares `tags = ["vendor:xilinx"]`, any component with a variant matching that tag uses it as the default.
4. **Component default.** If no variant is selected by any of the above, the component's base file-set is used (no variant overlay applied).

The framework logs the resolved variant for each component during Phase 1 (RESOLVE). When the variant was selected by platform defaults (priority 3), the log indicates this explicitly so users can trace the decision:

```
  acmecorp/memory_ctrl v1.0.0 → variant "xilinx" (from platform "zcu104" tag "vendor:xilinx")
  acmecorp/axi_common v1.3.0 → no variant (no match)
```

If multiple platform tags match different variants of the same component, the build fails with an ambiguity error rather than silently picking one.

### 3.5 Dependency Resolution [Phase 1]

Components declare dependencies by name and version constraint. The framework provides a **resolution service** that maps names to locations without requiring explicit paths in every manifest.

Resolution sources, checked in order:

1. **Workspace members.** Components discovered via the `workspace.toml` member globs.
2. **Registry (future).** A package registry for cross-repository sharing. Out of scope for initial implementation, but the resolution interface is designed to accommodate it from day one (see below).
3. **Explicit path override.** A project or workspace can pin a dependency to a specific path, overriding resolution. Useful for development and testing.

```toml
# workspace.toml — resolution configuration
[workspace]
members = ["lib/*", "projects/*"]

[resolution.overrides]
# Pin a dependency to a specific path for local development
some_external_lib = { path = "/home/user/dev/some_external_lib" }
```

The resolver builds a dependency graph and checks for cycles, version conflicts, and missing dependencies before any build work begins.

#### 3.5.1 Lockfile [Phase 1]

Resolved dependency versions are recorded in a **lockfile** (`loom.lock`) at the workspace root. The lockfile captures the exact resolved version of every dependency, ensuring that two builds of the same commit on different machines produce identical dependency graphs.

```toml
# loom.lock — auto-generated, committed to version control
[metadata]
loom_version = "0.1.0"
generated_at = "2026-03-03T14:00:00Z"

# Resolved workspace member paths — used for staleness detection.
# If the glob resolves to a different set of paths, the lockfile is stale.
workspace_members = [
    "lib/axi_common",
    "lib/axi_async_fifo",
    "lib/dsp_primitives",
    "platforms/zcu104",
    "projects/radar_processor",
]

[[package]]
name = "acmecorp/axi_common"
version = "1.3.0"
source = "workspace:lib/axi_common"
checksum = "sha256:abc123..."

[[package]]
name = "acmecorp/axi_async_fifo"
version = "1.2.0"
source = "workspace:lib/axi_async_fifo"
checksum = "sha256:def456..."
dependencies = ["acmecorp/axi_common >=1.0.0"]

# Resolved IP versions for vendor IP generators with floating versions.
# Ensures reproducibility across machines with the same tool version.
[[ip_resolution]]
generator = "radar_processor:sys_clk"
backend = "vivado"
ip_requested = "xilinx.com:ip:clk_wiz"
ip_resolved = "xilinx.com:ip:clk_wiz:6.0"
tool_version = "2023.2"
```

Behavior:

- `loom build` uses the lockfile if present, failing if the lockfile is stale (i.e., manifests have changed in ways that invalidate the lock).
- `loom deps update` re-resolves all dependencies and regenerates the lockfile, including IP version resolution.
- `loom deps update <n>` re-resolves a single dependency.
- The lockfile is committed to version control. CI builds fail if the lockfile is missing or stale.

**IP version locking.** When a vendor IP generator uses a floating version (e.g., `vivado_ip` with no VLNV version, or `quartus_ip` with no pinned variant), the first resolution queries the tool's IP catalog and records the resolved version in the lockfile's `[[ip_resolution]]` section. Subsequent builds use the locked version. `loom deps update` re-resolves IP versions along with dependencies. This ensures that two machines with the same tool version and the same lockfile produce identical IP output products. If the tool version changes, the IP resolution section becomes stale and must be re-resolved.

**Tool upgrade risk.** A vendor tool version upgrade forces re-resolution of all floating IPs simultaneously. For projects with dozens of IPs, this is a risky operation: new IP versions may have renamed properties, changed defaults, or behavioral changes. The mitigation strategy is layered:

1. **Before upgrading:** Run `loom ip upgrade --tool-version <new> --check-properties` (see §6.5.1). This validates property names without modifying anything.
2. **Incremental adoption:** For large projects, teams can temporarily pin critical IPs to exact versions (changing from floating to pinned) and upgrade them one at a time, rather than resolving everything at once.
3. **After upgrading:** The build report records the resolved IP version for every generator, so the effect of the upgrade is auditable post-hoc.

**Staleness detection.** The lockfile is stale when semantic dependency content changes:

- Dependency added, removed, or version constraint changed
- Component `version` or `name` field changed
- Workspace member set changed (glob resolves to different paths vs. `workspace_members`)
- Resolution override added, removed, or changed
- Active vendor tool version changed (invalidates `[[ip_resolution]]`)

**Not stale when:** source files change (cache's job), comments/descriptions change, or non-dependency metadata changes. Staleness uses semantic hashing of dependency graph inputs, not file timestamps.

**Workspace member tracking.** The `workspace_members` field records resolved glob paths at lockfile generation time. On build, Loom re-evaluates globs and compares — if they differ, the lockfile is stale. This avoids needing full resolution just to detect staleness.

For workspace-only resolution (Phase 1), the lockfile is simple — it records which workspace member satisfied each dependency and a content hash. When registry support is added later, the lockfile becomes essential for pinning remote dependency versions.

#### 3.5.2 Resolution Architecture [Phase 1]

The resolution service is implemented behind a trait with an IO-injectable backend, so that workspace-local resolution (Phase 1) and registry-based resolution (future) share the same interface:

```rust
trait DependencySource {
    /// Resolve a dependency name + constraint to a concrete location.
    /// Returns None if this source cannot satisfy the dependency.
    async fn resolve(&self, name: &str, constraint: &VersionReq) 
        -> Result<Option<ResolvedDependency>>;
    
    /// List all available versions of a dependency.
    async fn list_versions(&self, name: &str) 
        -> Result<Vec<Version>>;
}
```

The async interface accommodates network-based registries without requiring a refactor when that capability is added. For Phase 1 (workspace-only), the implementation is synchronous under the hood but conforms to the async interface.

---

## 4. Platform Model [Phase 3]

A **platform** captures the board-level reality that a project targets: the FPGA part, available clocks and their frequencies, peripheral interfaces, pin assignments, and other physical constraints. Platforms sit between components and projects — they provide the physical context that projects build against.

### 4.1 Platform Manifest

```toml
[platform]
name = "zcu104"
description = "Xilinx ZCU104 Evaluation Board"
part = "xczu7ev-ffvc1156-2-e"
board = "xilinx.com:zcu104:part0:1.1"    # optional board identifier

[platform.tool]
backend = "vivado"
version = "2023.2"

# Clock definitions — these are the physical clocks available on the board.
# Projects and generators can reference these by name.
[platform.clocks.sys_clk]
frequency_mhz = 125.0
period_ns = 8.0                          # derived: 1000 / frequency_mhz
pin = "H9"
standard = "LVDS"
description = "125 MHz system clock from Si5341"

[platform.clocks.user_clk]
frequency_mhz = 300.0
period_ns = 3.333
pin = "G10"
standard = "DIFF_SSTL12"
description = "300 MHz user clock from DDR4 PLL"

# Platform-level constraints (pin assignments, I/O standards, etc.)
[platform.constraints]
files = [
    "constraints/pins.xdc",
    "constraints/io_standards.xdc",
    "constraints/clocks.xdc",
]

# Platform parameters — values that projects or generators can reference.
# These parameterize the physical board, not the HDL.
[platform.params]
ddr4_data_width = 64
pcie_lanes = 4
has_hdmi = true
ethernet_speed = "1G"

# Default variant selections for this platform.
# When a component has variants with matching tags, these are selected
# automatically unless the project overrides them.
[platform.variant_defaults]
tags = ["vendor:xilinx"]
```

The platform model is vendor-agnostic. Here are equivalent examples for other toolchains:

```toml
# platform.toml — Intel/Quartus example
[platform]
name = "de10_nano"
description = "Terasic DE10-Nano with Cyclone V SoC"
part = "5CSEBA6U23I7"

[platform.tool]
backend = "quartus"
version = "23.1"
edition = "standard"      # or "pro" — affects available features

[platform.clocks.fpga_clk]
frequency_mhz = 50.0
period_ns = 20.0
pin = "V11"
standard = "3.3-V LVTTL"

[platform.constraints]
files = ["constraints/pins.qsf", "constraints/timing.sdc"]

[platform.params]
hps_enabled = true
sdram_data_width = 16

[platform.variant_defaults]
tags = ["vendor:intel"]
```

```toml
# platform.toml — Open-source toolchain (iCE40 + yosys/nextpnr)
[platform]
name = "icebreaker"
description = "1BitSquared iCEBreaker with iCE40UP5K"
part = "ice40up5k-sg48"

[platform.tool]
backend = "yosys_nextpnr"
# No version pinning — OSS tools use rolling releases.
# Pin via Nix flake or tool hash if reproducibility needed.

[platform.clocks.sys_clk]
frequency_mhz = 12.0
period_ns = 83.333
pin = "35"

[platform.constraints]
files = ["constraints/pins.pcf"]

[platform.params]
leds = 5
pmod_count = 2

[platform.variant_defaults]
tags = ["vendor:lattice", "toolchain:oss"]
```

#### 4.1.1 Virtual Platforms [Phase 3]

Not every project targets a physical board. IP development, simulation-only testbenches, and early prototyping may not have a target part or physical constraints. A **virtual platform** provides parameter context without requiring physical board details:

```toml
[platform]
name = "simulation_generic"
description = "Generic simulation environment for IP development"
virtual = true                            # no part, no physical constraints

[platform.tool]
# No backend required for virtual platforms
# Projects targeting virtual platforms can only simulate, not synthesize

[platform.clocks.sys_clk]
frequency_mhz = 100.0
period_ns = 10.0
# No pin or I/O standard — virtual clocks have no physical binding

[platform.simulation]
default_simulator = "verilator"

[platform.params]
data_width = 32
addr_width = 16
```

Virtual platforms enable `loom sim` but disallow `loom build` — there is no part to synthesize for. This is useful for component-level verification where the testbench exercises the component in isolation. If a user tries `loom build` against a virtual platform, the framework errors with a clear message: "Project targets virtual platform 'simulation_generic' which has no FPGA part. Use `loom sim` for simulation, or change the platform to a physical board."

### 4.2 Platform as Parameterization Surface [Phase 3]

Platforms provide named values that generators, IP configs, and constraint templates reference. This enables targeting multiple boards by switching the platform:

```toml
# In a generator or IP config, reference platform values:
[[generators]]
name = "sys_pll"
plugin = "vivado_ip"
[generators.config]
vlnv = "xilinx.com:ip:clk_wiz"
properties = { PRIM_IN_FREQ = "${platform.clocks.sys_clk.frequency_mhz}" }
```

The `${platform....}` syntax is expanded by the framework during manifest resolution. This is intentionally limited to simple value substitution — not a full expression language. If complex logic is needed, that belongs in a generator script that receives platform parameters as inputs.

#### 4.2.1 Platform Parameters in HDL [Phase 3]

Some platform-specific values (DDR data width, PCIe lane count) must flow into RTL via preprocessor defines. The `defines` mechanism supports platform parameter substitution:

```toml
# project.toml
[filesets.synth]
files = ["src/radar_top.sv"]
defines = [
    "DDR4_DATA_WIDTH=${platform.params.ddr4_data_width}",
    "PCIE_LANES=${platform.params.pcie_lanes}",
    "SYS_CLK_FREQ_MHZ=${platform.clocks.sys_clk.frequency_mhz}",
]
```

These expand to `-define DDR4_DATA_WIDTH=64` (or the tool-equivalent) during file-set assembly. The RTL receives them as standard preprocessor defines:

```systemverilog
// radar_top.sv
`ifndef DDR4_DATA_WIDTH
  `define DDR4_DATA_WIDTH 64  // fallback default for standalone use
`endif

module radar_top (
    // ...
);
    localparam DDR_WIDTH = `DDR4_DATA_WIDTH;
endmodule
```

**Pattern for parameterized IP.** When IP configurations depend on platform parameters, the `vivado_ip` generator config references platform values directly:

```toml
[[generators]]
name = "sys_pll"
plugin = "vivado_ip"
[generators.config]
vlnv = "xilinx.com:ip:clk_wiz"
properties = {
    PRIM_IN_FREQ = "${platform.clocks.sys_clk.frequency_mhz}",
    CLKOUT1_REQUESTED_OUT_FREQ = "${project.params.dsp_clk_freq}",
}
```

This chains naturally with platform switching: targeting a different platform changes the input clock frequency, which regenerates the PLL IP with the correct configuration.

### 4.3 Platform Discovery [Phase 3]

Platforms are stored in the workspace and discovered like components:

```
repo/
├── platforms/
│   ├── zcu104/
│   │   ├── platform.toml
│   │   └── constraints/
│   ├── kcu116/
│   │   ├── platform.toml
│   │   └── constraints/
│   └── custom_board_v2/
│       ├── platform.toml
│       └── constraints/
```

The workspace manifest includes platforms in its member globs:

```toml
[workspace]
members = ["lib/*", "projects/*", "platforms/*"]
```

---

## 5. Project Model [Phase 1]

A **project** is the top-level build target. It composes components and project-specific sources, targeting a platform (or directly specifying a part).

### 5.1 Project Manifest [Phase 1]

```toml
[project]
name = "radar_processor"
top_module = "radar_top"
description = "Main radar signal processing FPGA"

# Target a platform (preferred) or specify a part directly.
platform = "zcu104"

# Direct part specification (alternative to platform, for simple cases):
# [target]
# part = "xcvu9p-flga2104-2L-e"
# backend = "vivado"
# version = "2023.2"

[filesets.synth]
files = [
    "src/radar_top.sv",
    "src/beam_former.sv",
    "src/ddc_chain.sv",
]
constraints = [
    "constraints/timing.xdc",
    "constraints/physical.xdc",
]

[filesets.sim]
files = ["tb/radar_top_tb.sv"]
include_synth = true

[dependencies]
axi_async_fifo = ">=1.0.0"
axi_common = ">=1.0.0"
dsp_primitives = ">=2.0.0"
memory_ctrl = { version = ">=1.0.0", variant = "xilinx" }

# Generators — see §6
[[generators]]
name = "regmap"
# ...

# Build configuration — see §8
[build]
# ...
```

When a project targets a platform, the platform provides: part, backend selection, tool version, base constraints, clock definitions, variant defaults, and parameters. The project can override any of these.

### 5.2 Build Profiles [Phase 3]

Many real workflows require building the same design with controlled differences: targeting different boards, enabling/disabling features, adjusting clock frequencies for different product tiers. Rather than duplicating entire project manifests, the framework supports **build profiles**.

#### 5.2.1 Simple Profiles

In the simplest case, a profile is a named overlay on the base project that can modify specific fields:

```toml
[project]
name = "radar_processor"
top_module = "radar_top"
platform = "zcu104"

# ... base filesets, dependencies, generators, build config ...

# --- Build Profiles ---

[profiles.kcu116_port]
description = "Port to KCU116 evaluation board"
platform = "kcu116"
# Overrides just the platform. Everything else (sources, deps, generators)
# stays the same. Platform change cascades to part, constraints, clock
# frequencies, and variant selections.

[profiles.reduced_channels]
description = "2-channel version for lower-tier product"
[profiles.reduced_channels.params]
num_channels = 2
[profiles.reduced_channels.filesets.synth]
add_files = ["src/reduced_channel_config.sv"]

[profiles.timing_debug]
description = "Extra timing constraints for debug builds"
[profiles.timing_debug.filesets.synth]
add_constraints = ["constraints/debug_timing.xdc"]
[profiles.timing_debug.build]
default_strategy = "aggressive"
```

Simple profiles are selected on the CLI:

```
loom build                                # base project
loom build --profile kcu116_port          # build the KCU116 profile
loom build --profile reduced_channels     # build the 2-channel version
```

#### 5.2.2 Profile Dimensions and Composition

Simple profiles don't compose — selecting `kcu116_port` doesn't combine with `reduced_channels`. When you have orthogonal concerns (board × feature tier × debug level), the combinatorial explosion of simple profiles becomes unmanageable.

**Profile dimensions** solve this. A dimension is a named axis of variation with a set of choices that compose orthogonally:

```toml
[profile_dimensions.board]
description = "Target board"
default = "zcu104"

[profile_dimensions.board.choices.zcu104]
platform = "zcu104"

[profile_dimensions.board.choices.kcu116]
platform = "kcu116"

[profile_dimensions.board.choices.custom_v2]
platform = "custom_board_v2"

[profile_dimensions.tier]
description = "Product feature tier"
default = "full"

[profile_dimensions.tier.choices.full]
[profile_dimensions.tier.choices.full.params]
num_channels = 8

[profile_dimensions.tier.choices.reduced]
[profile_dimensions.tier.choices.reduced.params]
num_channels = 2
[profile_dimensions.tier.choices.reduced.filesets.synth]
add_files = ["src/reduced_channel_config.sv"]

[profile_dimensions.debug]
description = "Debug level"
default = "off"

[profile_dimensions.debug.choices.off]
# no changes

[profile_dimensions.debug.choices.timing]
[profile_dimensions.debug.choices.timing.filesets.synth]
add_constraints = ["constraints/debug_timing.xdc"]
[profile_dimensions.debug.choices.timing.build]
default_strategy = "aggressive"
```

Dimensional profiles are selected by specifying one choice per dimension:

```
loom build --profile board=kcu116,tier=reduced
loom build --profile board=zcu104,tier=full,debug=timing
loom build --profile tier=reduced    # other dimensions use defaults
```

Each unique combination of dimension choices produces its own build in a subdirectory named by the combination (`kcu116.reduced.off/`).

**Overlay application order.** When multiple dimensions are active, their overlays are applied in the order the dimensions are declared in the manifest. Within each overlay, the rules are: `platform` replaces, `params` merges (later wins), `add_files` appends, `add_constraints` appends, `build` merges.

**Invalid combinations.** Some dimension combinations may be invalid (e.g., the custom board doesn't support the full-channel mode). The project can declare exclusions:

```toml
[profile_exclusions]
rules = [
    { board = "custom_v2", tier = "full", reason = "Custom board lacks bandwidth for 8 channels" },
]
```

`loom build --profile board=custom_v2,tier=full` fails at validation with the specified reason.

**Sweeping.** `loom build --profile-all` builds all valid combinations across all dimensions. `loom build --profile-all board` sweeps one dimension while holding others at their defaults. This is useful for CI matrices.

#### 5.2.3 Choosing Between Simple and Dimensional Profiles

Simple profiles and dimensional profiles can coexist in the same manifest. Use simple profiles for one-off configurations that don't compose. Use dimensions when you have two or more orthogonal axes of variation. The framework validates that simple profile names don't collide with dimension-generated names.

Each profile (whether simple or dimensional) produces its own build artifacts in a separate subdirectory.

### 5.3 Multi-Project Repositories [Phase 1]

For repositories containing multiple FPGA designs (e.g., multiple boards or subsystems), each project has its own `project.toml` in its own directory. There is no cross-project build dependency — if two FPGAs are in the same system, they are independent builds that happen to share components from the same workspace.

---

## 6. Code Generation [Phase 2]

Code generation is a first-class concept. Any file that is **derived** rather than **authored** is the output of a generator, managed as a node in the build DAG.

### 6.1 Generator Declaration

Generators are declared in component or project manifests:

```toml
[[generators]]
name = "regmap"
plugin = "command"                # generator plugin type
command = "python scripts/gen_regs.py"
inputs = ["regs/radar_ctrl_regs.yaml"]
outputs = ["generated/radar_ctrl_regs.sv", "generated/radar_ctrl_regs.h"]
fileset = "synth"                 # which fileset receives the outputs
```

### 6.2 Generator Execution Model

The framework's contract with a generator:

1. **Inputs are declared.** The generator lists all files it reads. The framework hashes these to determine if regeneration is needed.
2. **Outputs are declared.** The generator lists all files it produces. After execution, the framework verifies these exist.
3. **Execution is opaque.** The framework runs the command (or delegates to a generator plugin) and does not inspect what happens internally.
4. **Outputs join a fileset.** Generated files are added to the specified fileset and participate in all subsequent build stages.
5. **Caching.** The framework computes a cache key from: input file content hashes, the command/config, and the generator plugin version. If the cache key matches a previous run, generation is skipped.

### 6.3 Generator-to-Generator Dependencies

A generator's declared inputs may overlap with another generator's declared outputs. The framework detects this and orders execution accordingly. Explicit ordering is also supported:

```toml
[[generators]]
name = "system_spec_parser"
plugin = "command"
command = "python scripts/parse_system.py"
inputs = ["spec/system.json"]
outputs = ["generated/regmap.yaml", "generated/memory_map.h"]

[[generators]]
name = "regmap"
plugin = "command"
command = "python scripts/gen_regs.py"
inputs = ["generated/regmap.yaml"]    # output of system_spec_parser
outputs = ["generated/ctrl_regs.sv"]
fileset = "synth"
# Framework detects the dependency via input/output overlap.
# Explicit declaration is also supported:
# depends_on = ["system_spec_parser"]
```

### 6.4 Generator Plugin Types

The core ships with a minimal set of built-in generator plugins. Additional types are provided by backend plugins or third-party extensions.

**Core generator plugins:**

| Plugin Name | Description |
|---|---|
| `command` | Run an arbitrary shell command. The most general escape hatch. |
| `python` | Run a Python script with optional virtualenv/dependency management. |

**Backend-provided generator plugins:**

| Plugin Name | Backend | Description |
|---|---|---|
| `vivado_ip` | Vivado | Generate Xilinx IP from a declarative configuration. See §6.5. |
| `vivado_bd` | Vivado | Regenerate a block design from a canonical Tcl export. |
| `vitis_hls` | Vivado | Run Vitis HLS to produce RTL from C/C++ sources. |
| `quartus_ip` | Quartus | Generate Intel IP from a declarative configuration (Platform Designer). |
| `quartus_qsys` | Quartus | Regenerate a Platform Designer subsystem. |

**Third-party generator plugins (examples):**

| Plugin Name | Description |
|---|---|
| `chisel` | Compile Chisel/FIRRTL to Verilog. |
| `amaranth` | Run Amaranth to produce RTL. |
| `systemrdl` | Compile SystemRDL register descriptions. |
| `spinalhdl` | Compile SpinalHDL to Verilog/VHDL. |

### 6.5 Vendor IP Generation [Phase 2]

Every major FPGA vendor provides parameterizable IP cores (PLLs, memory controllers, PCIe). Version-controlling vendor-generated files (`.xci`, `.ip`, `.qsys`) causes friction: opaque, tool-version-dependent, merge-hostile. Loom's approach: declare IP config in the manifest, generate vendor files at build time.

Each backend provides an IP generator plugin: specify the IP identifier and non-default properties, backend generates output products during Phase 2.

#### 6.5.0 Vivado IP [Phase 2] (`vivado_ip`)

```toml
[[generators]]
name = "sys_clk"
plugin = "vivado_ip"
[generators.config]
vlnv = "xilinx.com:ip:clk_wiz"              # no version: use latest compatible
properties = { PRIM_IN_FREQ = "${platform.clocks.sys_clk.frequency_mhz}", CLKOUT1_REQUESTED_OUT_FREQ = "100.000" }
```

The Vivado backend plugin translates this into Tcl that creates the IP, applies configuration, and generates output products. The `.xci` is a build artifact, not a versioned source. This eliminates Vivado version upgrade friction for IP configuration.

#### 6.5.0a Quartus IP [Phase 4] (`quartus_ip`)

```toml
[[generators]]
name = "sys_pll"
plugin = "quartus_ip"
[generators.config]
ip_name = "altera_pll"
properties = {
    gui_reference_clock_frequency = "${platform.clocks.sys_clk.frequency_mhz}",
    gui_output_clock_frequency0 = "100.0",
    gui_number_of_clocks = "2",
}
```

The Quartus backend generates a `.ip` parameter file and runs `qsys-generate` or the IP generation Tcl flow. Like Vivado, the generated files are build artifacts.

For open-source flows (yosys + nextpnr), there is no vendor IP catalog — PLL and memory primitives are instantiated directly in RTL using architecture-specific primitives. The `command` or `python` generator can produce parameterized wrapper modules if needed.

#### 6.5.1 VLNV Version Strategy [Phase 2] (Vivado-Specific)

Vivado rigidly locks IP catalog versions to Vivado versions. `clk_wiz:6.0` exists in 2023.2, but becomes `clk_wiz:6.1` in 2024.1. Hardcoding the IP version in `component.toml` causes builds to break on tool upgrades.

The `vivado_ip` generator supports three version strategies:

**Floating (default, recommended).** Omit the version from the VLNV string. The backend resolves to the latest compatible version for the active Vivado installation:

```toml
vlnv = "xilinx.com:ip:clk_wiz"              # latest for this Vivado version
```

**Pinned.** Specify an exact version when a specific IP version is required (e.g., for regression testing or known-good configurations):

```toml
vlnv = "xilinx.com:ip:clk_wiz:6.0"          # exact version
```

**Range.** Specify a minimum version, allowing the backend to select the highest available:

```toml
vlnv = "xilinx.com:ip:clk_wiz:>=6.0"        # 6.0 or higher
```

The resolved VLNV version is recorded in the build report, so toolchain upgrades are auditable.

**`loom ip upgrade`.** When upgrading the workspace tool version, `loom ip upgrade` scans all vendor IP generators (`vivado_ip`, `quartus_ip`, etc.), queries the new tool's IP catalog, and reports which IP versions need updating. With `--apply`, it rewrites the TOML files. The example below shows Vivado output, but the command works for any backend with an IP generator plugin:

```
$ loom ip upgrade --tool-version 2024.1
  sys_clk: xilinx.com:ip:clk_wiz:6.0 → xilinx.com:ip:clk_wiz:6.1 (update available)
  pcie_ep: xilinx.com:ip:pcie4_uscale_plus:1.3 → 1.3 (unchanged)
  
  Run "loom ip upgrade --apply" to update component.toml files.
```

**Property validation on upgrade.** Bumping IP versions is only half the problem. New IP versions often rename properties, change valid ranges, or remove configuration options entirely. A version bump that passes resolution can still fail deep in the vendor tool with opaque errors.

`loom ip upgrade --check-properties` goes further: for each IP generator, it queries the new IP version's parameter set and validates that every property name in the TOML config exists in the new version. Results are reported as actionable diagnostics:

```
$ loom ip upgrade --tool-version 2024.1 --check-properties
  sys_clk (clk_wiz 6.0 → 6.1):
    ✓ PRIM_IN_FREQ: valid
    ✗ CLKOUT1_REQUESTED_OUT_FREQ: renamed to CLKOUT1_OUT_FREQ in 6.1
    ✓ USE_LOCKED: valid
  
  mig_core (mig_7series 4.2 → 4.3):
    ✗ DQ_WIDTH: removed in 4.3 (now auto-derived from BANK_GROUP_WIDTH)
    ✓ ADDR_WIDTH: valid

  2 IPs have property issues. Fix before applying.
```

This catches breaking changes before they reach the vendor tool, turning an opaque synthesis failure into a pre-build diagnostic with clear remediation. The property check queries the vendor tool's IP metadata (Vivado: `list_property`; Quartus: IP parameter introspection), so it requires a working installation of the target tool version.

**Escape hatch for complex IP:** Some IP (MIG, PCIe, MRMAC) has deeply nested configuration that doesn't map well to flat key-value pairs. For these cases, the generator supports a Tcl fragment that runs in the IP customization context:

```toml
[[generators]]
name = "pcie_ep"
plugin = "vivado_ip"
[generators.config]
vlnv = "xilinx.com:ip:pcie4_uscale_plus"
tcl_config = "ip/pcie_config.tcl"    # arbitrary Tcl for complex setup
```

### 6.6 The Script Escape Hatch Problem [Phase 2]

Vendor tool workflows often involve scripts (Vivado Tcl, Quartus Tcl, yosys commands) that dynamically add source files, modify properties, or perform other actions that are opaque to the build system. This is a fundamental tension: the framework wants a static, declarative view of the file-set, but tool scripts can mutate it at runtime. This is most common in Vivado workflows where teams have accumulated legacy Tcl scripts over years, but applies to any backend with a scripting interface.

**Approach:** Treat dynamic scripts as a special generator type with explicitly declared side effects.

```toml
[[generators]]
name = "legacy_ip_setup"
plugin = "tcl_fragment"
script = "scripts/setup_legacy_ip.tcl"
inputs = ["scripts/setup_legacy_ip.tcl", "ip/legacy/*.vhd"]
outputs_unknown = true            # framework cannot verify outputs
fileset = "synth"
cacheable = false                 # must re-run every build
```

When `outputs_unknown = true`, the framework:

- Runs the Tcl fragment during the backend's build script generation phase (not during the core's DAG execution)
- Cannot cache or skip the generator
- **Forces re-execution of all downstream DAG nodes.** Because the framework cannot know what files the Tcl fragment produced or modified, it must conservatively assume the file-set has changed. This means every `outputs_unknown` generator invalidates the file-set assembly and backend build cache for every build. In practice, this can silently destroy incremental build performance — a project with a single `outputs_unknown` generator will never get a cache hit on its backend build.
- Emits a warning on every build: `Warning: Generator "legacy_ip_setup" has outputs_unknown=true. Incremental builds are disabled for this project. Run "loom migrate tcl-audit" for migration guidance.`

**Cache invalidation is the key consequence.** `outputs_unknown = true` disables incremental builds for the entire project — the cost is proportional to build time. This is by design: it makes the escape hatch's cost visible and creates migration pressure.

#### 6.6.1 Migration Helpers [Phase 2 Vivado, Phase 4 Quartus]

Loom provides tooling to help users migrate from existing vendor project files to declarative generators. The initial migration tools target Vivado (the most common migration path), with Quartus migration tools planned for Phase 4.

**Vivado migration tools:**

`loom migrate tcl-audit <script>` — Runs the Tcl script in a sandboxed Vivado session, instruments file-system access and Vivado API calls, and produces a report of what files were read, written, and what Vivado commands were executed. This report suggests a declarative generator configuration that would replace the script.

`loom migrate tcl-wrap <script>` — Generates a wrapper generator configuration that runs the Tcl script but with explicit `inputs` and `outputs` declarations based on the audit results. The user reviews and commits the declaration, removing the `outputs_unknown` flag.

`loom migrate xci-to-toml <file.xci>` — Reads an existing `.xci` IP configuration file, extracts the non-default property values, and generates the equivalent `vivado_ip` generator TOML block. This is critical for migration: teams with dozens of manually-tuned IPs cannot be expected to manually re-enter every configuration. The tool opens the `.xci` (which is XML), identifies the IP VLNV, diffs the configured properties against the IP's defaults (querying Vivado's IP catalog), and emits only the non-default settings:

```
$ loom migrate xci-to-toml ip/clk_wiz_0.xci
# Generated from ip/clk_wiz_0.xci

[[generators]]
name = "clk_wiz_0"
plugin = "vivado_ip"
[generators.config]
vlnv = "xilinx.com:ip:clk_wiz"          # floating version (was 6.0)
properties = {
    PRIM_IN_FREQ = "200.000",
    CLKOUT1_REQUESTED_OUT_FREQ = "100.000",
    CLKOUT2_REQUESTED_OUT_FREQ = "50.000",
    CLKOUT2_USED = "true",
    USE_LOCKED = "true",
}
```

For complex IPs (MIG, PCIe) where the property set is large or includes nested configuration, the tool falls back to extracting the full Tcl customization commands and emitting a `tcl_config` reference instead. The user can then simplify manually.

`loom migrate xci-to-toml --batch ip/` scans a directory for all `.xci` files and generates TOML blocks for each, producing a ready-to-paste configuration for the project manifest.

These are best-effort tools — complex Tcl with conditional logic may not be fully analyzable, and heavily customized IPs may require manual review — but they lower the barrier to migration significantly.

**Quartus migration tools (Phase 4):**

`loom migrate qsf-to-toml <project.qsf>` — Parses a Quartus project's QSF settings file and extracts pin assignments, constraints, IP references, and compilation settings into a Loom platform and project manifest. QSF files are flat key-value settings (one per line), making them easier to parse than Vivado's XML formats.

`loom migrate ip-to-toml <file.ip>` — Reads a Quartus IP parameter file (`.ip`) and generates the equivalent `quartus_ip` generator TOML block, analogous to `xci-to-toml` for Vivado.

This is explicitly a **compatibility mechanism**, not a recommended workflow. The framework should emit guidance suggesting migration to declarative configuration when it encounters `outputs_unknown = true`.

---

## 7. Build DAG and Execution [Phase 1 skeleton, Phase 2+ features]

### 7.1 Build Phases [Phase 1]

A full `loom build` proceeds through the following phases in order. Each phase has a well-defined input and output, and the framework logs phase transitions clearly in the build output.

```
Phase 1: RESOLVE
  Manifest parsing, dependency resolution, lockfile validation
  Output: ResolvedProject (complete dependency graph with all manifests merged)

Phase 2: GENERATE
  Run all generators in topological order (parallel where independent)
  Generators form a DAG based on input/output overlap or depends_on
  Output: All generated files materialized in build directory

Phase 3: ASSEMBLE
  Collect all static + generated files, apply constraint scoping,
  preprocess constraint templates, produce ordered file-set
  Output: AssembledFilesets (flat, ordered list of everything the backend needs)

Phase 4: VALIDATE
  Pre-flight checks before invoking the vendor tool (see §7.4)
  Output: Pass/fail with diagnostics

Phase 5: BUILD
  Execute the build DAG: OOC component builds → top-level build (see §7.2)
  Output: Build artifacts + checkpoints at each sub-phase

Phase 6: EXTRACT
  Query vendor tool for metrics, parse logs, produce structured report
  Output: BuildReport (JSON)

Phase 7: REPORT
  Format and emit the build report via reporter plugins, run post-build hooks
  Output: Formatted reports, hook side-effects
```

Phases 1-4 are fast (seconds). Phase 5 dominates wall-clock time (minutes to hours). The framework makes this phase structure visible to the user — the CLI shows which phase is active, and errors are attributed to the phase they occurred in.

### 7.2 The Build DAG — Phase 5 Detail [Phase 1 linear, Phase 3 OOC DAG]

Phase 5 is not a linear pipeline — it is a **directed acyclic graph of build nodes**. Each node is an invocation of the backend plugin: either an OOC component synthesis or a sub-phase of the top-level build. Edges represent data dependencies (a top-level synthesis that loads OOC checkpoints depends on those checkpoints being built first).

The framework defines **generic sub-phase names** that all backends map to:

| Generic Sub-Phase | Vivado | Quartus Prime | yosys + nextpnr |
|---|---|---|---|
| `synthesis` | `synth_design` | Analysis & Synthesis | `yosys -p "synth_*"` |
| `optimize` | `opt_design` | (part of Fitter) | (N/A) |
| `place` | `place_design` | Fitter (placement) | `nextpnr --place` |
| `route` | `route_design` | Fitter (routing) | `nextpnr --route` |
| `phys_optimize` | `phys_opt_design` | (N/A) | (N/A) |
| `bitstream` | `write_bitstream` | Assembler | `icepack` / `ecppack` |

Not every backend supports every sub-phase. The backend plugin declares which sub-phases it implements. The `optimize` and `phys_optimize` steps are Vivado-specific; Quartus folds optimization into its Fitter; yosys/nextpnr has no separate optimization pass. The `--stop-after` and `--start-at` CLI flags use the generic names, and the backend maps them to its tool-specific operations.

For a project with no OOC components, the build DAG degenerates to a simple linear chain:

```
synthesis → optimize → place → route → phys_optimize → bitstream
```

For a project with OOC components, the DAG has independent OOC nodes that feed into the top-level synthesis:

```
OOC: acmecorp/ddr4_controller ─────┐
OOC: acmecorp/pcie_endpoint    ─────┤
OOC: acmecorp/dsp_core         ─────┤
                                     ▼
                            top-level synthesis
                                     │
                                 optimize
                                     │
                                   place
                                     │
                                   route
                                     │
                              phys_optimize
                                     │
                                 bitstream
```

OOC nodes with no mutual dependencies execute in parallel (respecting `-j N`). The top-level synthesis begins only after all its OOC dependencies have produced checkpoints. Each node in the DAG — whether OOC or top-level — produces a checkpoint and reports completion to the framework for resume support (see §7.5).

This DAG model generalizes naturally. Partial reconfiguration (§17) adds further nodes: the base static build feeds into per-module implementation passes, each producing a partial bitstream. The framework doesn't need special-case logic for each flow — it executes whatever DAG the backend plugin constructs.

#### 7.2.1 Out-of-Context (OOC) Synthesis [Phase 3]

In large designs, compiling everything from source in a single monolithic synthesis run is too slow. **Out-of-context (OOC) synthesis** allows independent components to be synthesized into their own checkpoints before top-level synthesis. If a component hasn't changed, the backend reuses its cached OOC checkpoint instead of re-synthesizing the source — dramatically reducing iteration time. The concept exists across vendors: Vivado calls it OOC synthesis (producing `.dcp` checkpoints), Quartus Pro calls it design partitions (producing `.qdb` files). Backends that don't support OOC (yosys/nextpnr, Quartus Standard) simply synthesize everything in-context.

A component enables OOC synthesis in its manifest:

```toml
[component]
name = "acmecorp/ddr4_controller"
version = "2.0.0"

[synth]
ooc = true
ooc_top = "ddr4_ctrl_wrapper"    # synthesis top module within the component
```

When Loom encounters OOC-enabled components, it creates per-component build nodes in the Phase 5 DAG:

1. Assemble the component's own file-set (its synth files + its transitive dependencies' synth files)
2. Run the backend's OOC synthesis flow for that component (for Vivado: `synth_design -mode out_of_context`; for Quartus: design partition in incremental compilation)
3. Produce a checkpoint cached by the component's file-set hash + part + tool version

The top-level synthesis node in the DAG references these OOC checkpoints instead of compiling the component sources from scratch. The specific mechanism is backend-dependent (Vivado: `read_checkpoint` loading a `.dcp`; Quartus: partition `.qdb` import).

**OOC cache keys:** component file-set hash + transitive dependency hashes + target part + tool version + OOC-specific synthesis options. This is more granular than the top-level build cache key, which is the point — changing a top-level source file doesn't invalidate OOC checkpoints for unrelated components.

**Parallel OOC builds.** OOC synthesis for independent components is embarrassingly parallel. Loom runs them concurrently (respecting `-j N`), potentially on multiple machines in CI.

**When not to use OOC.** OOC synthesis adds overhead for small components and can limit cross-boundary optimization. The framework defaults `ooc = false` — it's opt-in for components large enough to benefit. A rule of thumb: components taking >60 seconds to synthesize in-context are candidates. Note: OOC synthesis is primarily supported by Vivado and Quartus Pro. Quartus Standard has limited partition support. yosys/nextpnr does not currently support OOC flows — the framework will skip OOC for backends that don't implement it and synthesize those components in-context.

**Edge cases in OOC dependency graphs:**

*Transitive OOC dependencies.* If component A (`ooc = true`) depends on component B (`ooc = true`), the OOC builds are layered: B is OOC-synthesized first, producing B's checkpoint. A's OOC synthesis then loads B's checkpoint (not B's sources) and synthesizes only A's own source files. At top-level synthesis, both A's and B's checkpoints are loaded. The OOC DAG mirrors the dependency graph — OOC builds execute in topological order.

*Mixed OOC/non-OOC dependencies.* If component A (`ooc = true`) depends on component B (no OOC), B's source files are compiled inline during A's OOC synthesis. This means changes to B invalidate A's OOC cache. This is correct behavior — A's OOC checkpoint must reflect B's current state. The cache key for A includes the hash of B's source files.

*Cross-boundary optimization.* OOC synthesis inserts a hard optimization boundary. The synthesis tool cannot optimize logic across the boundary between an OOC checkpoint and its consumer. For components where cross-boundary optimization is important (e.g., small glue logic, or components whose outputs feed directly into timing-critical paths), OOC may degrade quality of results. The framework emits a diagnostic when OOC is enabled on a component with fewer than a configurable threshold of source files (default: 500 lines), suggesting that the component may be too small to benefit.

#### 7.2.2 Build Sub-Phase Control [Phase 2]

The default `loom build` runs the full pipeline through bitstream generation. For debugging and iterative development, users often need finer control:

**Stopping early.** `--stop-after <sub-phase>` runs the pipeline up to and including the specified sub-phase, then exits successfully:

```
loom build --stop-after synthesis       # check RTL quality, utilization estimate
loom build --stop-after place           # inspect placement before routing
```

**Starting from a checkpoint.** `--start-at <sub-phase>` loads the most recent checkpoint and runs from the specified sub-phase. This combines naturally with `--resume`:

```
loom build --start-at route             # re-route using existing placement
loom build --start-at phys_optimize     # run another physical optimization iteration
```

**Combining controls.** These compose predictably:

```
loom build --start-at optimize --stop-after place   # just optimize + place
loom build --resume --stop-after synthesis           # resume, but stop after synth
```

The CLI uses the generic sub-phase names. The backend maps them to the tool's actual commands (e.g., `place` becomes `place_design` for Vivado, part of the Fitter for Quartus). If a sub-phase doesn't exist for the active backend (e.g., `phys_optimize` on yosys), the framework skips it.

The `--start-at` flag requires a valid checkpoint from a prior run. If no checkpoint exists, the build fails with a message indicating which checkpoint is needed. The `--stop-after` flag still produces a checkpoint at the stopping point, so subsequent runs can pick up from there.

### 7.3 Caching and Incrementality [Phase 2]

Each phase has a **cache key** computed from its inputs. If the cache key matches a stored result, the phase is skipped.

- **Generator cache keys:** hash of input file contents + command/config + plugin version. For vendor-tool generators (e.g., `vivado_ip`, `quartus_ip`), the cache key **must also include** the tool version and target part, since IP output products change across tool versions even for identical configurations. A tool version upgrade must invalidate all IP caches.
- **File-set assembly cache key:** hash of all assembled files, constraint order, and template parameter values.
- **Backend build cache key:** file-set hash + target part + tool version + build strategy + platform parameters.
- **Invalidation by `outputs_unknown`:** If any generator in the DAG has `outputs_unknown = true`, all downstream phases (ASSEMBLE, VALIDATE, BUILD) are unconditionally invalidated. See §6.6 for details.

The framework stores cache metadata in the build directory (`.build/`). This directory is gitignored. CI systems can persist it as a cache artifact for cross-run incrementality.

#### 7.3.1 Incremental Build Integration [Phase 5]

Vendor tool incrementality is orthogonal to framework-level caching but represents a significant optimization opportunity. When Loom's cache key doesn't match (sources changed), the framework must invoke the vendor tool. But it can still accelerate the build by providing a **reference checkpoint** from a previous run.

This is a backend capability — not all backends support it. The `BackendCapabilities.supports_incremental` flag (§10.6) declares whether the backend can use reference checkpoints. When supported, the framework automatically provides them.

**Vivado incremental synthesis.** Vivado's `synth_design -incremental` and `place_design -incremental` reuse unchanged logic from a reference `.dcp`, resynthesizing or re-placing only what changed. This can reduce synthesis time by 50-80% for small edits.

**Quartus incremental compilation.** Quartus Pro's incremental compilation uses design partitions (`.qdb` snapshots) to preserve unchanged regions. Quartus Standard has a simpler "rapid recompile" mode. The Quartus backend maps the framework's incremental interface to the appropriate mode based on edition.

**yosys/nextpnr.** No incremental support currently. The framework falls back to a clean build. If future yosys versions add incremental synthesis, the backend can adopt it without framework changes.

**Reference checkpoint selection.** The backend selects a reference checkpoint using the following priority:

1. **Explicit reference.** If the user passes `--reference <path>`, use that checkpoint.
2. **Previous successful build.** The most recent successful checkpoint for the same project + strategy + part combination. The backend records successful checkpoints in the build state file (§7.5).
3. **Previous failed build.** If the last build failed after synthesis (e.g., during routing), the post-synthesis checkpoint is still a valid reference for incremental synthesis.
4. **No reference.** If no suitable checkpoint exists, run a full (non-incremental) build.

**Cache interaction.** The framework-level cache and the vendor's incremental mode serve different purposes:

- **Framework cache hit** (cache key matches): Skip the vendor tool entirely. Zero tool invocations.
- **Framework cache miss + reference checkpoint available**: Invoke the tool in incremental mode. Faster than a clean build. This is the common case during iterative development.
- **Framework cache miss + no reference**: Full clean build. This happens on a fresh checkout or after major changes.

The backend records which checkpoints are available and their associated cache keys, so it can assess reference quality.

**Configuration.** Incremental builds are enabled by default on backends that support them. Projects can disable per-strategy:

```toml
[build.strategies.clean_build]
incremental = false    # force full synthesis/implementation
```

### 7.4 Pre-Build Validation [Phase 1]

Phase 4 (VALIDATE) runs a series of checks before invoking the vendor tool. This is the "fail fast" mechanism — catching errors here saves minutes to hours compared to catching them deep in synthesis or implementation.

**Built-in validations:**

- **File existence.** Every file in the assembled file-set actually exists on disk.
- **File type consistency.** Files have extensions matching their declared type (`.sv` for SystemVerilog, `.xdc`/`.sdc`/`.lpf`/`.pcf` for constraints, etc.). Files are not empty.
- **Dependency completeness.** All declared dependencies resolved successfully and are present in the lockfile.
- **Tool environment.** The required vendor tool version is installed, licensed, and matches the project requirement.
- **Constraint sanity.** Template-expanded constraint files are well-formed (basic syntax check, not full tool parsing).
- **Platform consistency.** If targeting a platform, the platform's part matches the backend's expectations. Clock frequencies referenced in constraint templates have corresponding platform definitions.
- **Profile validity.** If a profile is active, all overlay operations (add_files, add_constraints) reference files that exist.

**Backend-specific validations:** The backend plugin provides additional checks. For Vivado: verifying IP VLNVs are valid, part support, Tcl syntax checking. For Quartus: validating IP parameter files and device family support. For yosys: verifying the target architecture is supported by nextpnr.

**Custom validations via hooks.** The `pre_build` hook runs after built-in validation, allowing users to add project-specific checks (e.g., verifying that a register map is in sync with firmware headers).

Validation results are reported as a diagnostic list with severity (error, warning, info) and precise source location. Any error-severity diagnostic halts the build before Phase 5 begins.

### 7.5 Error Recovery, Checkpoints, and Resume [Phase 2]

Loom tracks build sub-phase completion via checkpoint files and supports resumption after failure.

**Checkpoint tracking.** The backend reports sub-phase completion. Each backend produces checkpoints in its own format (Vivado: `.dcp`, Quartus: `.qdb`, yosys/nextpnr: JSON netlist). The framework records status in `.build/<project>/<strategy>/build_state.json`:

```json
{
    "cache_key": "abc123...",
    "backend": "vivado",
    "phases_completed": ["synthesis", "optimize", "place"],
    "phases_failed": ["route"],
    "checkpoints": {
        "synthesis": ".build/radar_processor/default/post_synth.dcp",
        "optimize": ".build/radar_processor/default/post_opt.dcp",
        "place": ".build/radar_processor/default/post_place.dcp"
    },
    "failure": {
        "phase": "route",
        "exit_code": 1,
        "log": ".build/radar_processor/default/route.log",
        "summary": "Routing failed: 142 unroutable nets"
    }
}
```

**Resume behavior.** `loom build --resume` checks the build state file:

1. If the cache key matches (sources haven't changed since the failed build), resume from the last successful sub-phase. The backend loads its checkpoint format and continues from the next sub-phase.
2. If the cache key doesn't match (sources changed), the resume point may be invalid. The framework warns and falls back to a full rebuild. However, some backends can use a previous checkpoint as an incremental reference even when sources change (see §7.3.1 for Vivado's incremental synthesis).
3. If `--resume` is passed but no build state exists, the framework runs a normal build.

**Retry with different strategies.** A common workflow is: route fails with default strategy → retry with a different seed or more aggressive optimization. `loom build --resume --strategy aggressive` loads the last successful checkpoint and re-runs from that point using the new strategy. This is a natural fit because synthesis results are often reusable across implementation strategies.

**Non-Phase-5 failures.** If the build fails in Phase 1-4 (resolution, generation, assembly, validation), there is nothing to resume — these phases are fast and should be re-run. `--resume` only applies to Phase 5 (BUILD).

### 7.6 Dry Run [Phase 2]

`loom build --dry-run` executes Phases 1-4 (RESOLVE, GENERATE, ASSEMBLE, VALIDATE) and then prints the Phase 5 execution plan without invoking the vendor tool. The output includes:

- The resolved dependency graph
- Which generators would run (and which are cached)
- The complete, ordered file-set that would be passed to the backend
- The generated Tcl scripts (or equivalent backend commands) that would be executed
- Which build sub-phases would run (or be skipped via cache/resume)
- Estimated resource requirements (if the backend can provide them)

This allows users to inspect and verify the build plan before committing to a multi-hour vendor tool run. It's also useful for debugging manifest issues — you can see exactly what Loom assembled without waiting for synthesis.

```
$ loom build --dry-run
  ✓ Phase 1: RESOLVE (12 components, 3 generators)
  ✓ Phase 2: GENERATE
      ✓ regmap (cached)
      ✓ sys_pll (would run — config changed)
  ✓ Phase 3: ASSEMBLE (47 source files, 8 constraint files)
  ✓ Phase 4: VALIDATE (0 errors, 2 warnings)
  
  Phase 5: BUILD (would execute)
    Target:   xczu7ev-ffvc1156-2-e (platform: zcu104)
    Strategy: default
    Backend:  vivado 2023.2
    Steps:    synthesis → optimize → place → phys_optimize → route → bitstream
    
    Generated scripts: .build/radar_processor/default/build.tcl
    
  Dry run complete. Use "loom build" to execute.
```

### 7.7 Parallel Execution [Phase 2]

Generator nodes with no mutual dependencies can execute in parallel. The framework supports a configurable parallelism limit (`--jobs N` or `-j N`). Backend build stages are inherently sequential (synthesis before implementation), but multi-strategy sweeps (see §8.2), multi-profile builds, and OOC component builds are embarrassingly parallel across their respective axes.

#### 7.7.1 Plugin Execution Model and the Python GIL [Phase 2]

**Problem.** Python's GIL prevents parallel generator execution within a single process.

**Solution: subprocess isolation.** Generators execute as separate processes, not in-process PyO3 calls:

1. Rust core spawns a Python subprocess per generator (or reuses a warm pool).
2. Each generator runs in its own process with its own GIL — no contention.
3. IPC is JSON on stdin/stdout. Diagnostics on stderr.
4. Generators that shell out to tools (Vivado, scripts) are thin wrappers around subprocess calls.

**PyO3 is still used** for plugin discovery, config validation, and lightweight sync calls (`validate()`, `outputs()`). These are brief single-threaded operations. The key distinction: **metadata queries use in-process PyO3; execution uses subprocess isolation.**

**Backend plugins** don't have the GIL problem — only one backend invocation at a time. The `execute_build()` method runs via PyO3 but immediately shells out to the vendor tool. The GIL is released during the blocking `subprocess.wait()`.

**IPC overhead** target: <100ms per invocation (JSON + process spawn). Negligible vs. generator runtime. Batch mode available for many trivially fast generators.

---

## 8. Build Configuration [Phase 1 basic, Phase 5 sweeps]

### 8.1 Basic Configuration

```toml
[build]
build_dir = ".build"              # relative to project root
default_strategy = "default"

[build.synth]
# Synthesis-level options passed to the backend
# Vivado backend interprets these as strategy/directive settings

[build.impl]
# Implementation-level options

[build.bitstream]
# Bitstream generation options
```

The build section is interpreted by the backend plugin. The core passes it through without validation — the backend owns the schema for tool-specific options.

### 8.2 Strategy Sweeps [Phase 5]

For timing closure, multiple implementation strategies can run in parallel:

```toml
[build.strategies.default]
# Use tool defaults

[build.strategies.aggressive]
synth_directive = "PerformanceOptimized"
impl_directive = "ExtraTimingOpt"

[build.strategies.area]
synth_directive = "AreaOptimized_high"
impl_directive = "Default"
```

The `--sweep` CLI flag runs all declared strategies in parallel. The framework selects the first result that meets timing constraints, or reports all failures. The selection logic is part of the backend plugin (since "meets timing" is tool-specific).

---

## 9. Build Metrics and Reporting [Phase 2 metrics, Phase 4 reporters]

### 9.1 Metrics Data Model

Metrics are **nested dictionaries** with natural sub-structure (timing per clock domain, utilization per module). The framework defines a standard hierarchy; backends populate what they can extract. Every level is optional.

**Standard metrics hierarchy:**

```
timing/
├── summary/
│   ├── wns                    # float: worst negative slack (ns), overall
│   ├── tns                    # float: total negative slack (ns), overall
│   ├── whs                    # float: worst hold slack (ns), overall
│   ├── ths                    # float: total hold slack (ns), overall
│   └── failing_endpoints      # int: total failing endpoints
├── clocks/
│   ├── <clock_name>/          # per-clock-domain breakdown
│   │   ├── period             # float: clock period (ns)
│   │   ├── frequency_mhz     # float: frequency (MHz)
│   │   ├── wns                # float: WNS for this domain
│   │   ├── tns                # float: TNS for this domain
│   │   └── failing_endpoints  # int: failing endpoints in this domain
│   └── .../
└── inter_clock/               # optional: inter-clock path summary
    └── <src_clk>_to_<dst_clk>/
        └── wns

utilization/
├── summary/
│   ├── lut                    # float: percentage
│   ├── ff                     # float: percentage
│   ├── bram                   # float: percentage
│   ├── dsp                    # float: percentage
│   └── uram                   # float: percentage
├── absolute/                  # raw counts
│   ├── lut_used               # int
│   ├── lut_available          # int
│   ├── ff_used                # int
│   └── .../
└── by_module/                 # optional: per-module breakdown
    └── <module_name>/
        ├── lut                # int: count
        ├── ff                 # int: count
        └── .../

power/
├── total                      # float: watts
├── dynamic                    # float: watts
├── static                     # float: watts
└── by_rail/                   # optional: per-rail breakdown
    └── <rail_name>/
        └── power              # float: watts

build/
├── synth_duration             # float: seconds
├── impl_duration              # float: seconds
├── total_duration             # float: seconds
├── peak_memory_mb             # float: peak memory usage
├── warnings_synth             # int
├── warnings_impl              # int
└── drc_violations             # int
```

Backends can extend the hierarchy with arbitrary additional data. The framework's reporters and diff tools understand the tree structure and can compare at any level.

### 9.2 Metrics Query Interface

The backend's `extract_metrics` method receives a list of metric paths to populate. Paths use dot notation: `"timing.summary"`, `"timing.clocks"`, `"utilization.by_module"`. The special path `"*"` requests everything the backend can provide.

```python
# Request specific metrics
metrics = backend.extract_metrics(build_result, [
    "timing.summary",
    "timing.clocks",
    "utilization.summary",
    "power",
    "build",
])

# Request everything
metrics = backend.extract_metrics(build_result, ["*"])
```

This allows fast default builds that extract only summary metrics, with the option to request detailed breakdowns when needed (e.g., for regression investigation or detailed reports).

### 9.3 Metrics Extraction in the Vivado Backend

Two extraction methods:

1. **Tcl query phase.** After implementation, run a Tcl script (same or new Vivado session) executing `report_utilization -return_string`, `get_property SLACK [get_timing_paths]`, etc., writing structured JSON output.
2. **Log parsing (fallback).** Parse log files with structured parsers for data not available via Tcl queries.

### 9.4 Report Output

JSON build report — the primary CI integration interface:

```json
{
    "project": "radar_processor",
    "profile": null,
    "timestamp": "2026-03-03T14:22:00Z",
    "tool": { "name": "vivado", "version": "2023.2" },
    "platform": "zcu104",
    "target": { "part": "xczu7ev-ffvc1156-2-e" },
    "strategy": "aggressive",
    "status": "pass",
    "git": { "commit": "abc123f", "branch": "main", "dirty": false },
    "metrics": {
        "timing": {
            "summary": { "wns": 0.142, "tns": 0.0, "whs": 0.021, "ths": 0.0, "failing_endpoints": 0 },
            "clocks": {
                "sys_clk_125": { "period": 8.0, "frequency_mhz": 125.0, "wns": 0.412, "tns": 0.0 },
                "axi_clk_250": { "period": 4.0, "frequency_mhz": 250.0, "wns": 0.142, "tns": 0.0 },
                "dsp_clk_500": { "period": 2.0, "frequency_mhz": 500.0, "wns": 0.203, "tns": 0.0 }
            }
        },
        "utilization": {
            "summary": { "lut": 42.3, "ff": 28.1, "bram": 65.0, "dsp": 12.5, "uram": 0.0 },
            "absolute": { "lut_used": 123456, "lut_available": 291840, "ff_used": 98765, "ff_available": 583680 }
        },
        "power": { "total": 8.2, "dynamic": 6.1, "static": 2.1 },
        "build": { "synth_duration": 423.0, "impl_duration": 1847.0, "total_duration": 2305.0, "peak_memory_mb": 16384.0 }
    }
}
```

### 9.5 Reporter Plugins [Phase 4]

Reporter plugins consume the JSON build report and produce formatted output for specific consumers:

| Reporter | Output |
|---|---|
| `console` (built-in) | Human-readable terminal summary. |
| `json` (built-in) | The raw JSON report (default). |
| `github_actions` | GitHub Actions annotations and job summary. |
| `junit` | JUnit XML for CI systems that consume it. |
| `csv_append` | Append metrics to a CSV for time-series tracking. |
| `html_dashboard` | Standalone HTML report with charts. |

---

## 10. Plugin System [Phase 1 interfaces, Phase 2 loading]

### 10.1 Plugin Types

| Type | Role | Interface |
|---|---|---|
| **Generator** | Executes a code generation step | `validate`, `cache_key`, `execute`, `clean` |
| **Backend** | Drives a vendor FPGA tool for synthesis/implementation | `check_env`, `generate_scripts`, `execute_build`, `extract_metrics` |
| **Simulator** | Drives a simulation tool (Questa, VCS, Verilator, etc.) | `check_env`, `compile`, `elaborate`, `simulate`, `extract_results` |
| **Reporter** | Formats build reports for a specific consumer | `format_report` |
| **Hook** | User-defined scripts at lifecycle points | (not a code plugin — declared in manifest) |

### 10.2 Plugin Discovery and Loading

Plugins are authored in Python and loaded by the Rust core via an embedded Python interpreter (PyO3). Discovery:

1. **Built-in plugins.** Compiled into the `loom` binary for core functionality (e.g., `command` generator, `json` reporter). These are Rust implementations, not Python.
2. **Installed Python packages.** Python packages installed into Loom's managed plugin environment, using standard entry points. `pip install loom-vivado-backend` (into `loom`'s plugin venv) makes the Vivado backend available. Loom manages this environment separately from any system Python.
3. **Workspace-local plugins.** Python modules in a `plugins/` directory within the workspace, loaded by convention.
4. **Manifest declaration.** The workspace or project manifest can explicitly reference plugin paths.

```toml
# workspace.toml
[plugins]
# Explicitly declare local plugins
my_custom_generator = { path = "tools/plugins/my_generator.py" }
```

### 10.3 Plugin Interfaces

Plugin interfaces are defined as Python abstract base classes. The Rust core calls into these via PyO3. Plugin authors work entirely in Python — they `import loom.plugin` and subclass the appropriate base class.

#### 10.3.1 Generator Plugin Interface

```python
class GeneratorPlugin(ABC):
    """Base class for generator plugins."""

    @property
    @abstractmethod
    def plugin_name(self) -> str:
        """The identifier used in manifest `plugin = "..."` fields."""
        ...

    @abstractmethod
    def validate_config(self, config: GeneratorConfig) -> list[Diagnostic]:
        """Validate generator configuration.
        Return diagnostics (errors, warnings). Empty list = valid."""
        ...

    @abstractmethod
    def compute_cache_key(self, config: GeneratorConfig,
                          input_hashes: dict[str, str]) -> str:
        """Compute a cache key incorporating tool-specific state
        (e.g., tool version). Input file hashes are provided by
        the framework."""
        ...

    @abstractmethod
    def execute(self, config: GeneratorConfig,
                context: BuildContext) -> GeneratorResult:
        """Run the generator.
        Returns: produced file paths, logs, success/failure."""
        ...

    @abstractmethod
    def clean(self, config: GeneratorConfig,
              context: BuildContext) -> None:
        """Remove generated artifacts."""
        ...
```

#### 10.3.2 Backend Plugin Interface

```python
class BackendPlugin(ABC):
    """Base class for FPGA tool backends."""

    @property
    @abstractmethod
    def plugin_name(self) -> str:
        """The identifier used in manifest `backend = "..."` fields."""
        ...

    @abstractmethod
    def check_environment(self, required_version: str) -> EnvironmentStatus:
        """Verify tool installation, version, and licensing.
        Returns detailed status including path, actual version,
        license status, and any mismatches."""
        ...

    @abstractmethod
    def validate(self, project: ResolvedProject,
                 filesets: AssembledFilesets,
                 context: BuildContext) -> list[Diagnostic]:
        """Run backend-specific pre-build validation (Phase 4).
        Check part support, IP validity, script syntax, etc.
        Returns diagnostics — errors halt the build before Phase 5."""
        ...

    @abstractmethod
    def generate_build_scripts(self, project: ResolvedProject,
                               options: BuildOptions) -> list[Path]:
        """Produce the tool-specific scripts needed for the build.
        For Vivado: Tcl scripts. For Quartus: Tcl/QSF settings.
        For yosys: yosys script + nextpnr command.
        These scripts are inspectable artifacts for debugging."""
        ...

    @abstractmethod
    def execute_build(self, scripts: list[Path],
                      options: BuildOptions,
                      phase_callback: PhaseCallback) -> BuildResult:
        """Run the build. Manages tool invocation, log capture,
        and success/failure determination.
        
        Must call phase_callback.on_phase_complete(phase_name, checkpoint_path)
        after each sub-phase (synthesis, place, route, etc.)
        so the framework can track progress for resume support.
        
        Returns: status, log paths, checkpoint paths, phases completed."""
        ...

    @abstractmethod
    def resume_build(self, checkpoint: Path,
                     from_phase: str,
                     options: BuildOptions,
                     phase_callback: PhaseCallback) -> BuildResult:
        """Resume a build from a previously saved checkpoint.
        Loads the checkpoint and continues from the specified phase.
        Used by `loom build --resume`. See §7.5."""
        ...

    @abstractmethod
    def extract_metrics(self, build_result: BuildResult,
                        metric_queries: list[str]) -> BuildMetrics:
        """Extract structured metrics from a completed build.
        May re-open the vendor tool to run queries.
        metric_queries: list of metric paths from the standard set.
        Returns: populated BuildMetrics with available values."""
        ...

    @abstractmethod
    def select_strategy_result(self, results: list[BuildResult]) -> BuildResult | None:
        """Given multiple strategy results, select the best passing
        result. Returns None if no result meets constraints.
        Tool-specific because 'meets timing' is tool-specific."""
        ...
```

#### 10.3.3 Simulator Plugin Interface

Simulation is a separate plugin type. Simulators have a different execution model (compile → elaborate → simulate), and **are not interchangeable** — Verilator transpiles SV to C++ (no fork/join, limited interfaces, no UVM), while Questa/VCS support the full SystemVerilog/UVM stack. The interface addresses this via a **capability model**: tests declare requirements, and incompatible simulator/test combinations are skipped, not failed.

```python
@dataclass
class SimulatorCapabilities:
    """Declares what a simulator can and cannot do.
    Used by the framework to match tests to compatible simulators
    and skip incompatible test/simulator combinations gracefully."""

    # Language support
    systemverilog_full: bool     # full IEEE 1800 SV support
    vhdl: bool                   # VHDL compilation
    mixed_language: bool         # SV + VHDL co-simulation

    # Methodology support
    uvm: bool                    # UVM library available
    fork_join: bool              # dynamic process control (fork/join, disable fork)
    force_release: bool          # force/release signal overrides
    bind_statements: bool        # SystemVerilog bind for assertion insertion

    # Coverage support
    code_coverage: bool          # line, toggle, branch, FSM coverage
    functional_coverage: bool    # covergroup/coverpoint
    assertion_coverage: bool     # SVA cover properties

    # Execution model
    compilation_model: str       # "event_driven" | "cycle_accurate" | "formal"
    supports_gui: bool           # interactive waveform debugging
    supports_save_restore: bool  # checkpoint/restore mid-simulation

    # Performance characteristics (informational, not for matching)
    typical_compile_speed: str   # "fast" | "moderate" | "slow"
    typical_sim_speed: str       # "fast" | "moderate" | "slow"


class SimulatorPlugin(ABC):
    """Base class for simulation tool plugins."""

    @property
    @abstractmethod
    def plugin_name(self) -> str:
        """Identifier: 'questa', 'vcs', 'verilator', 'xsim', etc."""
        ...

    @property
    @abstractmethod
    def capabilities(self) -> SimulatorCapabilities:
        """Declare what this simulator supports. Used by the framework
        for test/simulator compatibility matching."""
        ...

    @abstractmethod
    def check_environment(self, required_version: str | None) -> EnvironmentStatus:
        """Verify simulator installation and licensing."""
        ...

    @abstractmethod
    def compile(self, filesets: ResolvedFilesets,
                options: SimOptions, context: BuildContext) -> CompileResult:
        """Compile sources into a simulation library.
        Handles language-specific compilation (VHDL, SV, etc.).
        For Verilator, this transpiles to C++ and compiles with gcc/clang."""
        ...

    @abstractmethod
    def elaborate(self, compile_result: CompileResult,
                  top_module: str, options: SimOptions,
                  context: BuildContext) -> ElaborateResult:
        """Elaborate the design. Some simulators merge this with compile.
        For Verilator, this is typically a no-op (elaboration happens
        during the compile/transpile step)."""
        ...

    @abstractmethod
    def simulate(self, elaborate_result: ElaborateResult,
                 options: SimOptions,
                 context: BuildContext) -> SimResult:
        """Run the simulation. Returns pass/fail, log paths, waveform paths."""
        ...

    @abstractmethod
    def extract_results(self, sim_result: SimResult) -> SimReport:
        """Extract structured results: pass/fail, coverage data,
        assertion counts, simulation time."""
        ...

    @abstractmethod
    def merge_coverage(self, coverage_dbs: list[Path],
                       output: Path) -> CoverageReport:
        """Merge multiple coverage databases from parallel test runs
        into a single merged database. Simulator-specific because
        coverage formats differ radically: UCDB (Questa), VDB (VCS),
        etc. Returns a CoverageReport with line/toggle/branch/FSM
        percentages and the path to the merged database."""
        ...
```

**Test-simulator compatibility.** Test manifests can declare simulator requirements so the framework skips incompatible combinations rather than producing cryptic compilation errors:

```toml
[[tests]]
name = "uvm_scoreboard_test"
top = "radar_top_uvm_tb"
tags = ["regression"]
[tests.requires]
uvm = true                    # needs UVM — won't work on Verilator
fork_join = true              # uses dynamic processes

[[tests]]
name = "basic_datapath"
top = "datapath_tb"
tags = ["smoke"]
# No requirements — compatible with any simulator, including Verilator
```

When `loom sim` runs a test suite, it checks each test's `requires` against the active simulator's `capabilities`. Incompatible tests are skipped with a clear message ("Skipping uvm_scoreboard_test: requires uvm (verilator does not support uvm)") rather than being run and failing opaquely. `loom sim --suite regression --check-compat` reports compatibility without running anything.

#### 10.3.4 Reporter Plugin Interface

```python
class ReporterPlugin(ABC):
    """Base class for report formatters."""

    @property
    @abstractmethod
    def plugin_name(self) -> str: ...

    @abstractmethod
    def format_report(self, report: BuildReport,
                      options: dict) -> ReporterOutput:
        """Transform a BuildReport into formatted output.
        ReporterOutput contains: content (str or bytes),
        suggested filename, and content type."""
        ...
```

### 10.4 BuildContext

The `BuildContext` object is passed to plugins and provides access to framework services without coupling plugins to internal implementation:

```python
@dataclass
class BuildContext:
    project: ResolvedProject       # full resolved project description
    platform: ResolvedPlatform | None  # platform definition, if targeting one
    profile: str | None          # active profile name, if any
    build_dir: Path                # output directory for this build
    workspace_root: Path           # workspace root path
    logger: Logger                 # structured logging
    tool_paths: dict[str, Path]    # discovered tool installations
    cache: CacheService            # read/write cache entries
    env: dict[str, str]            # environment variables
    params: dict[str, Any]         # merged platform + project parameters
```

### 10.5 Hook System [Phase 4]

Hooks are user-defined commands declared in the manifest and executed at defined lifecycle points. They run as subprocesses with a well-defined contract for input and output.

#### 10.5.1 Hook Declaration

Hooks can be declared at workspace or project level. Project hooks override workspace hooks for the same lifecycle point. A hook can be a simple command string, a list of commands (executed sequentially), or an expanded declaration with options:

```toml
# workspace.toml or project.toml
[hooks]
# Simple string form
pre_generate = "tools/hooks/check_prerequisites.sh"

# List form (sequential execution, stops on first failure)
post_generate = ["tools/hooks/validate_generated.py", "tools/hooks/format_generated.sh"]

# Expanded form with timeout and options
[hooks.pre_build]
command = "tools/hooks/check_licenses.sh"
timeout_seconds = 30          # kill the hook after 30s (default: 300)

[hooks.post_build]
command = "tools/hooks/extract_custom_metrics.py"
timeout_seconds = 60

[hooks.post_report]
command = "tools/hooks/upload_to_dashboard.py"
timeout_seconds = 120
allow_failure = true          # build succeeds even if this hook fails
```

**Timeouts.** Default: 300 seconds. Hook killed on timeout, reported as failure. Pre-phase hooks should use 30-60s; post-report may need longer for uploads.

**`allow_failure`.** Logs warning but doesn't halt the build. Only valid for `post_build` and `post_report` — pre-phase hooks always fail the build on error.

#### 10.5.2 Lifecycle Points

Hooks correspond to the build phases defined in §7.1:

| Hook | Runs When | Typical Use |
|---|---|---|
| `pre_generate` | Before Phase 2 (GENERATE) | Check prerequisites, validate input data |
| `post_generate` | After Phase 2, before Phase 3 (ASSEMBLE) | Validate generated files, run additional formatting |
| `pre_build` | After Phase 4 (VALIDATE), before Phase 5 (BUILD) | Notify build start, acquire resources, custom validation |
| `post_build` | After Phase 5 (BUILD), pass or fail | Extract custom metrics, notify completion, archive artifacts |
| `post_report` | After Phase 7 (REPORT) | Upload reports to dashboards, trigger downstream pipelines |

#### 10.5.3 Hook Contract

**Environment variables** set for every hook invocation:

| Variable | Description |
|---|---|
| `LOOM_CONTEXT_FILE` | Path to a JSON file with full build context (see schema below) |
| `LOOM_PROJECT` | Project name |
| `LOOM_PLATFORM` | Platform name (empty if direct part targeting) |
| `LOOM_PROFILE` | Active profile string (empty if base build) |
| `LOOM_BUILD_DIR` | Absolute path to build output directory |
| `LOOM_WORKSPACE_ROOT` | Absolute path to workspace root |
| `LOOM_PHASE` | Current lifecycle point (`pre_generate`, `post_build`, etc.) |
| `LOOM_STATUS` | `pending`, `pass`, or `fail` (meaningful for `post_build` and later) |

**Exit code contract:**

- Exit 0: Hook succeeded. Build continues.
- Exit 1: Hook failed. Build halts with an error attributing the failure to the hook.
- Exit 2: Hook wants to signal a warning but not fail the build. The warning is included in the build report.

**Stdout/stderr:** Captured in build log. Diagnostics to stderr. If stdout is valid JSON, it is merged into the build report under `hooks.<hook_name>` — this is how custom metrics flow into the report.

#### 10.5.4 Context File Schema

The `LOOM_CONTEXT_FILE` is a JSON file. Fields are populated progressively — earlier hooks have less data. The schema:

```json
{
    "loom_version": "0.1.0",
    "phase": "post_build",
    "timestamp": "2026-03-03T14:22:00Z",

    "project": {
        "name": "radar_processor",
        "top_module": "radar_top",
        "manifest_path": "/repo/projects/radar_processor/project.toml"
    },

    "platform": {
        "name": "zcu104",
        "part": "xczu7ev-ffvc1156-2-e",
        "params": { "ddr4_data_width": 64, "pcie_lanes": 4 },
        "clocks": {
            "sys_clk": { "frequency_mhz": 125.0, "period_ns": 8.0 },
            "user_clk": { "frequency_mhz": 300.0, "period_ns": 3.333 }
        }
    },

    "profile": {
        "active": "board=kcu116,tier=reduced",
        "params": { "num_channels": 2 }
    },

    "tool": {
        "backend": "vivado",
        "version": "2023.2",
        "path": "/tools/Xilinx/Vivado/2023.2"
    },

    "build": {
        "strategy": "default",
        "build_dir": "/repo/.build/radar_processor/default",
        "cache_key": "sha256:abc123..."
    },

    "git": {
        "commit": "abc123f",
        "branch": "main",
        "dirty": false
    },

    "generators": {
        "regmap": { "status": "cached", "outputs": ["generated/ctrl_regs.sv"] },
        "sys_pll": { "status": "ran", "outputs": ["generated/sys_pll/"] }
    },

    "filesets": {
        "synth_file_count": 47,
        "constraint_file_count": 8,
        "total_files": 55
    },

    "result": {
        "status": "pass",
        "backend": "vivado",
        "phases_completed": ["synthesis", "optimize", "place", "route", "bitstream"],
        "log_files": {
            "synthesis": ".build/radar_processor/default/synth.log",
            "implementation": ".build/radar_processor/default/impl.log"
        },
        "artifacts": {
            "bitstream": ".build/radar_processor/default/radar_top.bit",
            "checkpoints": { "route": ".build/radar_processor/default/post_route.dcp" }
        }
    },

    "metrics": {
        "timing": { "summary": { "wns": 0.142 } },
        "utilization": { "summary": { "lut": 42.3 } }
    }
}
```

Guaranteed fields per lifecycle point: `pre_generate` sees `project`, `platform`, `profile`, `tool`, `git`. `post_build` sees everything including `result` and `metrics`.

### 10.6 Backend Landscape [Reference]

The framework is designed to support a range of synthesis/implementation backends with different characteristics. This section documents the planned backend support and how each maps to the framework's abstractions.

| Capability | Vivado | Quartus Prime | yosys + nextpnr | Lattice Radiant |
|---|---|---|---|---|
| **Scripting** | Tcl | Tcl | yosys script + CLI | Tcl |
| **Constraint format** | `.xdc` (SDC superset) | `.sdc` + `.qsf` | `.pcf` / `.lpf` | `.lpf` / `.pdc` |
| **IP catalog** | VLNV-based (`vivado_ip`) | Platform Designer (`quartus_ip`) | None (raw primitives) | Radiant IP (`radiant_ip`) |
| **OOC synthesis** | `synth_design -mode ooc` | Design partitions (Pro) | Not supported | Not supported |
| **Checkpoint format** | `.dcp` | `.qdb` | JSON netlist | N/A |
| **Incremental build** | Reference `.dcp` | Incremental compilation | Not supported | Limited |
| **Block design** | `.bd` (Vivado IPI) | `.qsys` (Platform Designer) | N/A | N/A |
| **HLS integration** | Vitis HLS | Intel HLS Compiler | N/A | N/A |
| **License required** | Yes | Yes (free for some devices) | No | Yes |

**Implementation priorities:**

- **Phase 1-3:** Vivado backend (fully featured). This is the reference implementation demonstrating all framework capabilities.
- **Phase 4:** Quartus Prime backend (Pro and Standard editions). Quartus has a different execution model — the Fitter combines optimization, placement, and routing into a single monolithic step, so the generic `optimize → place → route` sub-phases map to a single Fitter invocation with internal sub-phases. The Quartus backend must report Fitter progress using the generic sub-phase names for consistency.
- **Phase 4-5:** yosys + nextpnr backend. This is architecturally interesting because it's a two-tool pipeline (yosys for synthesis, nextpnr for P&R) rather than a monolithic vendor tool. The backend orchestrates both tools. This backend demonstrates that the framework isn't secretly coupled to the "one vendor tool" model. Also valuable for CI: yosys/nextpnr can run syntax and elaboration checks on designs targeting any vendor, providing fast feedback without requiring commercial licenses.
- **Phase 5+:** Lattice Radiant backend. Similar architecture to Vivado/Quartus but with a smaller device and IP portfolio.

**Backend capability declarations.** Similar to the simulator capability model (§10.3.3), backend plugins declare their capabilities so the framework can adapt:

```python
@dataclass
class BackendCapabilities:
    supports_ooc: bool              # can synthesize components out-of-context
    supports_incremental: bool      # can use reference checkpoints
    supports_ip_generation: bool    # has a declarative IP generator plugin
    supports_block_design: bool     # has a block design generator plugin
    supports_strategy_sweep: bool   # can run multiple strategies in parallel
    checkpoint_format: str          # "dcp", "qdb", "json", "none"
    constraint_formats: list[str]   # ["xdc"], ["sdc", "qsf"], ["pcf"], etc.
    sub_phases: list[str]           # which generic sub-phases are supported
```

When a project uses a feature that the active backend doesn't support (e.g., OOC synthesis on yosys), the framework emits a clear diagnostic rather than failing opaquely: "Component 'dsp_core' has ooc = true, but backend 'yosys_nextpnr' does not support out-of-context synthesis. The component will be synthesized in-context."

---

## 11. Workspace Structure [Phase 1]

### 11.1 Recommended Layout

```
repo/
├── workspace.toml                # workspace root
├── loom.lock                     # dependency lockfile (committed to VCS)
├── lib/                          # shared component library
│   ├── axi_common/
│   │   ├── component.toml
│   │   ├── rtl/
│   │   └── tb/
│   ├── axi_async_fifo/
│   │   ├── component.toml
│   │   ├── rtl/
│   │   ├── constraints/
│   │   └── tb/
│   └── dsp_primitives/
│       ├── component.toml
│       ├── rtl/
│       └── tb/
├── platforms/
│   ├── zcu104/
│   │   ├── platform.toml
│   │   └── constraints/
│   └── custom_board_v2/
│       ├── platform.toml
│       └── constraints/
├── projects/
│   ├── radar_processor/
│   │   ├── project.toml
│   │   ├── src/
│   │   ├── constraints/
│   │   ├── ip/
│   │   └── scripts/
│   └── comms_baseband/
│       ├── project.toml
│       ├── src/
│       └── constraints/
├── tools/
│   ├── plugins/                  # workspace-local plugins
│   ├── hooks/                    # shared hook scripts
│   └── scripts/                  # utility scripts
└── .build/                       # build artifacts (gitignored)
    ├── radar_processor/
    │   ├── default/              # base project, default strategy
    │   │   ├── build.tcl         # generated build scripts (Tcl, yosys script, etc.)
    │   │   ├── build_state.json  # checkpoint tracking for --resume
    │   │   ├── post_synth.dcp    # checkpoint files (format is backend-specific:
    │   │   ├── post_route.dcp    #   .dcp for Vivado, .qdb for Quartus, etc.)
    │   │   └── radar_top.bit     # bitstream output
    │   ├── aggressive/           # base project, aggressive strategy
    │   ├── kcu116.full.off/      # dimensional profile build
    │   └── kcu116.reduced.off/   # another dimensional combination
    └── cache/                    # generator and build cache
```

### 11.2 Workspace Manifest

```toml
[workspace]
name = "my_fpga_repo"
members = ["lib/*", "platforms/*", "projects/*"]

[settings]
default_tool_version = "2023.2"
build_dir = ".build"

[resolution.overrides]
# Override dependency resolution for local development
# some_lib = { path = "/path/to/local/checkout" }

[plugins]
# Workspace-local plugin declarations

[hooks]
pre_build = "tools/hooks/check_licenses.sh"
post_build = "tools/hooks/extract_metrics.py"
```

---

## 12. CLI Design [Phase 1 core, Phase 2 polish]

The primary user interface is a command-line tool. The CLI command is `loom`.

### 12.1 UX Principles

The CLI is the face of the framework. It must feel like a tool built by people who care about developer experience.

**Fast startup.** The CLI should be responsive on every invocation. Manifest parsing, plugin discovery, and dependency resolution for a typical workspace should complete in under 200ms. This rules out architectures that eagerly load heavyweight dependencies or invoke vendor tools at startup. The implementation language and module structure should be chosen with cold-start time in mind.

**Progressive information density.** Default output is concise and human-readable: a progress indicator during builds, a summary table at the end. Verbose mode (`-v`, `-vv`) adds detail incrementally. JSON mode (`--json`) provides machine-readable output. The user never has to parse wall-of-text output to find what they need.

**Clear error messages.** Every error should state what went wrong, where (file and line if applicable), and what to do about it. "Missing dependency `axi_common`" is insufficient. "Component `axi_async_fifo` (lib/axi_async_fifo/component.toml:18) depends on `axi_common >=1.0.0`, which was not found in the workspace. Run `loom deps tree` to inspect the dependency graph." is better.

**Color and formatting.** The CLI should use color (with automatic detection and `--no-color` override) to distinguish status, errors, warnings, and structure. Build reports should be formatted as aligned tables. Timing pass/fail should be immediately visually obvious.

**Consistent command grammar.** Commands follow a `noun verb` or `verb` pattern. Flags are consistent across commands (`-p` always means project, `-v` always means verbose). Global flags apply everywhere.

### 12.2 Command Reference

```
SCAFFOLDING
  loom new component <path>             Scaffold a new component
  loom new project <path>               Scaffold a new project
  loom new platform <path>              Scaffold a new platform

BUILD
  loom build                              Build the project in the current directory
  loom build -p <project>                 Build a named project from anywhere
  loom build --strategy <n>            Build with a specific strategy
  loom build --sweep                      Run all strategies in parallel
  loom build --profile <n>             Build with a named profile
  loom build --profile board=kcu116       Dimensional profile selection
  loom build --profile-all                Build all valid profile combinations
  loom build --profile-all board          Sweep one dimension, defaults for rest
  loom build --resume                     Resume from last successful checkpoint
  loom build --resume --strategy <n>   Resume with a different strategy
  loom build --stop-after <sub-phase>    Stop after a sub-phase (e.g., synthesis)
  loom build --start-at <sub-phase>      Start from a checkpoint sub-phase
  loom build --dry-run                    Show execution plan without building
  loom build -j <N>                       Limit parallel jobs

GENERATION
  loom generate                         Run generators only (no backend build)
  loom ip regen                         Regenerate all declarative IP

SIMULATION
  loom sim                              Run simulation (default testbench)
  loom sim --top <testbench>            Run a specific testbench
  loom sim --tool <simulator>           Use a specific simulator plugin

INSPECTION
  loom report                           Display the last build report
  loom report --diff <ref>              Compare metrics against a git ref
  loom report --metrics timing.clocks   Show specific metrics subtree
  loom deps tree                        Show dependency graph
  loom deps tree -p <project>           Show dependencies for a specific project
  loom lint                             Validate manifests (no build)
  loom plugin list                      List available plugins

DEPENDENCIES
  loom deps update                      Re-resolve all dependencies, regenerate lockfile
  loom deps update <n>               Re-resolve a single dependency

ENVIRONMENT
  loom env check                        Validate environment (tool versions, licenses)
  loom env shell                        Enter a subshell with correct tool versions on PATH

MIGRATION
  loom migrate tcl-audit <script>       Analyze a Tcl script's file-system and Vivado API usage
  loom migrate tcl-wrap <script>        Generate a declarative generator config from a Tcl script
  loom migrate xci-to-toml <file.xci>   Convert Vivado .xci IP config to vivado_ip TOML block
  loom migrate xci-to-toml --batch <dir> Convert all .xci files in a directory
  loom migrate qsf-to-toml <file.qsf>  Convert Quartus QSF settings to Loom manifests (Phase 4)
  loom migrate ip-to-toml <file.ip>     Convert Quartus .ip config to quartus_ip TOML block (Phase 4)

IP MANAGEMENT
  loom ip upgrade                       Report IP version updates for new tool version
  loom ip upgrade --apply               Rewrite TOML files to bump IP versions
  loom ip upgrade --check-properties    Validate property names against new IP version

DEVELOPER EXPERIENCE
  loom lsp                              Generate LSP configuration for HDL editors
  loom lsp --format svls                Generate for a specific LSP (svls, verible, slang)

MAINTENANCE
  loom clean                            Remove build artifacts for current project
  loom clean --all                      Remove all workspace build artifacts
```

### 12.3 IDE and Editor Integration

One of the main reasons software developers use CMake or Bazel is because they generate `compile_commands.json` for the C++ Language Server. Modern HDL developers use LSPs (SVLS, Verible, Slang) that need the same information: file lists, include directories, and preprocessor defines.

**`loom lsp`** runs Phases 1-3 (RESOLVE, GENERATE, ASSEMBLE) and exports the assembled file-set in a format that HDL Language Servers can consume. The output includes: ordered file list, include search paths, preprocessor defines (including platform parameter substitutions), and tool-specific settings.

The default output format is a JSON file (`.loom/lsp.json`) that follows a Loom-specific schema, with adapter scripts for specific LSPs:

```json
{
    "version": 1,
    "project": "radar_processor",
    "platform": "zcu104",
    "defines": ["DDR4_DATA_WIDTH=64", "PCIE_LANES=4", "SYS_CLK_FREQ_MHZ=125"],
    "include_dirs": ["lib/axi_common/rtl", "generated/regmap"],
    "files": [
        { "path": "lib/axi_common/rtl/axi_pkg.sv", "language": "systemverilog" },
        { "path": "lib/axi_async_fifo/rtl/axi_async_fifo.sv", "language": "systemverilog" },
        { "path": "projects/radar_processor/src/radar_top.sv", "language": "systemverilog" }
    ]
}
```

**Per-LSP format support:**

- `loom lsp --format svls` generates `.svls.toml` with include paths and defines.
- `loom lsp --format verible` generates a Verible-compatible file list.
- `loom lsp --format slang` generates Slang compile arguments.

The LSP output updates automatically when generators run — `loom generate` refreshes the LSP config as a side effect. For editor integration, users add `loom lsp` to their editor's "on save" hook or run it once per session.

### 12.4 Exit Codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Build failed (timing not met, synthesis error, etc.) |
| 2 | Configuration error (bad manifest, missing dependency) |
| 3 | Environment error (tool not found, wrong version, license) |
| 4 | Internal error |

### 12.5 Output Modes

The `--json` global flag makes any command produce JSON output for scripting and CI consumption. The `--quiet` flag suppresses all output except errors. These modes are mutually exclusive with verbose output.

Build progress should use a compact, updating display (similar to `cargo build`) that shows the current phase, elapsed time, and any warnings as they occur. When stdout is not a TTY (e.g., in CI), the progress display degrades gracefully to line-by-line log output.

---

## 13. Environment Management [Phase 1 basic, Phase 4 shell]

### 13.1 Tool Version Enforcement

The platform manifest declares a required backend tool and version. The framework checks the installed version at build start and fails with a clear message if there's a mismatch:

```
Error: Project "radar_processor" requires Vivado 2023.2 (via platform "zcu104"),
       but Vivado 2024.1 is on PATH.
       Install the correct version or update platform.toml.
```

For backends without strict version pinning (e.g., yosys/nextpnr OSS flow), version enforcement is optional — the platform manifest can omit the version field, and the framework uses whatever is available.

### 13.2 Tool Discovery

The framework discovers vendor tools via a strict priority order. Higher-priority sources always override lower ones:

1. **Explicit configuration** in `workspace.toml` (`[settings.tools.vivado] path = "/tools/Xilinx/Vivado/2023.2"`) — highest priority.
2. **Environment variable** (`VIVADO_PATH`, `QUARTUS_ROOTDIR`, `YOSYS_PATH`, `RADIANT_PATH`, etc.)
3. **Standard installation paths** (e.g., `/tools/Xilinx/Vivado/<version>/`, `/tools/intelFPGA_pro/<version>/quartus/`, `/usr/local/bin/yosys`, `C:\Xilinx\Vivado\<version>\` on Windows)
4. **`PATH` search** — lowest priority.

Multiple versions can be installed simultaneously. The framework selects the highest version matching the project/platform requirement and logs the choice.

`loom env check` reports discovered tools and selection:

```
$ loom env check
  Backend: vivado
    /tools/Xilinx/Vivado/2023.2  (selected — matches project requirement "2023.2")
    /tools/Xilinx/Vivado/2024.1  (available)
  License: 27000@license-server.internal (reachable)
  Simulator: Questa 2023.4 at /tools/mentor/questa/2023.4/bin
```

```
$ loom env check                     # different project targeting Intel
  Backend: quartus
    /tools/intelFPGA_pro/23.1/quartus  (selected)
  License: 1800@lic.internal (reachable)
  Simulator: Verilator 5.024 at /usr/local/bin/verilator
```

```
$ loom env check                     # OSS project targeting iCE40
  Backend: yosys_nextpnr
    yosys 0.40 at /usr/local/bin/yosys
    nextpnr-ice40 at /usr/local/bin/nextpnr-ice40
  License: not required
  Simulator: Icarus Verilog 12.0 at /usr/local/bin/iverilog
```

### 13.3 Reproducible Environments

**`loom env shell`** — Subshell with correct tool versions on `PATH`, environment variables, and license settings. Analogous to `poetry shell` or `nix develop`:

```
$ loom env shell
  Activating environment for radar_processor
  Vivado 2023.2: /tools/Xilinx/Vivado/2023.2/bin
  License: 27000@license-server.internal
  Python plugins: .loom/plugins/venv

(loom:radar_processor) $ vivado -version
Vivado v2023.2 (64-bit)
```

The shell prompt includes the project name to clearly indicate which environment is active. This ensures that interactive vendor tool use (Vivado GUI for debugging, Quartus Timing Analyzer, etc.) matches the automated build environment exactly.

**Container support.** The framework is designed to run inside containers (Docker, Podman) or Nix environments. It does not assume a specific OS layout beyond the tool being discoverable. `loom env dockerfile` can generate a Dockerfile skeleton for CI that includes the correct vendor tool version, Loom, and project dependencies.

**Nix integration (future).** For teams using Nix, Loom can generate a `flake.nix` or `shell.nix` that pins the exact tool versions. This is the gold standard for reproducibility but requires Nix adoption.

### 13.4 Cross-Platform Support [Phase 2]

Windows is a first-class platform from Phase 2. Many teams develop on Windows and build in Linux CI.

#### 13.4.1 Path Handling

- **Path separators.** All paths in manifests use forward slashes (`/`), which work on both platforms. The Rust core normalizes to the native separator when invoking tools or writing files.
- **Case sensitivity.** Loom treats file paths as case-sensitive on all platforms for consistency. On case-insensitive file systems (Windows, macOS), this means `Rtl/MyModule.sv` and `rtl/mymodule.sv` are different files in the manifest even if the OS treats them as the same. The pre-build validation (§7.4) warns about case-sensitivity mismatches.
- **Generated scripts.** Backend plugins must generate vendor tool scripts with forward slashes regardless of the build host. Vivado and Quartus Tcl both interpret backslashes as escape characters; yosys scripts are also forward-slash-native.
- **Environment variables.** Plugin authors should use Loom's `BuildContext.env` dict rather than reading `os.environ` directly, since Loom normalizes platform-specific variable syntax.

#### 13.4.2 Shell and Command Execution

**Generator commands.** The `command` generator executes via `subprocess`. On Windows: `cmd.exe /c` or `powershell.exe -Command` instead of `/bin/sh -c`. Manifest supports platform overrides:

```toml
[[generators]]
name = "gen_regs"
plugin = "command"
[generators.config]
command = "python scripts/gen_regs.py"     # default (portable)
command_windows = "py scripts\\gen_regs.py" # Windows override (optional)
```

If `command_windows` is omitted, `command` is used on all platforms.

**Hook scripts.** Same convention — portable commands or platform overrides. Python recommended over shell scripts for cross-platform hooks.

**`loom env shell`.** Linux/macOS: subshell with `$SHELL`. Windows: PowerShell session with equivalent environment.

#### 13.4.3 CI and Build Server Considerations

The common pattern — develop on Windows, build on Linux — means manifests must be portable. The framework validates this: `loom lint` checks for platform-specific assumptions in commands and paths. The pre-build validation (§7.4) warns if a hook script has a `#!/bin/bash` shebang but the build host is Windows.

Windows CI (GitHub Actions `windows-latest`, Azure DevOps Windows agents) should be tested from Phase 2 onward. The Rust core and `std::path` handle the platform differences well, but Python plugin scripts and Tcl generation are where bugs will surface.

---

## 14. Architecture Decisions [Reference]

### 14.1 Implementation Language

**Decision:** Rust core with Python plugin SDK.

The core (manifest parsing, dependency resolution, DAG construction, cache management, CLI) is Rust. Plugins (generators, backends, simulators, reporters) are Python, loaded via PyO3.

**Why Rust core:** Sub-200ms CLI startup. Python import overhead makes this unreliable (300-800ms). Core logic (TOML parsing, graph algorithms, file hashing) benefits from Rust's type safety and performance.

**Why Python plugins:** FPGA engineers write Python. Generator scripts are Python. Vendor tool interop happens via subprocess. PyO3 bridges the boundary: Rust core embeds Python, loads plugin modules via entry points. Plugin authors see only Python — `import loom.plugin`, subclass, register. See §7.7.1 for the execution model.

**Distribution:** Single `loom` binary via `cargo install loom-fpga`. Backend plugins installed separately:

- `pip install loom-vivado-backend`
- `pip install loom-quartus-backend`
- `pip install loom-yosys-backend`
- `pip install loom-radiant-backend`

The binary has no vendor dependencies. CI servers need only the backends they use.

### 14.2 Manifest Format

**Decision:** TOML. Unambiguous, well-specified, widely supported, and familiar from Rust/Python ecosystems. YAML's implicit typing issues and JSON's verbosity make them less suitable for human-authored configuration.

### 14.3 Simulation Backend Model

**Decision:** Separate `SimulatorPlugin` type, distinct from `BackendPlugin`. See §10.3.3. Simulation and synthesis have fundamentally different execution models, tool diversity, and lifecycle requirements.

---

## 15. Implementation Roadmap

### Phase 1: End-to-End Proof

**Goal:** A Vivado project builds from `loom build` to bitstream. No generators, no profiles, no platforms. Just: manifests → dependency resolution → file-set assembly → Vivado non-project-mode build → exit code.

**Deliverables:**

- Rust project skeleton with CLI framework (clap)
- Manifest parser: `component.toml`, `project.toml`, `workspace.toml` (TOML via `toml` crate)
- Dependency resolution service (workspace-scoped, no registry)
- Lockfile generation and validation (`loom.lock`)
- File-set assembly with constraint scoping and ordering
- Plugin trait definitions (`BackendPlugin`, `GeneratorPlugin`) — interfaces only
- Vivado backend plugin: non-project-mode Tcl generation, batch execution, basic log capture
- Pre-build validation (Phase 4 checks: file existence, tool env, basic sanity)
- CLI: `loom build`, `loom clean`, `loom env check`, `loom deps tree`, `loom lint`
- Basic exit codes and error formatting

**Validation criterion:** An existing Vivado project, re-expressed as Loom manifests, builds successfully and produces an identical bitstream.

#### Phase 1 Crate Structure

```
loom-fpga/
├── Cargo.toml                    # workspace manifest
├── crates/
│   ├── loom-cli/                 # binary crate — CLI entry point
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs           # clap setup, command dispatch
│   │       └── commands/         # one module per CLI command
│   │           ├── build.rs
│   │           ├── clean.rs
│   │           ├── deps.rs
│   │           ├── env.rs
│   │           └── lint.rs
│   ├── loom-core/                # library crate — all framework logic
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── manifest/         # TOML parsing and validation
│   │       │   ├── component.rs  # ComponentManifest struct + deserialization
│   │       │   ├── project.rs    # ProjectManifest struct
│   │       │   ├── workspace.rs  # WorkspaceManifest struct
│   │       │   └── common.rs     # shared types (VersionReq, FileSet, etc.)
│   │       ├── resolve/          # dependency resolution
│   │       │   ├── resolver.rs   # DependencySource trait, workspace resolver
│   │       │   ├── lockfile.rs   # loom.lock read/write/staleness
│   │       │   └── graph.rs      # dependency graph construction + cycle detection
│   │       ├── assemble/         # file-set assembly
│   │       │   ├── fileset.rs    # collect files, apply constraint scoping
│   │       │   └── ordering.rs   # constraint ordering (component-scoped before global)
│   │       ├── build/            # build pipeline orchestration
│   │       │   ├── pipeline.rs   # Phase 1-7 sequencing
│   │       │   ├── validate.rs   # Phase 4 pre-build checks
│   │       │   └── context.rs    # BuildContext passed to backends
│   │       ├── plugin/           # plugin trait definitions
│   │       │   ├── backend.rs    # BackendPlugin trait (Rust trait, not Python yet)
│   │       │   └── generator.rs  # GeneratorPlugin trait (interface only in Phase 1)
│   │       └── error.rs          # LoomError enum, diagnostic formatting
│   └── loom-vivado/              # Vivado backend (Rust in Phase 1, Python in Phase 2+)
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── tcl_gen.rs        # generate non-project-mode Tcl script
│           ├── executor.rs       # spawn `vivado -mode batch`, capture logs
│           └── env_check.rs      # find Vivado, check version, license
```

#### Phase 1 Core Data Types

These Rust structs are the framework's internal representation. They map directly to the TOML schemas in §3.1, §5.1, §11.2.

```rust
// loom-core/src/manifest/component.rs
#[derive(Debug, Deserialize)]
pub struct ComponentManifest {
    pub component: ComponentMeta,
    pub filesets: HashMap<String, FileSet>,
    pub dependencies: HashMap<String, DependencySpec>,
    pub synth: Option<SynthOptions>,
}

#[derive(Debug, Deserialize)]
pub struct ComponentMeta {
    pub name: String,          // "acmecorp/axi_async_fifo"
    pub version: String,       // semver: "1.2.0"
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FileSet {
    pub files: Vec<PathBuf>,
    pub constraints: Option<Vec<PathBuf>>,
    pub constraint_scope: Option<String>,  // "component" | "global"
    pub include_synth: Option<bool>,       // sim fileset includes synth files
    pub defines: Option<Vec<String>>,
    pub compile_options: Option<Vec<String>>,
}

// loom-core/src/manifest/project.rs
#[derive(Debug, Deserialize)]
pub struct ProjectManifest {
    pub project: ProjectMeta,
    pub target: Option<TargetSpec>,       // direct part spec (Phase 1)
    pub filesets: HashMap<String, FileSet>,
    pub dependencies: HashMap<String, DependencySpec>,
    pub build: Option<BuildConfig>,
    // Phase 3+: platform, profiles, generators
}

#[derive(Debug, Deserialize)]
pub struct TargetSpec {
    pub part: String,           // "xczu7ev-ffvc1156-2-e"
    pub backend: String,        // "vivado"
    pub version: Option<String>, // "2023.2"
}

// loom-core/src/resolve/resolver.rs
pub struct ResolvedProject {
    pub project: ProjectManifest,
    pub dependency_graph: DependencyGraph,  // topologically sorted
    pub assembled_components: Vec<ResolvedComponent>,
}

pub struct ResolvedComponent {
    pub manifest: ComponentManifest,
    pub source_path: PathBuf,    // where on disk
    pub resolved_version: String,
}

// loom-core/src/assemble/fileset.rs
pub struct AssembledFilesets {
    pub synth_files: Vec<AssembledFile>,     // ordered: deps first, project last
    pub constraint_files: Vec<AssembledFile>, // ordered: component-scoped, then global
    pub defines: Vec<String>,
}

pub struct AssembledFile {
    pub path: PathBuf,           // absolute path to file
    pub source_component: String, // which component contributed this
    pub scope: Option<String>,    // constraint scope, if applicable
}
```

#### Phase 1 Build Pipeline

The Phase 1 `loom build` execution is a linear pipeline (no DAG, no parallelism):

```
fn build(workspace_root: &Path, project_name: &str) -> Result<BuildResult> {
    // Phase 1: RESOLVE
    let workspace = parse_workspace(workspace_root)?;
    let project = parse_project(workspace_root, project_name)?;
    let lockfile = load_or_generate_lockfile(&workspace, &project)?;
    let resolved = resolve_dependencies(&workspace, &project, &lockfile)?;

    // Phase 2: GENERATE — skipped in Phase 1 (no generators)

    // Phase 3: ASSEMBLE
    let filesets = assemble_filesets(&resolved)?;

    // Phase 4: VALIDATE
    validate_pre_build(&filesets, &resolved)?;

    // Phase 5: BUILD
    let backend = VivadoBackend::new(&resolved.project.target)?;
    backend.check_environment()?;
    let scripts = backend.generate_build_scripts(&resolved, &filesets)?;
    let result = backend.execute_build(&scripts)?;

    // Phase 6: EXTRACT — minimal in Phase 1 (just exit code + log path)
    // Phase 7: REPORT — minimal in Phase 1 (print pass/fail to terminal)
    
    Ok(result)
}
```

#### Phase 1 Vivado Tcl Generation

The Vivado backend generates a single Tcl script for non-project-mode batch execution:

```tcl
# Auto-generated by Loom — do not edit
# Project: radar_processor
# Part: xczu7ev-ffvc1156-2-e

# Read source files (dependency order)
read_verilog -sv {/abs/path/lib/axi_common/rtl/axi_common_pkg.sv}
read_verilog -sv {/abs/path/lib/axi_async_fifo/rtl/cdc_gray_counter.sv}
read_verilog -sv {/abs/path/lib/axi_async_fifo/rtl/axi_async_fifo.sv}
read_verilog -sv {/abs/path/projects/radar_processor/src/radar_top.sv}

# Read constraints (component-scoped first, then global)
read_xdc -ref axi_async_fifo {/abs/path/lib/axi_async_fifo/constraints/cdc_false_paths.xdc}
read_xdc {/abs/path/projects/radar_processor/constraints/timing.xdc}
read_xdc {/abs/path/projects/radar_processor/constraints/physical.xdc}

# Synthesis
synth_design -top radar_top -part xczu7ev-ffvc1156-2-e

# Implementation
opt_design
place_design
route_design

# Bitstream
write_bitstream -force {.build/radar_processor/default/radar_top.bit}
```

The Tcl generator must handle: VHDL vs. SystemVerilog `read_*` commands, library mapping for VHDL, absolute paths with forward slashes (even on Windows), and `-ref` scoping for component constraints with `constraint_scope = "component"`.

### Phase 2: Generators, Caching, and CLI Polish

**Goal:** Code generation works. Incremental builds work. The CLI feels good to use. Developers get LSP integration. Windows is a first-class platform.

- `command` generator plugin (run arbitrary shell commands, with `command_windows` override)
- Generator DAG with input/output dependency detection
- Cache key computation and incremental build (skip unchanged nodes)
- `vivado_ip` generator plugin (declarative IP from TOML config, floating VLNV)
- Tcl fragment generator with `outputs_unknown` support and warnings
- Constraint templating (`.xdc.tpl`, `.sdc.tpl`, `.pcf.tpl` preprocessing, see §3.3.1)
- JSON build report with hierarchical metrics (timing per clock, utilization detail)
- Build checkpoint tracking and `loom build --resume` (see §7.5)
- Build sub-phase control: `--stop-after`, `--start-at` (see §7.2.2)
- `loom build --dry-run` (see §7.6)
- `loom lsp` for HDL editor integration (see §12.3)
- `loom ip upgrade` with `--check-properties` validation (see §6.5.1)
- `loom migrate xci-to-toml` for IP migration from existing .xci files (see §6.6.1)
- CLI UX: color output, progress display, verbose modes, `--json` flag, `-j N` parallelism
- CLI: `loom generate`, `loom ip regen`, `loom report`
- PyO3 integration: Python plugin loading and subprocess-based execution (see §7.7.1)
- Windows CI: Automated test suite running on Windows (GitHub Actions / Azure DevOps)

### Phase 3: Platforms, Variants, and Profiles

**Goal:** A single project targets multiple boards. Components have vendor-specific variants. Profile dimensions enable combinatorial builds. Virtual platforms support sim-only development.

- Platform manifest parsing and resolution (`platform.toml`)
- Virtual platform support (`virtual = true`, see §4.1.1)
- Platform parameter substitution in manifests, defines, and constraint templates
- Platform parameters flowing into HDL via defines (see §4.2.1)
- Component variant model and tag-based variant selection
- Simple project profiles and profile dimension composition (see §5.2.2)
- Profile exclusion rules and `loom build --profile-all` sweeping
- Out-of-context synthesis for components (`ooc = true`, see §7.2.1)
- Scaffolding: `loom new component`, `loom new project`, `loom new platform`

### Phase 4: Reporting, CI, Hooks, and Quartus Backend

**Goal:** Build metrics are tracked over time. CI integration is seamless. Hook contract is fully specified. A second vendor backend validates the abstraction layer.

- Reporter plugin interface
- Built-in reporters: `console` (formatted terminal tables), `json`, `github_actions`
- Hierarchical metrics diff across git refs (`loom report --diff`)
- Hook system with full contract (environment variables, context file schema, exit codes — see §10.5)
- `loom env shell` for reproducible interactive environments
- `loom migrate tcl-audit` and `loom migrate tcl-wrap` for Tcl migration
- **Quartus Prime backend** (Pro and Standard editions) — validates that the backend interface genuinely supports non-Vivado tools. Includes: Tcl/QSF script generation, Fitter invocation with sub-phase progress reporting, `.sdc` constraint handling, `quartus_ip` generator for Platform Designer IP, `loom ip upgrade` support for Intel IP versions. This is the critical "second backend" that proves the architecture is truly vendor-agnostic.
- `BackendCapabilities` model (§10.6) — backends declare supported features

### Phase 5: Simulation, yosys Backend, and Advanced Build

**Goal:** Verification is a first-class workflow. Timing closure is assisted. OSS toolchain support broadens adoption.

- `SimulatorPlugin` interface with capability model (§10.3.3) and initial backends (Vivado xsim, Verilator)
- Simulator capability queries and test-simulator compatibility filtering
- Simulation compile options and tool-specific flag passthrough
- Multi-strategy sweep with parallel execution and selection logic
- Incremental build with reference checkpoints (§7.3.1)
- `vivado_bd` generator plugin (block design Tcl export/regenerate)
- **yosys + nextpnr backend** — demonstrates the two-tool pipeline model (yosys for synthesis, nextpnr for P&R). Targets ice40, ECP5, and Gowin architectures. Valuable for CI even on Vivado/Quartus projects: yosys can run fast syntax/elaboration checks without a commercial license.
- CLI: `loom sim`, `loom sim --check-compat`, `loom build --sweep`

### Phase 6: Test Organization (Design Phase)

**Goal:** Design (and begin implementing) structured test management for verification workflows.

This phase begins with design work — the test organization model needs careful thought before implementation. The core question is how Loom should represent test suites, test cases, and their relationship to components and projects. See §16 for the design direction.

- Test suite and test case manifest model (component-level and project-level)
- Test discovery and selection (`loom sim --suite`, `loom sim --filter`)
- Test result aggregation and reporting (pass/fail/error per case, coverage merge)
- Regression mode (`loom sim --regression` — run all tests, report summary)
- Integration with CI (JUnit XML output, coverage artifacts)

### Phase 7: Ecosystem

**Goal:** Loom works beyond a single team's monorepo. Vendor coverage is comprehensive.

- Package registry design for cross-repo dependencies
- Lattice Radiant backend
- Additional simulator plugins (Questa, VCS, Xcelium, Icarus)
- `quartus_qsys` generator for Platform Designer subsystems
- `loom env dockerfile` for CI container generation
- Community backend plugin development guide
- Documentation, examples, tutorials, and onboarding guides

---

## 16. Future Direction: Test Organization [Phase 6]

Simulation support in Phase 5 covers the mechanics — compiling, elaborating, and running testbenches. But real verification workflows need more structure: test suites, test cases, selection filters, regression management, and coverage aggregation. This section outlines the design direction for structured test management, which will be designed in Phase 6 and refined through implementation experience.

### 16.1 The Problem

A typical FPGA project has dozens to hundreds of tests organized along multiple dimensions: unit tests per component, integration tests per subsystem, system-level tests per project, and cross-cutting concerns like coverage and performance benchmarks. Today these are managed through ad-hoc Makefile targets, shell scripts, or vendor tool GUIs. There is no standard model for declaring, discovering, organizing, or reporting on tests.

### 16.2 Design Direction: Testbenches as Miniature Projects

The key architectural insight — inspired by Rust's `cargo test` — is that each testbench is essentially a miniature, isolated "project" that targets a simulator backend instead of a synthesis backend. Just as `cargo test` compiles each test binary independently, Loom should treat each test case as its own resolution → generate → assemble → compile → elaborate → simulate pipeline.

This means:

- A test case has its own resolved dependency graph (the component under test + its transitive dependencies + test-only dependencies like BFMs or assertion libraries).
- A test case has its own file-set (the component's sim fileset + the testbench file + any test-specific stimulus files).
- Test cases are independent and can execute in parallel without interference.
- Coverage databases from parallel test runs are merged post-execution.

This model naturally supports component-level unit tests (a component's own testbench), integration tests (a project-level testbench composing multiple components), and system-level tests (full design simulation).

### 16.3 Test Manifests

Components and projects declare test cases in their manifests. A test case specifies a testbench top, simulator options, and expected outcomes:

```toml
# component.toml — test declarations
[[tests]]
name = "basic_loopback"
top = "axi_async_fifo_tb"
description = "Basic data loopback at various clock ratios"
timeout_seconds = 300
tags = ["smoke", "regression"]

[[tests]]
name = "overflow_recovery"
top = "axi_async_fifo_overflow_tb"
description = "Verify FIFO behavior under overflow conditions"
timeout_seconds = 600
tags = ["regression", "corner_case"]
[tests.sim_options]
defines = ["OVERFLOW_INJECT=1"]

[[tests]]
name = "cdc_stress"
top = "axi_async_fifo_cdc_tb"
description = "CDC stress with randomized clock jitter"
timeout_seconds = 1200
tags = ["stress", "nightly"]
[tests.sim_options]
defines = ["JITTER_EN=1", "RANDOM_SEED=${seed}"]
[tests.requires]
fork_join = true              # uses dynamic processes — incompatible with Verilator

# Test-only dependencies (not needed for synthesis)
[tests.dependencies]
axi_bfm = ">=1.0.0"
```

**Test suites.** Named collections of tests, defined by tag filters or explicit lists:

```toml
# project.toml or workspace.toml
[test_suites]
smoke = { tags = ["smoke"] }
regression = { tags = ["regression"] }
nightly = { tags = ["nightly", "stress", "regression"] }
component_only = { components = ["acmecorp/axi_async_fifo", "acmecorp/axi_common"] }
```

**CLI integration:**

```
loom sim --suite smoke                Run smoke tests
loom sim --suite regression           Run full regression
loom sim --filter "axi_*"             Run tests matching glob
loom sim --tag stress                 Run all tests tagged "stress"
loom sim --regression                 Run all tests, produce summary report
```

### 16.4 Coverage Merging

**Decision:** Loom handles coverage merging, delegated to the simulator plugin.

Simulators produce wildly different coverage database formats: UCDB (Questa), VDB (VCS), coverage.dat (Verilator). The `SimulatorPlugin` interface includes a `merge_coverage()` method (see §10.3.3) that combines per-test coverage databases into a single merged database. Loom orchestrates the merge after all tests in a suite complete.

The workflow:

1. Each test runs independently, producing its own coverage database in the build directory.
2. After all tests complete (pass or fail), Loom collects the per-test coverage databases.
3. Loom calls `simulator.merge_coverage(dbs, output_path)` to produce a merged database.
4. The merged coverage data (line/toggle/branch/FSM percentages) is included in the test report.

This enables massively parallel regression suites — tests run on separate machines, coverage databases are collected, and Loom merges them into a single report.

### 16.5 Test Result Model

Each test case produces a structured result (pass/fail/error/timeout) with optional coverage data. Results aggregate into a test report:

```json
{
    "suite": "regression",
    "total": 47,
    "passed": 45,
    "failed": 1,
    "errors": 1,
    "duration_seconds": 3842.0,
    "coverage": { "line": 87.3, "toggle": 72.1, "branch": 68.4 },
    "cases": [
        { "name": "basic_loopback", "component": "acmecorp/axi_async_fifo", "status": "pass", "duration": 142.0 },
        { "name": "overflow_recovery", "component": "acmecorp/axi_async_fifo", "status": "fail", "duration": 311.0,
          "failure": { "message": "Assertion failed at tb/overflow_tb.sv:142", "log": "..." } }
    ]
}
```

### 16.6 Open Questions for Phase 6

These questions will be resolved through design work in Phase 6:

- **Parameterized tests.** How to declare a test that runs multiple times with different parameters (seeds, data widths, configurations). Does the manifest support `${seed}` expansion, or is that a generator's job? The `cargo test` model suggests seed iteration should be a test runner concern, not a manifest concern.
- **Test dependencies on build phases.** Gate-level simulation requires synthesis outputs (netlists). Test cases need a way to declare that they depend on a build phase completing first. This may mean test cases can reference a project's build artifacts.
- **Test stability tracking.** Detecting flaky tests requires history across runs. How does this integrate with the metrics regression model from §9?
- **Distributed execution.** Large regression suites benefit from parallel execution across machines. The initial model parallelizes locally via `-j N`; distributed execution may require a job scheduler interface.

---

## 17. Future Direction: Partial Reconfiguration [Future]

Partial reconfiguration (PR) is increasingly common in production FPGA systems — dynamic loading of accelerators, protocol handlers, and crypto engines at runtime. PR flows are significantly more complex than standard builds and require primitives that the current architecture must accommodate, even if full PR support is a Layer 2 convenience abstraction built later.

### 17.1 What PR Requires

A PR flow produces multiple bitstreams per project: one base bitstream (the static region) and one or more partial bitstreams (reconfigurable modules). This has several architectural implications:

**Partition definitions.** The design is divided into a static region and one or more reconfigurable partitions (RPs). Each RP has a defined floorplan area on the FPGA fabric. These are expressed as physical constraints and Vivado properties — not something the build system invents, but something it must convey to the backend.

**Reconfigurable module variants.** Each RP can be loaded with different modules at runtime. These modules are essentially component variants, but with a build-time twist: each variant must be synthesized OOC and then implemented within the partition's physical boundaries. This is a natural extension of the existing variant and OOC synthesis models.

**Multiple bitstream outputs.** A standard build produces one bitstream. A PR build produces N+1 bitstreams (1 base + N partial). The project manifest must declare this, and the backend must orchestrate the multi-phase Vivado flow: static synthesis → static implementation → per-module OOC synthesis → per-module implementation within locked static context.

**Partition boundary constraints.** The interface between static and reconfigurable regions has strict timing requirements. These constraints are auto-generated by Vivado based on the partition definition, but the project may need to influence them.

### 17.2 Core Primitives Needed

The core framework (Layer 0) doesn't need PR-specific logic, but it must support these primitives that PR builds rely on:

- **Multiple build outputs per project.** The current model assumes one bitstream per build. PR needs N+1 outputs. The backend plugin interface should allow `BuildResult` to contain multiple output artifacts.
- **Build-time variant selection.** The current variant model selects one variant at resolution time. PR needs to build *all* RP module variants in a single build invocation, each producing its own partial bitstream.
- **Dependent builds.** The partial bitstreams depend on the base build's locked static context (a checkpoint with the static region placed and routed). This is a build-to-build dependency within a single project — naturally expressed as additional nodes in the Phase 5 build DAG (§7.2).

### 17.3 Suggested Approach

PR support should be a Layer 2 abstraction — a `PartialReconfigPlugin` that uses the core's OOC synthesis, variant, and multi-output capabilities to orchestrate the Vivado PR flow. The Phase 5 build DAG (§7.2) already generalizes beyond linear pipelines, so PR nodes fit naturally:

1. Allow `BuildResult` to contain multiple named output artifacts (already accommodated by the JSON structure).
2. The backend plugin constructs a PR-specific build DAG within Phase 5: static synthesis → static implementation → per-module OOC synthesis → per-module implementation. These are just additional DAG nodes.
3. Support a project-level declaration that this is a PR build with named reconfigurable partitions and their module variants.

The detailed PR manifest syntax and orchestration logic are deferred to a future phase, but the core primitives should be kept in mind during Phase 1-3 implementation to avoid painting ourselves into a corner.

---

## Appendix A: Comparison with Existing Approaches

| Feature | Loom | Vivado Project Mode | FuseSoC / Edalize | Make/CMake | Vendor Tcl Scripts |
|---|---|---|---|---|---|
| Declarative manifests | Yes (TOML) | No (.xpr XML) | Yes (CAPI2 YAML) | Partial | No |
| Dependency resolution | Workspace + lockfile | Manual | Registry-based | Manual | Manual |
| Code generation | First-class DAG nodes | N/A | Partial (generators) | Ad-hoc targets | Ad-hoc |
| Multi-vendor support | Vivado, Quartus, yosys, Radiant | No | Yes (via edalize) | Yes (no FPGA awareness) | No |
| Structured metrics | Hierarchical, queryable | Manual report files | No | No | Manual |
| Plugin system | Generators, backends, sims, reporters | N/A | Backends via edalize | N/A | N/A |
| Strategy sweeps | Built-in parallel | Manual | No | Ad-hoc | Ad-hoc |
| Constraint composition | Scoped, ordered, templated | Manual | File list only | Manual | Manual |
| Monorepo support | Workspace model | N/A | Library model | Varies | N/A |
| Platform abstraction | First-class (multi-vendor) | Board files | No | N/A | N/A |
| Component variants | Overlay model (vendor/sim) | N/A | No | N/A | N/A |
| Build profiles | Dimensional composition | N/A | No | N/A | N/A |
| Lockfile | Yes (`loom.lock` + IP versions) | N/A | No | N/A | N/A |
| Pre-build validation | Built-in + extensible | Minimal | No | No | No |
| OOC synthesis | Per-component caching (DAG) | Manual OOC flow | No | No | Manual |
| Checkpoint resume | `--resume` from checkpoint | Manual | No | No | Manual |
| Sub-phase control | `--stop-after`, `--start-at` | Manual | No | No | Manual |
| Dry run | `--dry-run` shows full plan | No | No | `-n` (limited) | No |
| Constraint templating | `.xdc/.sdc/.pcf.tpl` | No | No | No | No |
| LSP integration | `loom lsp` for HDL editors | N/A | No | `compile_commands.json` | N/A |
| Vendor IP management | `loom ip upgrade` | Manual | No | N/A | Manual |
| Simulator capabilities | Capability model + filtering | N/A | No | N/A | N/A |
| Windows support | First-class from Phase 2 | Yes | Partial | Yes | Varies |
| Build DAG | OOC + top-level as nodes | Manual | No | Makefile DAG | Manual |
| OSS toolchain support | yosys + nextpnr backend | N/A | Yes (via edalize) | Manual | N/A |

FuseSoC/edalize is the closest existing tool. Loom differentiates in build model (DAG-based Phase 5, OOC caching, strategy sweeps, checkpoint resume), platform parameterization, and variant/profile system. FuseSoC has stronger multi-vendor coverage today via edalize.

---

## Appendix B: Glossary

- **Backend**: A plugin that drives a specific vendor tool (Vivado, Quartus, etc.) for synthesis and implementation.
- **Backend capabilities**: A structured declaration of what a synthesis/implementation backend supports (OOC, incremental builds, IP generation, checkpoint format). The framework adapts behavior based on backend capabilities, emitting diagnostics for unsupported features.
- **Build DAG**: The directed acyclic graph of build nodes within Phase 5. OOC component builds and the top-level build are nodes; edges represent data dependencies (top-level synthesis depends on OOC checkpoints). Generalizes to partial reconfiguration flows.
- **Build phase**: One of the seven sequential stages of a `loom build` invocation: RESOLVE, GENERATE, ASSEMBLE, VALIDATE, BUILD, EXTRACT, REPORT. Phase 5 (BUILD) is internally a DAG, not a linear pipeline.
- **Build sub-phase**: A step within Phase 5's build DAG. The framework defines generic names (synthesis, optimize, place, route, phys_optimize, bitstream) that each backend maps to its tool-specific commands.
- **Checkpoint**: A vendor tool snapshot produced at a build sub-phase boundary, enabling resume after failure. Format is backend-specific: Vivado `.dcp`, Quartus `.qdb`, yosys JSON netlist.
- **Component**: A reusable unit of HDL with a manifest declaring its file-sets and dependencies.
- **Constraint template**: A constraint file with `.tpl` extension (e.g., `.xdc.tpl`, `.sdc.tpl`, `.pcf.tpl`) with `{{parameter}}` placeholders, preprocessed by the framework before passing to the backend.
- **File-set**: A named collection of files of a specific type (synth, sim, constraints).
- **Generator**: A build step that produces derived files from inputs. Runs during the GENERATE phase before file-set assembly. Executed as subprocesses for GIL-free parallelism.
- **Hook**: A user-defined script executed at a lifecycle point (e.g., `pre_build`, `post_build`), receiving build context via environment variables and a JSON file. Supports timeouts and `allow_failure` for best-effort hooks.
- **Incremental synthesis**: A vendor tool feature that reuses unchanged logic from a reference checkpoint. Vivado: `synth_design -incremental`; Quartus: incremental compilation. Loom's backends automatically provide reference checkpoints from previous builds where supported.
- **Lockfile**: A file (`loom.lock`) recording the exact resolved version of every dependency and resolved vendor IP versions. Includes `workspace_members` for staleness detection. Staleness is based on semantic dependency graph changes, not file timestamps.
- **Namespace**: The `org/name` prefix on component names (e.g., `acmecorp/axi_common`), enforced from day one to prevent registry collisions.
- **OOC synthesis**: Out-of-context synthesis. Synthesizing a component into its own checkpoint independently of the top-level design, enabling per-component caching. Supported by Vivado and Quartus Pro; gracefully degraded (in-context synthesis) on backends that don't support it. OOC builds are nodes in the Phase 5 build DAG.
- **Partial reconfiguration**: An advanced FPGA flow producing multiple bitstreams (base + reconfigurable modules) for runtime-swappable accelerators. Requires multi-output builds, build-time variant selection, and dependent build phases. See §17.
- **Platform**: A board-level definition capturing part, clocks, interfaces, pins, and parameters. Projects target platforms. Can be physical (with an FPGA part) or virtual (simulation-only). Vendor-agnostic: works with Xilinx, Intel, Lattice, and OSS toolchains.
- **Profile**: A named overlay on a project that modifies platform, parameters, filesets, or build configuration. Can be simple (named) or dimensional (composable across orthogonal axes).
- **Profile dimension**: An axis of variation (e.g., board, tier, debug level) with a set of choices that compose orthogonally with other dimensions.
- **Project**: A top-level build target that composes components to target a specific platform or part.
- **Reference checkpoint**: A checkpoint from a previous build used by incremental synthesis/implementation to accelerate subsequent builds. The backend selects the most recent successful checkpoint automatically.
- **Reporter**: A plugin that formats build reports for a specific consumer (CI, dashboard, terminal).
- **Simulator**: A plugin that drives a simulation tool (Questa, VCS, Verilator, Icarus, etc.) for verification. Exposes a capability model so the framework can filter incompatible tests. Includes coverage merging across parallel test runs.
- **Simulator capabilities**: A structured declaration of what a simulator supports (UVM, fork/join, interfaces, compilation model). Tests declare requirements; incompatible tests are skipped, not failed.
- **Variant**: A named overlay on a component that provides vendor-specific or context-specific file-set modifications. Selection follows a strict priority: project explicit > profile > platform defaults > no variant. See §3.4.
- **Virtual platform**: A platform with `virtual = true` that has no FPGA part or physical constraints. Used for simulation-only projects, IP development, and prototyping.
- **VLNV**: Vendor/Library/Name/Version — the Xilinx IP identification scheme used by the `vivado_ip` generator. Supports floating (latest), pinned (exact), and range-based version strategies. Other backends use their own IP identification schemes (Quartus: IP name + variant).
- **Workspace**: A repository-level container that groups components, platforms, and projects with shared configuration and dependency resolution.