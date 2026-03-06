# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Loom is an FPGA build system with a Rust core and Python plugin SDK (Phase 2+). The full specification is in `system_plan.md` (~3100 lines). Detailed implementation plans are in `plans/` with phase-specific subdirectories.

## Build Commands

```bash
cargo build                    # build all crates
cargo test                     # run all tests
cargo test -p loom-core        # test a single crate
cargo test test_name           # run a single test by name
cargo clippy -- -D warnings    # lint
cargo fmt --check              # format check
cargo fmt                      # auto-format
```

## Architecture

Three-layer design (see `system_plan.md` §2):
- **Layer 0:** Vendor-agnostic core — manifests, dependency resolution, build DAG, CLI
- **Layer 1:** Tool plugins — synthesis backends (Vivado, Quartus, yosys+nextpnr)
- **Layer 2:** Convenience abstractions — IP catalog, strategy sweeps, metrics

### Crate Structure

```
crates/
├── loom-cli/       # Binary. CLI entry point (clap derive). One module per command in commands/.
├── loom-core/      # Library. All framework logic:
│   ├── manifest/   #   TOML parsing: component.rs, project.rs, workspace.rs, platform.rs,
│   │               #                 generator.rs, test.rs, common.rs, mod.rs
│   ├── resolve/    #   Dep resolution: resolver.rs, lockfile.rs, graph.rs,
│   │               #                  workspace.rs, platform.rs, registry.rs
│   ├── assemble/   #   File-set assembly: fileset.rs, ordering.rs, template.rs
│   ├── generate/   #   Code gen: dag.rs, node.rs, cache.rs, execute.rs, plugins/
│   ├── build/      #   Pipeline: pipeline.rs, validate.rs, context.rs,
│   │               #             checkpoint.rs, hooks.rs, report.rs, progress.rs
│   ├── sim/        #   Sim runner: compat.rs, discovery.rs, runner.rs
│   ├── plugin/     #   Trait definitions: backend.rs, simulator.rs, generator.rs,
│   │               #                      reporter.rs, mod.rs
│   ├── util.rs
│   └── error.rs    #   LoomError enum, exit code mapping
├── loom-vivado/    # Library. Vivado backend: tcl_gen.rs, executor.rs, env_check.rs, ooc.rs
├── loom-quartus/   # Library. Quartus backend: tcl_gen.rs, executor.rs, env_check.rs
├── loom-yosys/     # Library. yosys+nextpnr backend: synth.rs, pnr.rs, pack.rs, env_check.rs
├── loom-radiant/   # Library. Lattice Radiant backend: tcl_gen.rs, executor.rs, env_check.rs
├── loom-xsim/      # Library. Vivado Simulator: compile.rs, elaborate.rs, simulate.rs, env_check.rs
├── loom-verilator/ # Library. Verilator simulator: env_check.rs
├── loom-icarus/    # Library. Icarus Verilog simulator: env_check.rs
├── loom-questa/    # Library. Siemens Questa simulator: env_check.rs
├── loom-vcs/       # Library. Synopsys VCS simulator: env_check.rs
└── loom-xcelium/   # Library. Cadence Xcelium simulator: env_check.rs
```

### Build Pipeline

Full pipeline: `RESOLVE → GENERATE → ASSEMBLE → VALIDATE → BUILD → REPORT`

Manifests (`component.toml`, `project.toml`, `workspace.toml`) → dependency resolution → lockfile → code generators (DAG) → file-set assembly → backend script generation → tool execution → metrics/report.

## Spec Conventions

- `§N.N` references point to sections in `system_plan.md`
- `[Phase N]` tags indicate when a feature is implemented; untagged = Phase 1
- Code blocks and TOML examples in the spec are the primary specification — implement to match exactly
- If a plan in `plans/` contradicts `system_plan.md`, the spec wins

## Implementation Rules

### Phase Boundaries

All 7 phases are implemented. The `-j` flag enables parallel test execution in `loom sim` (using `std::thread::scope` with a counting semaphore). For `loom build`, `-j` is parsed but not yet wired up.

### Manifests

- Component names use `org/name` format (e.g., `acmecorp/axi_async_fifo`) — enforced from day one
- Dependencies can use short name when unambiguous; lockfile always records full namespaced name
- Constraint `constraint_scope` defaults to `"component"` (not `"global"`)
- Forward slashes in manifest paths; convert to OS-native only when invoking tools

### Error Handling

- Use `thiserror::Error` for `LoomError` enum with `#[error(...)]` derive
- Exit codes: 0=success, 1=build fail, 2=config error, 3=env error, 4=internal
- Error messages must be actionable: "what happened, context, what to do next"
- Validation functions return `Vec<String>` of errors (not fail-fast)

### Rust Conventions

- Edition 2021, workspace dependencies in root `Cargo.toml`
- Serde with `#[serde(...)]` attributes for TOML deserialization
- `petgraph` for dependency graphs, `sha2` for cache keys, `semver` for version parsing
- Unit tests in `#[cfg(test)]` modules, fixtures in `tests/fixtures/`
- Shared test helpers in `crates/loom-core/tests/common/mod.rs`

### Vivado Tcl

Non-project-mode batch execution. The generated Tcl must handle:
- VHDL vs. SystemVerilog `read_*` commands
- Library mapping for VHDL
- Absolute paths with forward slashes (even on Windows)
- `-ref` scoping for component constraints with `constraint_scope = "component"`

## Plans

Follow `plans/README.md` for implementation order. Within a phase, tasks are numbered and must be done in order. Each task file has "Done when" acceptance criteria — verify before moving on.

All phases complete. See `plans/README.md` for the full phase breakdown.
