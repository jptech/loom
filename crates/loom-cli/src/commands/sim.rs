use std::path::PathBuf;

use clap::Args;
use colored::Colorize;

use loom_core::error::LoomError;
use loom_core::generate::execute::{merge_generated_files, run_generate_phase};
use loom_core::generate::registry::PluginRegistry;
use loom_core::manifest::test::{TestStatus, TestSuiteReport};
use loom_core::plugin::simulator::{SimOptions, SimulatorPlugin};
use loom_core::sim::discovery::{
    discover_tests, filter_by_component, filter_tests, DiscoveredTest,
};
use loom_core::sim::runner::{run_test_suite, SimRunnerOptions};

use crate::ui::{self, Icon};
use crate::GlobalContext;

#[derive(Args)]
pub struct SimArgs {
    /// Top-level testbench module (single-test mode)
    #[arg(short, long)]
    pub top: Option<String>,

    /// Simulator to use (auto, xsim, verilator, icarus, questa, vcs, xcelium)
    #[arg(long, default_value = "auto")]
    pub tool: String,

    /// Test suite to run
    #[arg(long)]
    pub suite: Option<String>,

    /// Pattern filter for test names (supports * wildcards)
    #[arg(long)]
    pub filter: Option<String>,

    /// Run all tests (regression mode)
    #[arg(long)]
    pub regression: bool,

    /// Check test/simulator compatibility without running
    #[arg(long)]
    pub check_compat: bool,

    /// Enable coverage collection
    #[arg(long)]
    pub coverage: bool,

    /// Enable waveform dumping (VCD/FST)
    #[arg(long)]
    pub waves: bool,

    /// Additional defines
    #[arg(short = 'D', long = "define")]
    pub defines: Vec<String>,

    /// Additional plusargs
    #[arg(long)]
    pub plusargs: Vec<String>,

    /// Random seed
    #[arg(long)]
    pub seed: Option<u64>,

    /// Project name (default: auto-detect)
    #[arg(short = 'p', long)]
    pub project: Option<String>,

    /// Filter tests to a specific component
    #[arg(long)]
    pub component: Option<String>,

    /// Write JUnit XML to file
    #[arg(long)]
    pub junit: Option<PathBuf>,

    /// Number of tests to run in parallel (default: 1 = sequential)
    #[arg(short = 'j', long = "jobs", default_value = "1")]
    pub jobs: usize,
}

pub fn run(args: SimArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let (sim_name, simulator) = get_simulator(&args.tool)?;
    let auto_detected = args.tool == "auto";

    // Determine the mode of operation.
    // Priority: check_compat > suite > regression > filter > single-test (--top) > auto
    if args.check_compat {
        return run_check_compat_enhanced(&args, simulator.as_ref(), ctx);
    }

    if args.suite.is_some() || args.regression || args.filter.is_some() {
        return run_multi_test(args, simulator.as_ref(), &sim_name, auto_detected, ctx);
    }

    if args.top.is_some() {
        // Explicit --top: single-test mode
        return run_single(args, simulator.as_ref(), &sim_name, auto_detected, ctx);
    }

    // No flags given: auto-detect.  If there are [[tests]] in the workspace,
    // default to regression (run all).  Otherwise fall back to single-test
    // with the legacy tb_{top_module} convention.
    if has_test_definitions(&args, ctx) {
        let args = SimArgs {
            regression: true,
            ..args
        };
        return run_multi_test(args, simulator.as_ref(), &sim_name, auto_detected, ctx);
    }

    run_single(args, simulator.as_ref(), &sim_name, auto_detected, ctx)
}

