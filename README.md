# Loom

Loom is a build system for FPGA projects. It manages multi-component workspaces, resolves dependencies, assembles filesets, and drives vendor toolchains (Vivado, Quartus, yosys/nextpnr, Radiant) through a unified CLI.

## Key ideas

- **Declarative manifests** — TOML files (`component.toml`, `project.toml`, `workspace.toml`) describe your design; Loom figures out the rest.
- **Workspace-native** — First-class support for monorepos with shared IP libraries and multiple target projects.
- **Vendor-agnostic core** — One project structure works across Xilinx, Intel, Lattice, and open-source toolchains.
- **Deterministic builds** — Lockfile pins exact dependency versions. Cache keys track inputs so generators only re-run when needed.

## Quick start

```bash
# Create a new workspace
mkdir my_fpga && cd my_fpga
loom new workspace

# Create a reusable component
loom new component lib/myorg/uart

# Create a project targeting a specific FPGA
loom new project projects/top_design

# Validate manifests
loom lint

# Build
loom build

# Run simulation
loom sim --tool verilator
```

## Project structure

A typical Loom workspace looks like this:

```
my_fpga/
├── workspace.toml              # Workspace root
├── loom.lock                   # Lockfile (auto-generated)
├── lib/
│   ├── myorg/axi_common/
│   │   ├── component.toml
│   │   └── rtl/
│   │       └── axi_pkg.sv
│   └── myorg/uart/
│       ├── component.toml
│       ├── rtl/
│       │   └── uart.sv
│       └── constraints/
│           └── uart_timing.xdc
├── projects/
│   └── top_design/
│       ├── project.toml
│       ├── src/
│       │   └── top.sv
│       └── constraints/
│           └── pins.xdc
└── platforms/
    └── zcu104.toml             # Board/platform definition
```

## Manifest reference

### workspace.toml

Defines the workspace root and which directories contain components and projects.

```toml
[workspace]
name = "my_fpga"
members = ["lib/*", "projects/*"]

[settings]
default_tool_version = "2023.2"
build_dir = ".build"
```

### component.toml

Describes a reusable IP component. Component names must use `org/name` format.

```toml
[component]
name = "myorg/uart"
version = "1.2.0"
description = "UART transceiver with configurable baud rate"

[filesets.synth]
files = ["rtl/uart.sv", "rtl/uart_rx.sv", "rtl/uart_tx.sv"]
constraints = ["constraints/uart_timing.xdc"]
constraint_scope = "component"      # scoped to this component (default)

[filesets.sim]
files = ["tb/uart_tb.sv"]
include_synth = true
defines = ["SIMULATION"]

[dependencies]
axi_common = ">=1.0.0"

# Detailed dependency with variant selection
[dependencies.memory_ctrl]
version = ">=2.0.0"
variant = "xilinx"
```

#### Variants

Components can declare vendor- or platform-specific variants:

```toml
[variants.xilinx]
description = "Xilinx UltraRAM-based implementation"
tags = ["vendor:xilinx"]

[variants.xilinx.filesets.synth]
add_files = ["rtl/xilinx/uram_wrapper.sv"]
add_constraints = ["constraints/xilinx/uram.xdc"]

[[variants.xilinx.generators]]
name = "mig_ip"
plugin = "vivado_ip"
[variants.xilinx.generators.config]
vlnv = "xilinx.com:ip:mig_7series"
```

#### Generators

Components can declare code generators that produce HDL from other inputs:

```toml
[[generators]]
name = "regmap"
plugin = "command"
command = "python scripts/gen_regs.py"
inputs = ["regs.yaml"]
outputs = ["generated/regs.sv"]
fileset = "synth"
cacheable = true
```

#### Tests

```toml
[[tests]]
name = "basic_loopback"
top = "tb_loopback"
timeout_seconds = 300
tags = ["smoke", "regression"]

[tests.requires]
uvm = false

[tests.sim_options]
defines = ["SIM=1"]
plusargs = ["VERBOSE"]
```

### project.toml

Defines a build target — which top module, FPGA part, and backend to use.

```toml
[project]
name = "my_design"
top_module = "top"

[target]
part = "xczu7ev-ffvc1156-2-e"
backend = "vivado"
version = "2023.2"

[filesets.synth]
files = ["src/top.sv"]
constraints = ["constraints/pins.xdc"]

[dependencies]
uart = ">=1.0.0"
axi_common = ">=1.0.0"

[build]
build_dir = ".build"
```

#### Profiles

Projects can define build profiles for different configurations:

```toml
[profiles.debug]
description = "Debug build with ILA"
[profiles.debug.filesets.synth]
add_files = ["debug/ila_wrapper.sv"]
add_constraints = ["debug/ila_timing.xdc"]
```

### platform.toml

Describes a board or target platform, separating board-level concerns from design logic.

