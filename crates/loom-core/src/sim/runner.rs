use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

use crate::assemble::assemble_filesets;
use crate::build::context::BuildContext;
use crate::error::LoomError;
use crate::manifest::test::{TestCaseResult, TestStatus, TestSuiteReport};
use crate::plugin::simulator::{SimOptions, SimulatorPlugin};
use crate::resolve::resolver::ResolvedProject;
use crate::sim::compat::{check_compatibility, check_runner_compatibility};
use crate::sim::discovery::DiscoveredTest;

/// Options passed from the CLI to the test runner.
#[derive(Debug, Clone, Default)]
pub struct SimRunnerOptions {
    /// Extra defines from CLI (merged with per-test defines).
    pub defines: Vec<String>,
    /// Extra plusargs from CLI.
    pub plusargs: Vec<String>,
    /// Seed override (overrides per-test seed).
    pub seed: Option<u64>,
    /// Enable coverage collection.
    pub enable_coverage: bool,
    /// Enable waveform dumping.
    pub waves: bool,
    /// Path to write JUnit XML output.
    pub junit_path: Option<PathBuf>,
    /// Number of tests to run in parallel (1 = sequential).
    pub jobs: usize,
}

/// Run a set of discovered tests as a suite, returning an aggregated report.
///
/// Tests are run through compile → elaborate → simulate → extract results.
/// When `options.jobs > 1`, tests run in parallel with up to N concurrent
/// threads.  Incompatible tests are skipped upfront.  Coverage databases
/// are collected for merging after all tests complete.
pub fn run_test_suite(
    suite_name: &str,
    tests: &[&DiscoveredTest],
    simulator: &dyn SimulatorPlugin,
    resolved: &ResolvedProject,
    workspace_root: &Path,
    options: &SimRunnerOptions,
) -> Result<TestSuiteReport, LoomError> {
    let suite_start = std::time::Instant::now();
    let caps = simulator.capabilities();

    // Assemble filesets once (shared across all tests).
    let filesets = assemble_filesets(resolved)?;
    let base_context = BuildContext::new(resolved.clone(), workspace_root.to_path_buf());

    // ── Phase 1: pre-filter ─────────────────────────────────────────
    // Separate skipped tests (printed immediately) from runnable tests.
    let mut cases: Vec<TestCaseResult> = Vec::new();
    let mut runnable: Vec<(&DiscoveredTest, SimOptions)> = Vec::new();

    for dt in tests {
        // Check simulator compatibility
        if let Some(reqs) = &dt.test.requires {
            let incompatibilities = check_compatibility(reqs, &caps);
            if !incompatibilities.is_empty() {
                let reason = format!("incompatible: {}", incompatibilities.join(", "));
                eprintln!(
                    "  [skip] {} ({}) — {}",
                    dt.test.name, dt.component_name, reason
                );
                cases.push(TestCaseResult {
                    name: dt.test.name.clone(),
                    component: dt.component_name.clone(),
                    status: TestStatus::Skipped,
                    duration_secs: 0.0,
                    error_message: Some(reason),
                    log_path: None,
                });
                continue;
            }
        }

        // Check runner compatibility
        if let Some(reason) = check_runner_compatibility(dt.test.runner.as_deref(), simulator) {
            eprintln!(
                "  [skip] {} ({}) — {}",
                dt.test.name, dt.component_name, reason
            );
            cases.push(TestCaseResult {
                name: dt.test.name.clone(),
                component: dt.component_name.clone(),
                status: TestStatus::Skipped,
                duration_secs: 0.0,
                error_message: Some(reason),
                log_path: None,
            });
            continue;
        }

        let sim_options = build_sim_options(dt, options);
        runnable.push((dt, sim_options));
    }

    // ── Phase 2: execute ────────────────────────────────────────────
    let num_jobs = options.jobs.max(1);
    let mut coverage_dbs: Vec<PathBuf> = Vec::new();

    if num_jobs <= 1 || runnable.len() <= 1 {
        run_sequential(
            &runnable,
            simulator,
            &filesets,
            &base_context,
            &mut cases,
            &mut coverage_dbs,
        )?;
    } else {
        run_parallel(
            num_jobs,
            &runnable,
            simulator,
            &filesets,
            &base_context,
            &mut cases,
            &mut coverage_dbs,
        );
    }

    // ── Phase 3: aggregate ──────────────────────────────────────────
    // Merge coverage if requested
    let coverage = if options.enable_coverage && !coverage_dbs.is_empty() {
        let output_path = workspace_root.join(".build").join("coverage_merged");
        simulator.merge_coverage(&coverage_dbs, &output_path).ok()
    } else {
        None
    };

    let total = cases.len() as u32;
    let passed = cases
        .iter()
        .filter(|c| c.status == TestStatus::Passed)
        .count() as u32;
    let failed = cases
        .iter()
        .filter(|c| c.status == TestStatus::Failed)
        .count() as u32;
    let errors = cases
        .iter()
        .filter(|c| c.status == TestStatus::Error)
        .count() as u32;
    let skipped = cases
        .iter()
        .filter(|c| c.status == TestStatus::Skipped)
        .count() as u32;

    let report = TestSuiteReport {
        suite: suite_name.to_string(),
        simulator: simulator.plugin_name().to_string(),
        total,
        passed,
        failed,
        errors,
        skipped,
        duration_secs: suite_start.elapsed().as_secs_f64(),
        coverage,
        cases,
    };

    // Write JUnit XML if requested
    if let Some(ref junit_path) = options.junit_path {
        let xml = report.to_junit_xml();
        if let Some(parent) = junit_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(junit_path, xml).map_err(|e| LoomError::Io {
            path: junit_path.clone(),
            source: e,
        })?;
    }

    Ok(report)
}

