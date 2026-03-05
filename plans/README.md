# Loom — Agentic Implementation Plans

This directory contains the actionable implementation plans for building the Loom FPGA build system. Each plan is scoped to be implementable by an AI coding agent in one or a few sessions.

## How to Use These Plans

1. **Always read `system_plan.md` first.** The plans reference `§N.N` section markers from that document. When a plan says "see §3.1," open `system_plan.md` and find Section 3.1.
2. **Work phases in order.** Each phase builds on the previous. Do not start Phase 2 until Phase 1 acceptance criteria are met.
3. **Work tasks in order within a phase.** Tasks within a phase have dependencies and are numbered for this reason.
4. **Verify acceptance criteria before moving on.** Each task has explicit "Done when" criteria. Do not skip verification.
5. **The spec is authoritative.** If a plan contradicts `system_plan.md`, the spec wins. Flag inconsistencies.

## Phase Overview

| Phase | Goal | Tasks | Status |
|---|---|---|---|
| **Phase 1** | End-to-end build: manifests → Vivado → bitstream | 15 | Complete |
| **Phase 2** | Generators, caching, CLI polish, Windows | 15 | Complete |
| **Phase 3** | Platforms, variants, profiles, OOC synthesis | 6 | Complete |
| **Phase 4** | Reporting, CI, hooks, Quartus backend | 8 | Complete |
| **Phase 5** | Simulation, yosys backend, strategy sweeps | 8 | Complete |
| **Phase 6** | Test organization (design + implement) | 5 | Complete |
| **Phase 7** | Ecosystem, registry, additional backends | 5 | Complete |

## Repository Layout (Target)

```
loom-fpga/                     ← Cargo workspace root
├── Cargo.toml
├── crates/
│   ├── loom-cli/              ← Binary: CLI entry point
│   ├── loom-core/             ← Library: all framework logic
│   └── loom-vivado/           ← Library: Vivado backend (Rust in Phase 1)
├── plans/                     ← This directory
│   ├── phase-1/
│   ├── phase-2/
│   └── ...
└── tests/                     ← Integration test fixtures
    └── fixtures/
        ├── simple_project/    ← Minimal test workspace
        └── multi_component/   ← Workspace with dependency graph
```

## Technology Stack

| Layer | Technology |
|---|---|
| Core binary | Rust (`clap`, `toml`, `serde`, `petgraph`, `sha2`) |
| Plugin host | PyO3 (Phase 2+) |
| Plugin SDK | Python `loom.plugin` ABC classes (Phase 2+) |
| Configuration | TOML |
| Build artifacts | `.build/` (gitignored) |
| Lockfile | `loom.lock` (committed to VCS) |

## Key Dependencies (Cargo)

```toml
# loom-core
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
semver = "1"
petgraph = "0.6"       # dependency graph
sha2 = "0.10"          # cache key hashing
glob = "0.3"           # workspace member discovery
walkdir = "2"          # directory traversal
thiserror = "1"        # error types
anyhow = "1"           # error context
chrono = { version = "0.4", features = ["serde"] }

# loom-cli
clap = { version = "4", features = ["derive"] }
colored = "2"          # terminal color
indicatif = "0.17"     # progress bars (Phase 2)

# loom-vivado (Phase 1: pure Rust)
# Phase 2+: pyo3 = { version = "0.21", features = ["auto-initialize"] }
```
