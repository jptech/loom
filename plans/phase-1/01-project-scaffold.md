# Task 01: Project Scaffold

**Prerequisites:** None
**Goal:** Create the Cargo workspace with all three crates. `cargo build` succeeds with no code logic yet.

## Spec Reference
`system_plan.md` §0.2 (Implementation Stack), §15 Phase 1 Crate Structure

## Files to Create

```
loom-fpga/
├── Cargo.toml                    ← workspace manifest
├── crates/
│   ├── loom-cli/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs           ← stub: fn main() {}
│   ├── loom-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs            ← stub: pub mod manifest; pub mod error;
│   └── loom-vivado/
│       ├── Cargo.toml
│       └── src/
│           └── lib.rs            ← stub
├── tests/
│   └── fixtures/
│       └── simple_project/       ← see phase-1/README.md for contents
└── .gitignore                    ← include: /target/, /.build/
```

## Workspace `Cargo.toml`

```toml
[workspace]
members = ["crates/loom-cli", "crates/loom-core", "crates/loom-vivado"]
resolver = "2"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
semver = "1"
thiserror = "1"
anyhow = "1"
chrono = { version = "0.4", features = ["serde"] }
sha2 = "0.10"
hex = "0.4"
glob = "0.3"
walkdir = "2"
petgraph = "0.6"
clap = { version = "4", features = ["derive"] }
```

## `crates/loom-core/Cargo.toml`

```toml
[package]
name = "loom-core"
version = "0.1.0"
edition = "2021"

[dependencies]
serde.workspace = true
serde_json.workspace = true
toml.workspace = true
semver.workspace = true
thiserror.workspace = true
anyhow.workspace = true
chrono.workspace = true
sha2.workspace = true
hex.workspace = true
glob.workspace = true
walkdir.workspace = true
petgraph.workspace = true
```

## `crates/loom-cli/Cargo.toml`

```toml
[package]
name = "loom-cli"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "loom"
path = "src/main.rs"

[dependencies]
loom-core = { path = "../loom-core" }
loom-vivado = { path = "../loom-vivado" }
clap.workspace = true
anyhow.workspace = true
serde_json.workspace = true
```

## `crates/loom-vivado/Cargo.toml`

```toml
[package]
name = "loom-vivado"
version = "0.1.0"
edition = "2021"

[dependencies]
loom-core = { path = "../loom-core" }
anyhow.workspace = true
thiserror.workspace = true
serde.workspace = true
serde_json.workspace = true
```

## Directory Structure for `loom-core/src/`

```
src/
├── lib.rs
├── error.rs          ← LoomError (stub for now)
├── manifest/
│   ├── mod.rs
│   ├── component.rs
│   ├── project.rs
│   ├── workspace.rs
│   └── common.rs
├── resolve/
│   ├── mod.rs
│   ├── resolver.rs
│   ├── lockfile.rs
│   └── graph.rs
├── assemble/
│   ├── mod.rs
│   ├── fileset.rs
│   └── ordering.rs
├── build/
│   ├── mod.rs
│   ├── pipeline.rs
│   ├── validate.rs
│   └── context.rs
└── plugin/
    ├── mod.rs
    ├── backend.rs
    └── generator.rs
```

Create these files as empty stubs with `// TODO` comments and the module declarations.

`loom-core/src/lib.rs`:
```rust
pub mod error;
pub mod manifest;
pub mod resolve;
pub mod assemble;
pub mod build;
pub mod plugin;
```

## Done When

- `cargo build --workspace` succeeds with no errors (warnings OK)
- `cargo test --workspace` runs (0 tests pass, that's fine)
- `tests/fixtures/simple_project/` directory exists with files described in phase-1/README.md