// ── Sequential runner ───────────────────────────────────────────────

/// Run tests one at a time with detailed per-phase progress output.
fn run_sequential(
    runnable: &[(&DiscoveredTest, SimOptions)],
    simulator: &dyn SimulatorPlugin,
    filesets: &crate::assemble::fileset::AssembledFilesets,
    base_context: &BuildContext,
    cases: &mut Vec<TestCaseResult>,
    coverage_dbs: &mut Vec<PathBuf>,
) -> Result<(), LoomError> {
    let total = runnable.len();

    for (idx, (dt, sim_options)) in runnable.iter().enumerate() {
        let test_start = std::time::Instant::now();

        eprintln!(
            "  [{}/{}] {} ({})",
            idx + 1,
            total,
            dt.test.name,
            dt.component_name
        );

        let mut test_context = base_context.clone();
        test_context.build_dir = base_context.build_dir.join("tests").join(&dt.test.name);

        let result = run_single_test(dt, simulator, filesets, sim_options, &test_context, true);
        let elapsed = test_start.elapsed().as_secs_f64();

        collect_result(dt, result, elapsed, cases, coverage_dbs);
    }

    Ok(())
}

// ── Parallel runner ─────────────────────────────────────────────────

/// Run tests concurrently (up to `num_jobs` at a time) with compact output.
fn run_parallel(
    num_jobs: usize,
    runnable: &[(&DiscoveredTest, SimOptions)],
    simulator: &dyn SimulatorPlugin,
    filesets: &crate::assemble::fileset::AssembledFilesets,
    base_context: &BuildContext,
    cases: &mut Vec<TestCaseResult>,
    coverage_dbs: &mut Vec<PathBuf>,
) {
    let total = runnable.len();
    let completed = AtomicUsize::new(0);
    let output_lock = Mutex::new(());

    // Counting semaphore: (active_count, condvar)
    let semaphore = (Mutex::new(0usize), Condvar::new());

    let results: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = runnable
            .iter()
            .enumerate()
            .map(|(idx, (dt, sim_options))| {
                let sem = &semaphore;
                let completed = &completed;
                let out = &output_lock;

                let mut test_context = base_context.clone();
                test_context.build_dir = base_context.build_dir.join("tests").join(&dt.test.name);

                s.spawn(move || {
                    // Acquire slot
                    {
                        let (lock, cvar) = sem;
                        let mut active = lock.lock().unwrap();
                        while *active >= num_jobs {
                            active = cvar.wait(active).unwrap();
                        }
                        *active += 1;
                    }

                    let test_start = std::time::Instant::now();
                    let result =
                        run_single_test(dt, simulator, filesets, sim_options, &test_context, false);
                    let elapsed = test_start.elapsed().as_secs_f64();

                    // Release slot
                    {
                        let (lock, cvar) = sem;
                        let mut active = lock.lock().unwrap();
                        *active -= 1;
                        cvar.notify_one();
                    }

                    // Print completion atomically
                    let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    {
                        let _guard = out.lock().unwrap();
                        let status = match &result {
                            Ok(tr) if tr.passed => "pass",
                            Ok(_) => "FAIL",
                            Err(_) => "ERROR",
                        };
                        eprintln!(
                            "  [{}/{}] {} {} ({}) {:.1}s",
                            done, total, status, dt.test.name, dt.component_name, elapsed
                        );
                    }

                    (idx, dt, result, elapsed)
                })
            })
            .collect();

        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    // Collect results in original order
    let mut indexed: Vec<_> = results.into_iter().collect();
    indexed.sort_by_key(|(idx, _, _, _)| *idx);

    for (_idx, dt, result, elapsed) in indexed {
        collect_result(dt, result, elapsed, cases, coverage_dbs);
    }
}

