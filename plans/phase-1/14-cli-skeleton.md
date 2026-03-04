# Task 14: CLI Skeleton

**Prerequisites:** Task 13 complete
**Goal:** Set up the `clap`-based CLI with all Phase 1 subcommands and global flags. Each command dispatches to a handler. `loom --help` shows correct usage.

## Spec Reference
`system_plan.md` §12.1 (UX Principles), §12.2 (Command Reference), §12.4 (Exit Codes), §12.5 (Output Modes)

## File to Implement
`crates/loom-cli/src/main.rs`
`crates/loom-cli/src/commands/mod.rs`
`crates/loom-cli/src/commands/build.rs`
`crates/loom-cli/src/commands/clean.rs`
`crates/loom-cli/src/commands/deps.rs`
`crates/loom-cli/src/commands/env.rs`
`crates/loom-cli/src/commands/lint.rs`

## `main.rs`

```rust
use clap::{Parser, Subcommand};
use std::process;

mod commands;

#[derive(Parser)]
#[command(
    name = "loom",
    about = "FPGA build system",
    version,
    author,
)]
pub struct Cli {
    /// Enable verbose output (-v, -vv for more)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Suppress all output except errors
    #[arg(long, global = true)]
    pub quiet: bool,

    /// Output machine-readable JSON
    #[arg(long, global = true)]
    pub json: bool,

    /// Disable colored output
    #[arg(long, global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Build the FPGA project
    Build(commands::build::BuildArgs),

    /// Remove build artifacts
    Clean(commands::clean::CleanArgs),

    /// Dependency management
    #[command(subcommand)]
    Deps(commands::deps::DepsCommands),

    /// Environment management
    #[command(subcommand)]
    Env(commands::env::EnvCommands),

    /// Validate manifests without building
    Lint(commands::lint::LintArgs),
}

fn main() {
    let cli = Cli::parse();

    // Set up global state from flags
    let ctx = GlobalContext {
        verbose: cli.verbose,
        quiet: cli.quiet,
        json: cli.json,
        no_color: cli.no_color,
    };

    let result = match cli.command {
        Commands::Build(args) => commands::build::run(args, &ctx),
        Commands::Clean(args) => commands::clean::run(args, &ctx),
        Commands::Deps(cmd) => commands::deps::run(cmd, &ctx),
        Commands::Env(cmd) => commands::env::run(cmd, &ctx),
        Commands::Lint(args) => commands::lint::run(args, &ctx),
    };

    match result {
        Ok(()) => process::exit(0),
        Err(err) => {
            display_error(&err, &ctx);
            process::exit(err.exit_code());
        }
    }
}

pub struct GlobalContext {
    pub verbose: u8,
    pub quiet: bool,
    pub json: bool,
    pub no_color: bool,
}

fn display_error(err: &loom_core::error::LoomError, ctx: &GlobalContext) {
    if ctx.json {
        let json = serde_json::json!({
            "error": err.to_string(),
            "exit_code": err.exit_code()
        });
        eprintln!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
    } else {
        let prefix = match err.exit_code() {
            1 => "Build error",
            2 => "Configuration error",
            3 => "Environment error",
            _ => "Error",
        };
        eprintln!("error[{}]: {}", err.exit_code(), prefix);
        eprintln!("{}", err);
    }
}
```

## Command Arg Structs

```rust
// commands/build.rs
use clap::Args;

#[derive(Args)]
pub struct BuildArgs {
    /// Project name (default: auto-detect from current directory)
    #[arg(short = 'p', long)]
    pub project: Option<String>,

    /// Build strategy
    #[arg(long, default_value = "default")]
    pub strategy: String,

    /// Parallel jobs (parsed but ignored in Phase 1)
    #[arg(short = 'j', long)]
    pub jobs: Option<usize>,
}

pub fn run(args: BuildArgs, ctx: &super::super::GlobalContext) -> Result<(), loom_core::error::LoomError> {
    todo!("Implement in Task 15")
}
```

```rust
// commands/clean.rs
use clap::Args;

#[derive(Args)]
pub struct CleanArgs {
    /// Remove all workspace build artifacts
    #[arg(long)]
    pub all: bool,

    /// Project name
    #[arg(short = 'p', long)]
    pub project: Option<String>,
}

pub fn run(args: CleanArgs, ctx: &super::super::GlobalContext) -> Result<(), loom_core::error::LoomError> {
    todo!("Implement in Task 15")
}
```

```rust
// commands/deps.rs
use clap::{Args, Subcommand};

#[derive(Subcommand)]
pub enum DepsCommands {
    /// Show dependency tree
    Tree(DepsTreeArgs),
    /// Re-resolve all dependencies and regenerate lockfile
    Update(DepsUpdateArgs),
}

#[derive(Args)]
pub struct DepsTreeArgs {
    #[arg(short = 'p', long)]
    pub project: Option<String>,
}

#[derive(Args)]
pub struct DepsUpdateArgs {
    /// Specific dependency to update (default: all)
    pub dependency: Option<String>,
}

pub fn run(cmd: DepsCommands, ctx: &super::super::GlobalContext) -> Result<(), loom_core::error::LoomError> {
    todo!("Implement in Task 15")
}
```

```rust
// commands/env.rs
use clap::Subcommand;

#[derive(Subcommand)]
pub enum EnvCommands {
    /// Check tool environment
    Check,
}

pub fn run(cmd: EnvCommands, ctx: &super::super::GlobalContext) -> Result<(), loom_core::error::LoomError> {
    todo!("Implement in Task 15")
}
```

```rust
// commands/lint.rs
use clap::Args;

#[derive(Args)]
pub struct LintArgs {
    /// Project name (default: current directory)
    #[arg(short = 'p', long)]
    pub project: Option<String>,
}

pub fn run(args: LintArgs, ctx: &super::super::GlobalContext) -> Result<(), loom_core::error::LoomError> {
    todo!("Implement in Task 15")
}
```

## `commands/mod.rs`

```rust
pub mod build;
pub mod clean;
pub mod deps;
pub mod env;
pub mod lint;
```

## Tests

```bash
# Manual verification (not unit tests)
cargo run --bin loom -- --help
cargo run --bin loom -- build --help
cargo run --bin loom -- deps tree --help
cargo run --bin loom -- env check --help
cargo run --bin loom -- lint --help
```

Expected output for `loom --help`:
```
FPGA build system

Usage: loom [OPTIONS] <COMMAND>

Commands:
  build  Build the FPGA project
  clean  Remove build artifacts
  deps   Dependency management
  env    Environment management
  lint   Validate manifests without building
  help   Print this message or the help of the given subcommand(s)

Options:
  -v, --verbose...  Enable verbose output (-v, -vv for more)
      --quiet       Suppress all output except errors
      --json        Output machine-readable JSON
      --no-color    Disable colored output
  -h, --help        Print help
  -V, --version     Print version
```

## Done When

- `cargo build --bin loom` succeeds
- `loom --help` shows correct subcommands and global flags
- `loom build --help` shows project, strategy, jobs flags
- `loom deps --help` shows tree and update subcommands
- `loom env --help` shows check subcommand
- All commands return a non-panicking error when called (even with `todo!()` internals)
- Exit code is correct when errors occur