/// Quick check whether any component in the project defines [[tests]].
fn has_test_definitions(args: &SimArgs, ctx: &GlobalContext) -> bool {
    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let (workspace_root, ws_manifest) = match loom_core::resolve::find_workspace_root(&cwd) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let members = match loom_core::resolve::discover_members(&workspace_root, &ws_manifest) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let all_components = match loom_core::resolve::load_all_components(&members) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let (project_root, project_manifest) = match loom_core::resolve::resolve_project_selection(
        &members,
        args.project.as_deref(),
        Some(&cwd),
        ws_manifest.settings.default_project.as_deref(),
    ) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let source = loom_core::resolve::WorkspaceDependencySource::new(all_components);
    let resolved = match loom_core::resolve::resolve_project(
        project_manifest,
        project_root,
        workspace_root,
        &source,
    ) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let _ = ctx; // suppress unused warning
    !discover_tests(&resolved).is_empty()
}

// ── Single-test mode ────────────────────────────────────────────────

/// Run a single testbench directly (the original `loom sim --top <module>` behavior).
fn run_single(
    args: SimArgs,
    simulator: &dyn SimulatorPlugin,
    sim_name: &str,
    auto_detected: bool,
    ctx: &GlobalContext,
) -> Result<(), LoomError> {
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;

    let (workspace_root, ws_manifest) = loom_core::resolve::find_workspace_root(&cwd)?;
    let members = loom_core::resolve::discover_members(&workspace_root, &ws_manifest)?;
    let all_components = loom_core::resolve::load_all_components(&members)?;
    let (project_root, project_manifest) = loom_core::resolve::resolve_project_selection(
        &members,
        args.project.as_deref(),
        Some(&cwd),
        ws_manifest.settings.default_project.as_deref(),
    )?;

    let source = loom_core::resolve::WorkspaceDependencySource::new(all_components);
    let resolved = loom_core::resolve::resolve_project(
        project_manifest,
        project_root,
        workspace_root.clone(),
        &source,
    )?;

    let build_context =
        loom_core::build::context::BuildContext::new(resolved.clone(), workspace_root);

    // Run generators (e.g. register file codegen) before assembling filesets
    let mut registry = PluginRegistry::with_builtins();
    registry.register("vivado_ip", |_decl| {
        Ok(Box::new(loom_vivado::generator::VivadoIpGenerator))
    });
    registry.register("quartus_ip", |_decl| {
        Ok(Box::new(loom_quartus::generator::QuartusIpGenerator))
    });
    let gen_result = run_generate_phase(&resolved, &build_context, &registry, None)?;

    let mut filesets = loom_core::assemble::assemble_filesets(&resolved)?;
    merge_generated_files(&mut filesets, &gen_result.produced_files);

    let top_module = args
        .top
        .unwrap_or_else(|| format!("tb_{}", resolved.project.project.top_module));

    let options = SimOptions {
        top_module: top_module.clone(),
        defines: args.defines,
        plusargs: args.plusargs,
        seed: args.seed,
        timeout_secs: Some(3600),
        enable_coverage: args.coverage,
        gui: false,
        waves: args.waves,
        extra_args: vec![],
    };

    if !ctx.quiet {
        let sim_label = if auto_detected {
            format!("{} (auto-detected)", sim_name)
        } else {
            sim_name.to_string()
        };
        ui::header(&[
            ("\u{00B7}", "sim"),
            ("\u{00B7}", &sim_label),
            ("\u{00B7}", &format!("top: {}", top_module)),
        ]);
    }

    // Compile
    if !ctx.quiet {
        ui::status(Icon::Dot, "Compile", "");
    }
    let compile_result = simulator.compile(&filesets, &options, &build_context)?;
    if !compile_result.success {
        if !ctx.quiet {
            ui::status(Icon::Cross, "Compile", "failed");
            for err in &compile_result.errors {
                ui::sub_item(err, false);
            }
        }
        return Err(LoomError::BuildFailed {
            phase: "compile".to_string(),
            log_path: compile_result.log_path.clone(),
        });
    }

    // Elaborate
    if !ctx.quiet {
        ui::status(Icon::Dot, "Elaborate", "");
    }
    let elaborate_result =
        simulator.elaborate(&compile_result, &top_module, &options, &build_context)?;
    if !elaborate_result.success {
        if !ctx.quiet {
            ui::status(Icon::Cross, "Elaborate", "failed");
            for err in &elaborate_result.errors {
                ui::sub_item(err, false);
            }
        }
        return Err(LoomError::BuildFailed {
            phase: "elaborate".to_string(),
            log_path: elaborate_result.log_path.clone(),
        });
    }

    // Simulate
    if !ctx.quiet {
        ui::status(Icon::Dot, "Simulate", "");
    }
    let sim_result = simulator.simulate(&elaborate_result, &options, &build_context)?;

    // Extract results
    let report = simulator.extract_results(&sim_result)?;

    if ctx.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    }

    if report.passed {
        if !ctx.quiet {
            ui::summary_pass("Simulation passed", Some(report.duration_secs));
        }
        Ok(())
    } else {
        if !ctx.quiet {
            ui::summary_fail(
                "Simulation failed",
                &format!("{} errors", report.error_count),
            );
        }
        Err(LoomError::BuildFailed {
            phase: "simulate".to_string(),
            log_path: sim_result.log_path.clone(),
        })
    }
}