// ── Shared helpers ──────────────────────────────────────────────────

/// Convert a `SingleTestResult` (or error) into a `TestCaseResult` and
/// append it to the `cases` / `coverage_dbs` vectors.
fn collect_result(
    dt: &DiscoveredTest,
    result: Result<SingleTestResult, LoomError>,
    elapsed: f64,
    cases: &mut Vec<TestCaseResult>,
    coverage_dbs: &mut Vec<PathBuf>,
) {
    match result {
        Ok(tr) => {
            if let Some(db) = tr.coverage_db {
                coverage_dbs.push(db);
            }
            let error_message = if tr.passed {
                None
            } else {
                let phase = tr.failure_phase.as_deref().unwrap_or("simulate");
                let detail = tr.failure_details.first().cloned().unwrap_or_default();
                if detail.is_empty() {
                    Some(format!("failed during {}", phase))
                } else {
                    Some(format!("failed during {}: {}", phase, detail))
                }
            };
            cases.push(TestCaseResult {
                name: dt.test.name.clone(),
                component: dt.component_name.clone(),
                status: if tr.passed {
                    TestStatus::Passed
                } else {
                    TestStatus::Failed
                },
                duration_secs: elapsed,
                error_message,
                log_path: Some(tr.log_path.to_string_lossy().to_string()),
            });
        }
        Err(e) => {
            cases.push(TestCaseResult {
                name: dt.test.name.clone(),
                component: dt.component_name.clone(),
                status: TestStatus::Error,
                duration_secs: elapsed,
                error_message: Some(e.to_string()),
                log_path: None,
            });
        }
    }
}

