# Phase 6: Test Organization

**Prerequisites:** Phase 5 complete
**Goal:** Design and implement structured test management for verification workflows.

## Spec Reference
`system_plan.md` §16 (Test Organization)

---

## Overview

Phase 6 starts with a design phase (resolve open questions from spec §16.6), then implements the test manifest model, discovery, aggregation, and CI integration.

## Open Questions to Resolve First (Design Phase)

Before implementing, answer these:

1. **Parameterized tests.** `RANDOM_SEED=${seed}` in sim_options: should seed iteration be a manifest concern (declare N seeds) or a test runner concern (auto-iterate)? Recommended: test runner concern — `--seeds N` flag on `loom sim`.

2. **Gate-level simulation dependencies.** How do test cases declare dependency on a build phase? Proposed: `[tests.requires] build_artifact = "netlist"` which `loom sim` checks before running.

3. **Distributed execution.** Phase 6 scope: local parallelism only (`-j N`). Distributed support is Phase 7+.

4. **Test stability tracking.** Start simple: emit per-test pass/fail history to `.build/<project>/test_history.json`. Don't tackle flakiness detection until proven needed.

---

## Tasks

### Task 01: Test Manifest Model

**Files:** `crates/loom-core/src/manifest/test.rs`

```rust
#[derive(Deserialize)]
pub struct TestDecl {
    pub name: String,
    pub top: String,
    pub description: Option<String>,
    pub timeout_seconds: Option<u32>,
    pub tags: Vec<String>,
    pub requires: Option<SimRequirements>,
    pub sim_options: Option<SimOptions>,
    #[serde(default)]
    pub dependencies: HashMap<String, DependencySpec>,
}

#[derive(Deserialize, Default)]
pub struct SimRequirements {
    pub uvm: bool,
    pub fork_join: bool,
    pub force_release: bool,
    pub systemverilog_full: bool,
}

#[derive(Deserialize)]
pub struct TestSuiteDecl {
    pub tags: Option<Vec<String>>,
    pub components: Option<Vec<String>>,
    pub tests: Option<Vec<String>>,
}
```

Add to `ComponentManifest` and `ProjectManifest`:
```rust
#[serde(rename = "tests", default)]
pub tests: Vec<TestDecl>,

#[serde(default)]
pub test_suites: HashMap<String, TestSuiteDecl>,
```

### Task 02: Test Discovery and Selection

`loom sim --suite <name>` resolves the suite definition and collects matching tests.
`loom sim --filter "axi_*"` uses glob matching on test names.
`loom sim --tag regression` filters by tag.

Each test gets its own "mini-project" resolution: `component + sim fileset + test-only deps`.

### Task 03: Test Execution and Result Aggregation

Run tests in parallel (up to `-j N`). Each test:
1. Compile (or reuse shared compile result for same fileset)
2. Elaborate
3. Simulate (with timeout)
4. Extract results

Aggregate results into a `TestSuiteReport`:
```rust
pub struct TestSuiteReport {
    pub suite: String,
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub errors: u32,
    pub skipped: u32,  // due to capability mismatch
    pub duration_seconds: f64,
    pub coverage: Option<CoverageReport>,
    pub cases: Vec<TestCaseResult>,
}
```

After all tests complete, call `simulator.merge_coverage()` if coverage was requested.

### Task 04: Coverage Merging

For each simulator: implement `merge_coverage()` method.
- Questa: `vcover merge -out merged.ucdb test1.ucdb test2.ucdb ...`
- VCS: `urg -dir test1.vdb test2.vdb -dbname merged`
- Verilator: `verilator_coverage --write merged.info ...`

Coverage percentages from merged database → `TestSuiteReport.coverage`.

### Task 05: CI Integration

`--regression` flag: run all tests, produce:
- JUnit XML: `test-results.xml`
- Coverage report: `coverage-report.html` (if coverage enabled)
- Summary to stdout

GitHub Actions: use `actions/upload-artifact` to save test results. The `GitHubActionsReporter` formats test failures as annotations.

`loom sim --check-compat` reports compatibility table without running anything:
```
Test                    questa   verilator  xsim
basic_loopback          ✓        ✓          ✓
uvm_scoreboard_test     ✓        ✗ (no UVM) ✓
cdc_stress              ✓        ✗ (no fork)✓
```
