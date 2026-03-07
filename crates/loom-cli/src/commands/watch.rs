use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use clap::Args;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};

use loom_core::error::LoomError;

use crate::ui;
use crate::GlobalContext;

#[derive(Args)]
pub struct WatchArgs {
    /// Run simulation instead of build on changes
    #[arg(long)]
    pub sim: bool,

    /// Simulator to use when --sim is active
    #[arg(long, default_value = "xsim")]
    pub tool: String,

    /// Top-level module for simulation
    #[arg(short, long)]
    pub top: Option<String>,

    /// Project name (default: auto-detect)
    #[arg(short = 'p', long)]
    pub project: Option<String>,
}

/// File extensions to watch for changes.
const WATCH_EXTENSIONS: &[&str] = &[
    "sv", "svh", "v", "vh", "vhd", "vhdl", // HDL sources
    "xdc", "sdc", "lpf", "pcf",  // Constraints
    "toml", // Manifests
];

fn should_watch(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    WATCH_EXTENSIONS.contains(&ext.as_str())
}

pub fn run(args: WatchArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;

    // Find workspace root to determine watch directory
    let (workspace_root, _ws_manifest) = loom_core::resolve::find_workspace_root(&cwd)?;

    let mode = if args.sim { "sim" } else { "build" };

    if !ctx.quiet {
        ui::header(&[("\u{00B7}", "watch"), ("\u{00B7}", mode)]);
        eprintln!("  Watching {} for changes...", workspace_root.display());
        eprintln!("  Press Ctrl+C to stop.\n");
    }

    let (tx, rx) = std::sync::mpsc::channel();

    let mut debouncer = new_debouncer(Duration::from_millis(300), tx)
        .map_err(|e| LoomError::Internal(format!("Failed to create file watcher: {}", e)))?;

    // Watch the workspace root recursively
    debouncer
        .watcher()
        .watch(&workspace_root, notify::RecursiveMode::Recursive)
        .map_err(|e| {
            LoomError::Internal(format!(
                "Failed to watch '{}': {}",
                workspace_root.display(),
                e
            ))
        })?;

    loop {
        if ctx.cancelled.load(Ordering::Relaxed) {
            if !ctx.quiet {
                eprintln!("\n  Watch stopped.");
            }
            return Ok(());
        }

        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Ok(events)) => {
                // Filter to relevant file changes
                let changed: Vec<_> = events
                    .iter()
                    .filter(|e| e.kind == DebouncedEventKind::Any && should_watch(&e.path))
                    .collect();

                if changed.is_empty() {
                    continue;
                }

                // Show what triggered the rebuild
                let trigger = changed[0]
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");

                let timestamp = chrono::Local::now().format("%H:%M:%S");
                if !ctx.quiet {
                    eprintln!(
                        "  [{}] Change detected: {} ({} file{})",
                        timestamp,
                        trigger,
                        changed.len(),
                        if changed.len() == 1 { "" } else { "s" }
                    );
                }

                // Run build or sim
                if args.sim {
                    let sim_args = super::sim::SimArgs {
                        top: args.top.clone(),
                        tool: args.tool.clone(),
                        suite: None,
                        filter: None,
                        regression: false,
                        check_compat: false,
                        coverage: false,
                        waves: false,
                        defines: vec![],
                        plusargs: vec![],
                        seed: None,
                        project: args.project.clone(),
                        component: None,
                        junit: None,
                        jobs: 1,
                    };
                    match super::sim::run(sim_args, ctx) {
                        Ok(()) => {}
                        Err(e) => {
                            if !ctx.quiet {
                                eprintln!("  Watch: {}", e);
                            }
                        }
                    }
                } else {
                    let build_args = super::build::BuildArgs {
                        project: args.project.clone(),
                        strategy: "default".to_string(),
                        profile: None,
                        dry_run: false,
                        resume: false,
                        stop_after: None,
                        start_at: None,
                        jobs: None,
                        reference: None,
                        profile_all: false,
                        sweep: false,
                        passthrough: false,
                    };
                    match super::build::run(build_args, ctx) {
                        Ok(()) => {}
                        Err(e) => {
                            if !ctx.quiet {
                                eprintln!("  Watch: {}", e);
                            }
                        }
                    }
                }

                if !ctx.quiet {
                    eprintln!("\n  Watching for changes...");
                }
            }
            Ok(Err(errors)) => {
                eprintln!("  Watch error: {:?}", errors);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Check if cancelled
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(LoomError::Internal(
                    "File watcher disconnected unexpectedly.".to_string(),
                ));
            }
        }
    }
}