// ── Multi-test mode (suite / regression / filter) ───────────────────

/// Run multiple tests via suite, regression, or filter mode.
fn run_multi_test(
    args: SimArgs,
    simulator: &dyn SimulatorPlugin,
    sim_name: &str,
    auto_detected: bool,
    ctx: &GlobalContext,
) -> Result<(), LoomError> {
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;

    let (workspace_root, ws_manifest) = loom_core::resolve::find_workspace_root(&cwd)?;
    let members = loom_core::resolve::discover_members(&workspace_root, &ws_manifest)?;
    let all_components = loom_core::resolve::load_all_components(&members)?;
    let (project_root, project_manifest) = loom_core::resolve::resolve_project_selection(
        &members,
        args.project.as_deref(),
        Some(&cwd),
        ws_manifest.settings.default_project.as_deref(),
    )?;

    let source = loom_core::resolve::WorkspaceDependencySource::new(all_components.clone());
    let resolved = loom_core::resolve::resolve_project(
        project_manifest,
        project_root,
        workspace_root.clone(),
        &source,
    )?;

    // Discover all tests from resolved components
    let all_tests = discover_tests(&resolved);

    if all_tests.is_empty() {
        if !ctx.quiet {
            ui::status(
                Icon::Warning,
                "No tests",
                "no [[tests]] found in components",
            );
        }
        return Ok(());
    }

    // Apply component filter if specified
    let tests_in_scope: Vec<DiscoveredTest>;
    let scope_ref: &[DiscoveredTest] = if let Some(ref component) = args.component {
        tests_in_scope = filter_by_component(&all_tests, component)
            .into_iter()
            .cloned()
            .collect();
        if tests_in_scope.is_empty() {
            return Err(LoomError::Internal(format!(
                "No tests found for component '{}'. Check --component name.",
                component
            )));
        }
        &tests_in_scope
    } else {
        &all_tests
    };

    // Select tests based on mode
    let (suite_name, selected): (String, Vec<&DiscoveredTest>) =
        if let Some(ref suite_name) = args.suite {
            // Find the suite declaration across all components
            let suite = find_suite(&resolved, suite_name)?;
            let matched = loom_core::sim::resolve_suite(&suite, scope_ref);
            if matched.is_empty() {
                return Err(LoomError::Internal(format!(
                    "Suite '{}' matched no tests. Check tags/components/names.",
                    suite_name
                )));
            }
            (suite_name.clone(), matched)
        } else if let Some(ref pattern) = args.filter {
            let matched = filter_tests(scope_ref, pattern);
            if matched.is_empty() {
                return Err(LoomError::Internal(format!(
                    "Filter '{}' matched no tests. Use '*' for wildcard.",
                    pattern
                )));
            }
            (format!("filter:{}", pattern), matched)
        } else {
            // Regression mode: all tests
            let refs: Vec<&DiscoveredTest> = scope_ref.iter().collect();
            ("regression".to_string(), refs)
        };

    if !ctx.quiet {
        let sim_label = if auto_detected {
            format!("{} (auto-detected)", sim_name)
        } else {
            sim_name.to_string()
        };
        ui::header(&[
            ("\u{00B7}", "sim"),
            ("\u{00B7}", &sim_label),
            ("\u{00B7}", &format!("{} tests", selected.len())),
        ]);
        let suite_detail = if args.jobs > 1 {
            format!(
                "{} ({} tests) [{} parallel]",
                suite_name,
                selected.len(),
                args.jobs
            )
        } else {
            format!("{} ({} tests)", suite_name, selected.len())
        };
        ui::status(Icon::Dot, "Suite", &suite_detail);
    }

    let runner_options = SimRunnerOptions {
        defines: args.defines,
        plusargs: args.plusargs,
        seed: args.seed,
        enable_coverage: args.coverage,
        waves: args.waves,
        junit_path: args.junit,
        jobs: args.jobs,
    };

    let report = run_test_suite(
        &suite_name,
        &selected,
        simulator,
        &resolved,
        &workspace_root,
        &runner_options,
    )?;

    // Display results
    if ctx.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    }

    if !ctx.quiet {
        display_suite_report(&report);
    }

    if report.failed > 0 || report.errors > 0 {
        Err(LoomError::BuildFailed {
            phase: "simulate".to_string(),
            log_path: PathBuf::from(".build/sim"),
        })
    } else {
        Ok(())
    }
}