```toml
[platform]
name = "zcu104"
description = "Xilinx ZCU104 Evaluation Kit"
part = "xczu7ev-ffvc1156-2-e"
board = "xilinx.com:zcu104:part0:1.1"

[platform.clocks.sys_clk]
frequency_mhz = 125.0
pin = "H9"
standard = "LVDS"

[platform.constraints]
files = ["constraints/zcu104_pins.xdc"]

[platform.params]
ddr4_data_width = 64

[platform.tool]
backend = "vivado"
version = "2023.2"
```

## CLI commands

| Command | Description |
|---------|-------------|
| `loom build` | Build the FPGA project (synthesis through bitstream) |
| `loom sim` | Run simulation (xsim, verilator, icarus, questa, vcs, xcelium) |
| `loom lint` | Validate all manifests without building |
| `loom clean` | Remove build artifacts |
| `loom deps tree` | Print the dependency graph |
| `loom deps lock` | Regenerate the lockfile |
| `loom env check` | Verify tool installations |
| `loom env shell` | Open a subshell with tool environment configured |
| `loom env dockerfile` | Generate a Dockerfile for CI builds |
| `loom new component` | Scaffold a new component |
| `loom new project` | Scaffold a new project |
| `loom new platform` | Scaffold a new platform definition |
| `loom ip list` | List all IP instances across the workspace |
| `loom ip upgrade` | Check for IP version upgrades |
| `loom report` | Show the last build report |
| `loom lsp` | Export LSP configuration for editor integration |
| `loom migrate xci-to-toml` | Convert Vivado `.xci` files to TOML generator config |
| `loom registry search` | Search the package registry |
| `loom registry publish` | Publish a component to the registry |
| `loom registry install` | Install a component from the registry |

### Global flags

```
-v, --verbose       Increase verbosity (-v, -vv)
    --quiet         Suppress all output except errors
    --json          Output machine-readable JSON
    --no-color      Disable colored output
```

### Build flags

```
loom build [OPTIONS]

    --backend <NAME>       Override backend (vivado, quartus, yosys, radiant)
    --part <PART>          Override target part
    --profile <NAME>       Select a build profile
    --sweep                Run strategy sweep
    --reference <PATH>     Reference build for comparison
    -j <N>                 Parallelism (parsed, currently unused)
```

### Simulation flags

```
loom sim [OPTIONS]

    --tool <NAME>          Simulator (xsim, verilator, icarus, questa, vcs, xcelium)
    --top <MODULE>         Top-level testbench module
    --suite <NAME>         Run a test suite
    --filter <PATTERN>     Filter tests by name pattern
    --regression           Run all tests
    --check-compat         Check simulator compatibility without running
    --coverage             Enable coverage collection
    -D <DEFINE>            Additional defines
    --plusargs <ARG>        Additional plusargs
    --seed <N>             Random seed
```

## Supported backends

| Backend | Toolchain | Constraint format | Status |
|---------|-----------|-------------------|--------|
| `vivado` | AMD/Xilinx Vivado | XDC | Full |
| `quartus` | Intel Quartus Prime | SDC, QSF | Full |
| `yosys` | yosys + nextpnr | PCF, LPF, CST | Full (ice40, ECP5, Gowin) |
| `radiant` | Lattice Radiant | PDC, LPF | Full |

## Supported simulators

| Simulator | Tool | SystemVerilog | VHDL | UVM | Coverage |
|-----------|------|:---:|:---:|:---:|:---:|
| `xsim` | Xilinx Vivado Simulator | Full | Yes | No | Code |
| `verilator` | Verilator | Partial | No | No | Code |
| `icarus` | Icarus Verilog | Partial | No | No | No |
| `questa` | Siemens Questa | Full | Yes | Yes | All |
| `vcs` | Synopsys VCS | Full | Yes | Yes | All |
| `xcelium` | Cadence Xcelium | Full | Yes | Yes | All |

## Build pipeline

Loom executes a linear build pipeline:

```
RESOLVE → GENERATE → ASSEMBLE → VALIDATE → BUILD → REPORT
```

1. **Resolve** — Walk workspace, parse manifests, resolve dependencies, check lockfile.
2. **Generate** — Run code generators (register maps, IP cores, block designs).
3. **Assemble** — Collect HDL files and constraints in dependency order.
4. **Validate** — Check that all referenced files exist and the tool environment is ready.
5. **Build** — Generate backend-specific scripts (Tcl, yosys commands) and execute.
6. **Report** — Extract metrics, write build report, return exit code.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Build failure (synthesis, place, route, simulation) |
| 2 | Configuration error (bad manifest, missing dependency, version conflict) |
| 3 | Environment error (tool not found, version mismatch) |
| 4 | Internal error |

## Development

```bash
cargo build                    # Build all crates
cargo test                     # Run all tests (184 tests)
cargo clippy -- -D warnings    # Lint
cargo fmt                      # Format
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for internal architecture details.

## License

See LICENSE file for details.