/// Build SimOptions for a single test, merging per-test options with CLI overrides.
fn build_sim_options(dt: &DiscoveredTest, cli_options: &SimRunnerOptions) -> SimOptions {
    let test_opts = dt.test.sim_options.as_ref();

    let mut defines: Vec<String> = test_opts.map(|o| o.defines.clone()).unwrap_or_default();
    defines.extend(cli_options.defines.iter().cloned());

    let mut plusargs: Vec<String> = test_opts.map(|o| o.plusargs.clone()).unwrap_or_default();
    plusargs.extend(cli_options.plusargs.iter().cloned());

    // CLI seed overrides per-test seed
    let seed = cli_options.seed.or_else(|| test_opts.and_then(|o| o.seed));

    let timeout = dt.test.timeout_seconds.map(|t| t as u64);

    let mut extra_args = Vec::new();

    // Set up cocotb-specific arguments if runner is "cocotb"
    if dt.test.runner.as_deref() == Some("cocotb") {
        // cocotb module name derived from the first Python source file
        if let Some(source) = dt.test.sources.first() {
            let module_name = Path::new(source)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("test");
            // cocotb 2.x uses COCOTB_TEST_MODULES; set both for 1.x compat
            extra_args.push(format!("COCOTB_TEST_MODULES={}", module_name));
            extra_args.push(format!("COCOTB_MODULE={}", module_name));
            extra_args.push(format!("COCOTB_TOPLEVEL={}", dt.test.top));

            // PYTHONPATH: directory containing the Python test module
            let source_path = dt.component_path.join(source);
            if let Some(parent) = source_path.parent() {
                extra_args.push(format!("PYTHONPATH={}", parent.display()));
            }
        }
    }

    SimOptions {
        top_module: dt.test.top.clone(),
        defines,
        plusargs,
        seed,
        timeout_secs: timeout,
        enable_coverage: cli_options.enable_coverage,
        gui: false,
        waves: cli_options.waves,
        extra_args,
    }
}

/// Result of running a single test through all phases.
struct SingleTestResult {
    passed: bool,
    log_path: PathBuf,
    coverage_db: Option<PathBuf>,
    /// Which phase caused the failure (compile / elaborate / simulate).
    failure_phase: Option<String>,
    /// Error details from the failing phase.
    failure_details: Vec<String>,
}

/// Run a single test through compile → elaborate → simulate.
///
/// When `verbose` is true (sequential mode), per-phase progress is printed
/// to stderr in real time.  When false (parallel mode), execution is silent.
fn run_single_test(
    _dt: &DiscoveredTest,
    simulator: &dyn SimulatorPlugin,
    filesets: &crate::assemble::fileset::AssembledFilesets,
    options: &SimOptions,
    context: &BuildContext,
    verbose: bool,
) -> Result<SingleTestResult, LoomError> {
    use std::io::Write;

    // Compile
    if verbose {
        eprint!("        compile ...");
        std::io::stderr().flush().ok();
    }
    let compile_result = simulator.compile(filesets, options, context)?;
    if !compile_result.success {
        if verbose {
            eprintln!(" FAILED");
        }
        return Ok(SingleTestResult {
            passed: false,
            log_path: compile_result.log_path,
            coverage_db: None,
            failure_phase: Some("compile".to_string()),
            failure_details: compile_result.errors,
        });
    }
    if verbose {
        eprintln!(" ok");
    }

    // Elaborate
    if verbose {
        eprint!("        elaborate ...");
        std::io::stderr().flush().ok();
    }
    let elaborate_result =
        simulator.elaborate(&compile_result, &options.top_module, options, context)?;
    if !elaborate_result.success {
        if verbose {
            eprintln!(" FAILED");
        }
        return Ok(SingleTestResult {
            passed: false,
            log_path: elaborate_result.log_path,
            coverage_db: None,
            failure_phase: Some("elaborate".to_string()),
            failure_details: elaborate_result.errors,
        });
    }
    if verbose {
        eprintln!(" ok");
    }

    // Simulate
    if verbose {
        eprint!("        simulate ...");
        std::io::stderr().flush().ok();
    }
    let sim_start = std::time::Instant::now();
    let sim_result = simulator.simulate(&elaborate_result, options, context)?;
    let sim_elapsed = sim_start.elapsed().as_secs_f64();

    // Extract results
    let report = simulator.extract_results(&sim_result)?;

    if verbose {
        if report.passed {
            eprintln!(" ok ({:.1}s)", sim_elapsed);
        } else {
            eprintln!(" FAILED ({:.1}s)", sim_elapsed);
        }
    }

    Ok(SingleTestResult {
        passed: report.passed,
        log_path: sim_result.log_path,
        coverage_db: sim_result.coverage_db,
        failure_phase: if report.passed {
            None
        } else {
            Some("simulate".to_string())
        },
        failure_details: if report.passed {
            vec![]
        } else {
            sim_result.errors
        },
    })
}