// ── Enhanced compatibility check ────────────────────────────────────

/// Show per-test compatibility against the chosen simulator.
fn run_check_compat_enhanced(
    args: &SimArgs,
    simulator: &dyn SimulatorPlugin,
    ctx: &GlobalContext,
) -> Result<(), LoomError> {
    let caps = simulator.capabilities();

    // Print simulator capabilities
    ui::section_header(&format!("Simulator: {}", simulator.plugin_name()));
    let check = |supported: bool| -> Icon {
        if supported {
            Icon::Check
        } else {
            Icon::Cross
        }
    };
    ui::status(check(caps.systemverilog_full), "SystemVerilog", "");
    ui::status(check(caps.vhdl), "VHDL", "");
    ui::status(check(caps.uvm), "UVM", "");
    ui::status(check(caps.fork_join), "fork/join", "");
    ui::status(check(caps.force_release), "force/release", "");
    ui::status(check(caps.code_coverage), "Coverage", "");
    ui::detail_line("Model", &caps.compilation_model);

    // Try to discover tests for per-test compatibility
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;

    if let Ok((workspace_root, ws_manifest)) = loom_core::resolve::find_workspace_root(&cwd) {
        if let Ok(members) = loom_core::resolve::discover_members(&workspace_root, &ws_manifest) {
            if let Ok(all_components) = loom_core::resolve::load_all_components(&members) {
                if let Ok((project_root, project_manifest)) =
                    loom_core::resolve::resolve_project_selection(
                        &members,
                        args.project.as_deref(),
                        Some(&cwd),
                        ws_manifest.settings.default_project.as_deref(),
                    )
                {
                    let source = loom_core::resolve::WorkspaceDependencySource::new(all_components);
                    if let Ok(resolved) = loom_core::resolve::resolve_project(
                        project_manifest,
                        project_root,
                        workspace_root,
                        &source,
                    ) {
                        let tests = discover_tests(&resolved);
                        if !tests.is_empty() {
                            eprintln!();
                            ui::section_header("Test compatibility:");
                            for dt in &tests {
                                let mut reasons = Vec::new();

                                if let Some(reqs) = &dt.test.requires {
                                    reasons.extend(loom_core::sim::compat::check_compatibility(
                                        reqs, &caps,
                                    ));
                                }
                                if let Some(reason) =
                                    loom_core::sim::compat::check_runner_compatibility(
                                        dt.test.runner.as_deref(),
                                        simulator,
                                    )
                                {
                                    reasons.push(reason);
                                }

                                let label = format!("{} ({})", dt.test.name, dt.component_name);
                                if reasons.is_empty() {
                                    ui::status(Icon::Check, &label, "compatible");
                                } else {
                                    ui::status(Icon::Cross, &label, &reasons.join("; "));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if !ctx.quiet {
        eprintln!();
    }

    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────

fn get_simulator(name: &str) -> Result<(String, Box<dyn SimulatorPlugin>), LoomError> {
    match name {
        "auto" => auto_detect_simulator(),
        "xsim" => Ok((name.to_string(), Box::new(loom_xsim::XsimBackend))),
        "verilator" => Ok((name.to_string(), Box::new(loom_verilator::VerilatorBackend))),
        "icarus" => Ok((name.to_string(), Box::new(loom_icarus::IcarusBackend))),
        "questa" => Ok((name.to_string(), Box::new(loom_questa::QuestaBackend))),
        "vcs" => Ok((name.to_string(), Box::new(loom_vcs::VcsBackend))),
        "xcelium" => Ok((name.to_string(), Box::new(loom_xcelium::XceliumBackend))),
        _ => Err(LoomError::ToolNotFound {
            tool: name.to_string(),
            message: format!(
                "Unknown simulator '{}'. Supported: auto, xsim, verilator, icarus, questa, vcs, xcelium.",
                name
            ),
        }),
    }
}

fn auto_detect_simulator() -> Result<(String, Box<dyn SimulatorPlugin>), LoomError> {
    let probes: [(&str, Box<dyn SimulatorPlugin>); 3] = [
        ("xsim", Box::new(loom_xsim::XsimBackend)),
        ("verilator", Box::new(loom_verilator::VerilatorBackend)),
        ("icarus", Box::new(loom_icarus::IcarusBackend)),
    ];
    for (name, sim) in probes {
        if sim.check_environment(None).is_ok() {
            return Ok((name.to_string(), sim));
        }
    }
    Err(LoomError::ToolNotFound {
        tool: "simulator".to_string(),
        message: "No simulator found. Install one of: Vivado (xsim), Verilator, or Icarus Verilog."
            .to_string(),
    })
}

/// Find a named test suite from component manifests in the resolved project.
fn find_suite(
    resolved: &loom_core::resolve::resolver::ResolvedProject,
    suite_name: &str,
) -> Result<loom_core::manifest::test::TestSuiteDecl, LoomError> {
    for comp in &resolved.resolved_components {
        if let Some(suite) = comp.manifest.test_suites.get(suite_name) {
            return Ok(suite.clone());
        }
    }
    Err(LoomError::Internal(format!(
        "Test suite '{}' not found in any component manifest.",
        suite_name
    )))
}

/// Display a test suite report to the terminal.
fn display_suite_report(report: &TestSuiteReport) {
    eprintln!();
    for case in &report.cases {
        let (icon, status_str) = match case.status {
            TestStatus::Passed => (Icon::Check, "pass".green().to_string()),
            TestStatus::Failed => (Icon::Cross, "FAIL".red().to_string()),
            TestStatus::Error => (Icon::Cross, "ERROR".red().to_string()),
            TestStatus::Skipped => (Icon::Warning, "skip".yellow().to_string()),
        };
        let duration = if case.duration_secs > 0.0 {
            format!(" {}", ui::format_duration(case.duration_secs).dimmed())
        } else {
            String::new()
        };
        eprintln!(
            "  {} {:<6} {} ({}){}",
            icon.render(),
            status_str,
            case.name,
            case.component,
            duration,
        );
        if let Some(ref msg) = case.error_message {
            if case.status != TestStatus::Passed {
                eprintln!("           {}", msg.dimmed());
            }
        }
    }

    // Summary line
    let summary = format!(
        "{} total, {} passed, {} failed, {} errors, {} skipped",
        report.total, report.passed, report.failed, report.errors, report.skipped
    );

    if report.failed > 0 || report.errors > 0 {
        ui::summary_fail("Tests failed", &summary);
    } else {
        ui::summary_pass("Tests passed", Some(report.duration_secs));
        eprintln!("    {}", summary.dimmed());
    }
}
