use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::{Parser, Subcommand};

mod backend_registry;
mod commands;
pub mod ui;

#[derive(Parser)]
#[command(name = "loom", about = "FPGA build system", version, author)]
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

    /// IP management
    #[command(subcommand)]
    Ip(commands::ip::IpCommands),

    /// Validate manifests without building
    Lint(commands::lint::LintArgs),

    /// Export LSP configuration
    Lsp(commands::lsp::LspArgs),

    /// Migration utilities
    #[command(subcommand)]
    Migrate(commands::migrate::MigrateCommands),

    /// Create new component, project, or platform
    #[command(subcommand)]
    New(commands::new::NewCommands),

    /// Package registry operations
    #[command(subcommand)]
    Registry(commands::registry::RegistryCommands),

    /// Show last build report
    Report(commands::report::ReportArgs),

    /// Run simulation
    Sim(commands::sim::SimArgs),

    /// Show project status dashboard
    Status(commands::status::StatusArgs),
}

pub struct GlobalContext {
    pub verbose: u8,
    pub quiet: bool,
    pub json: bool,
    pub no_color: bool,
    pub cancelled: Arc<AtomicBool>,
}

fn main() {
    let cli = Cli::parse();

    if cli.no_color {
        colored::control::set_override(false);
    }

    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = Arc::clone(&cancelled);
    ctrlc::set_handler(move || {
        if cancelled_clone.load(Ordering::Relaxed) {
            // Second Ctrl+C — bail immediately
            process::exit(130);
        }
        cancelled_clone.store(true, Ordering::Relaxed);
    })
    .expect("Failed to set Ctrl+C handler");

    let ctx = GlobalContext {
        verbose: cli.verbose,
        quiet: cli.quiet,
        json: cli.json,
        no_color: cli.no_color,
        cancelled,
    };

    let result = match cli.command {
        Commands::Build(args) => commands::build::run(args, &ctx),
        Commands::Clean(args) => commands::clean::run(args, &ctx),
        Commands::Deps(cmd) => commands::deps::run(cmd, &ctx),
        Commands::Env(cmd) => commands::env::run(cmd, &ctx),
        Commands::Ip(cmd) => commands::ip::run(cmd, &ctx),
        Commands::Lint(args) => commands::lint::run(args, &ctx),
        Commands::Lsp(args) => commands::lsp::run(args, &ctx),
        Commands::Migrate(cmd) => commands::migrate::run(cmd, &ctx),
        Commands::New(cmd) => commands::new::run(cmd, &ctx),
        Commands::Registry(cmd) => commands::registry::run(cmd, &ctx),
        Commands::Report(args) => commands::report::run(args, &ctx),
        Commands::Sim(args) => commands::sim::run(args, &ctx),
        Commands::Status(args) => commands::status::run(args, &ctx),
    };

    match result {
        Ok(()) => process::exit(0),
        Err(err) => {
            display_error(&err, &ctx);
            process::exit(err.exit_code());
        }
    }
}

fn display_error(err: &loom_core::error::LoomError, ctx: &GlobalContext) {
    if ctx.json {
        let json = serde_json::json!({
            "error": err.to_string(),
            "exit_code": err.exit_code()
        });
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&json).unwrap_or_default()
        );
    } else {
        let prefix = match err.exit_code() {
            1 => "Build error",
            2 => "Configuration error",
            3 => "Environment error",
            130 => "Interrupted",
            _ => "Error",
        };
        ui::error_block(err.exit_code(), prefix, &err.to_string());
    }
}
