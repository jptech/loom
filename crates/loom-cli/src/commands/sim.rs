use clap::Args;

use loom_core::error::LoomError;
use loom_core::plugin::simulator::{SimOptions, SimulatorPlugin};

use crate::GlobalContext;

#[derive(Args)]
pub struct SimArgs {
    /// Top-level testbench module
    #[arg(short, long)]
    pub top: Option<String>,

    /// Simulator to use (xsim, verilator)
    #[arg(long, default_value = "xsim")]
    pub tool: String,

    /// Test suite to run
    #[arg(long)]
    pub suite: Option<String>,

    /// Pattern filter for test names
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

    /// Additional defines
    #[arg(short = 'D', long = "define")]
    pub defines: Vec<String>,

    /// Additional plusargs
    #[arg(long)]
    pub plusargs: Vec<String>,

    /// Random seed
    #[arg(long)]
    pub seed: Option<u64>,
}

pub fn run(args: SimArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let simulator = get_simulator(&args.tool)?;

    if args.check_compat {
        return run_check_compat(simulator.as_ref(), ctx);
    }

    // Resolve project and assemble filesets
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;

    let (workspace_root, ws_manifest) = loom_core::resolve::find_workspace_root(&cwd)?;
    let members = loom_core::resolve::discover_members(&workspace_root, &ws_manifest)?;
    let all_components = loom_core::resolve::load_all_components(&members)?;
    let (_project_root, project_manifest) = loom_core::resolve::find_project(&members, None)?;

    let source = loom_core::resolve::WorkspaceDependencySource::new(all_components);
    let resolved = loom_core::resolve::resolve_project(
        project_manifest,
        _project_root,
        workspace_root.clone(),
        &source,
    )?;

    let filesets = loom_core::assemble::assemble_filesets(&resolved)?;
    let build_context =
        loom_core::build::context::BuildContext::new(resolved.clone(), workspace_root);

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
        extra_args: vec![],
    };

    if !ctx.quiet {
        eprintln!(
            "  Simulating with {} (top: {})...",
            simulator.plugin_name(),
            top_module
        );
    }

    // Step 1: Compile
    if !ctx.quiet {
        eprintln!("  Compiling...");
    }
    let compile_result = simulator.compile(&filesets, &options, &build_context)?;
    if !compile_result.success {
        eprintln!("  Compilation failed:");
        for err in &compile_result.errors {
            eprintln!("    {}", err);
        }
        return Err(LoomError::BuildFailed {
            phase: "compile".to_string(),
            log_path: compile_result.log_path.clone(),
        });
    }

    // Step 2: Elaborate
    if !ctx.quiet {
        eprintln!("  Elaborating...");
    }
    let elaborate_result =
        simulator.elaborate(&compile_result, &top_module, &options, &build_context)?;
    if !elaborate_result.success {
        eprintln!("  Elaboration failed:");
        for err in &elaborate_result.errors {
            eprintln!("    {}", err);
        }
        return Err(LoomError::BuildFailed {
            phase: "elaborate".to_string(),
            log_path: elaborate_result.log_path.clone(),
        });
    }

    // Step 3: Simulate
    if !ctx.quiet {
        eprintln!("  Running simulation...");
    }
    let sim_result = simulator.simulate(&elaborate_result, &options, &build_context)?;

    // Step 4: Extract results
    let report = simulator.extract_results(&sim_result)?;

    if ctx.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    } else {
        let status = if report.passed { "PASSED" } else { "FAILED" };
        eprintln!("  Simulation {}: {:.1}s", status, report.duration_secs);
        if report.error_count > 0 {
            eprintln!("  Errors: {}", report.error_count);
        }
    }

    if report.passed {
        Ok(())
    } else {
        Err(LoomError::BuildFailed {
            phase: "simulate".to_string(),
            log_path: sim_result.log_path.clone(),
        })
    }
}

fn get_simulator(name: &str) -> Result<Box<dyn SimulatorPlugin>, LoomError> {
    match name {
        "xsim" => Ok(Box::new(loom_xsim::XsimBackend)),
        "verilator" => Ok(Box::new(loom_verilator::VerilatorBackend)),
        "icarus" => Ok(Box::new(loom_icarus::IcarusBackend)),
        "questa" => Ok(Box::new(loom_questa::QuestaBackend)),
        "vcs" => Ok(Box::new(loom_vcs::VcsBackend)),
        "xcelium" => Ok(Box::new(loom_xcelium::XceliumBackend)),
        _ => Err(LoomError::ToolNotFound {
            tool: name.to_string(),
            message: format!(
                "Unknown simulator '{}'. Supported: xsim, verilator, icarus, questa, vcs, xcelium.",
                name
            ),
        }),
    }
}

fn run_check_compat(
    simulator: &dyn SimulatorPlugin,
    _ctx: &GlobalContext,
) -> Result<(), LoomError> {
    let caps = simulator.capabilities();
    println!("Simulator: {}", simulator.plugin_name());
    println!("  SystemVerilog: {}", caps.systemverilog_full);
    println!("  VHDL:          {}", caps.vhdl);
    println!("  UVM:           {}", caps.uvm);
    println!("  fork/join:     {}", caps.fork_join);
    println!("  force/release: {}", caps.force_release);
    println!("  Coverage:      {}", caps.code_coverage);
    println!("  Model:         {}", caps.compilation_model);
    Ok(())
}
