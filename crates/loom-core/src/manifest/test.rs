use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::component::DependencySpec;

/// A test case declaration in component.toml or project.toml.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TestDecl {
    /// Test name.
    pub name: String,
    /// Top-level testbench module.
    pub top: String,
    /// Description of the test.
    pub description: Option<String>,
    /// Timeout in seconds.
    pub timeout_seconds: Option<u32>,
    /// Tags for filtering (e.g., "smoke", "regression", "nightly").
    #[serde(default)]
    pub tags: Vec<String>,
    /// Simulator requirements for compatibility checking.
    pub requires: Option<TestSimRequirements>,
    /// Simulation options (defines, plusargs, etc.).
    pub sim_options: Option<TestSimOptions>,
    /// Additional dependencies for this test only.
    #[serde(default)]
    pub dependencies: HashMap<String, DependencySpec>,
    /// Test runner: None or "hdl" (default), "cocotb", "verilator" (future).
    /// When absent, the test uses a traditional HDL testbench where `top` is the
    /// testbench module. With "cocotb", `top` is the DUT module and `sources`
    /// lists Python test scripts.
    pub runner: Option<String>,
    /// Source files for non-HDL runners (e.g., Python scripts for cocotb).
    #[serde(default)]
    pub sources: Vec<String>,
}

/// Simulator requirements declared by a test.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TestSimRequirements {
    #[serde(default)]
    pub uvm: bool,
    #[serde(default)]
    pub fork_join: bool,
    #[serde(default)]
    pub force_release: bool,
    #[serde(default)]
    pub systemverilog_full: bool,
    #[serde(default)]
    pub vhdl: bool,
    #[serde(default)]
    pub mixed_language: bool,
    /// Require a build artifact (e.g., "netlist" for gate-level sim).
    pub build_artifact: Option<String>,
}

/// Simulation options embedded in a test declaration.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TestSimOptions {
    #[serde(default)]
    pub defines: Vec<String>,
    #[serde(default)]
    pub plusargs: Vec<String>,
    pub seed: Option<u64>,
}

/// A test suite definition grouping multiple tests.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TestSuiteDecl {
    /// Description of the suite.
    pub description: Option<String>,
    /// Include tests matching these tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Include tests from these components.
    #[serde(default)]
    pub components: Vec<String>,
    /// Include these specific test names.
    #[serde(default)]
    pub tests: Vec<String>,
}

/// Result of a single test case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCaseResult {
    pub name: String,
    pub component: String,
    pub status: TestStatus,
    pub duration_secs: f64,
    pub error_message: Option<String>,
    pub log_path: Option<String>,
}

/// Test status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TestStatus {
    Passed,
    Failed,
    Error,
    Skipped,
}

/// Aggregated report for a test suite run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSuiteReport {
    pub suite: String,
    pub simulator: String,
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub errors: u32,
    pub skipped: u32,
    pub duration_secs: f64,
    pub coverage: Option<crate::plugin::simulator::CoverageReport>,
    pub cases: Vec<TestCaseResult>,
}

impl TestSuiteReport {
    /// Generate JUnit XML for CI.
    pub fn to_junit_xml(&self) -> String {
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str(&format!(
            "<testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\" time=\"{:.1}\">\n",
            self.suite, self.total, self.failed, self.errors, self.skipped, self.duration_secs
        ));

        for case in &self.cases {
            xml.push_str(&format!(
                "  <testcase name=\"{}\" classname=\"{}\" time=\"{:.1}\"",
                case.name, case.component, case.duration_secs
            ));

            match case.status {
                TestStatus::Passed => {
                    xml.push_str("/>\n");
                }
                TestStatus::Failed => {
                    xml.push_str(">\n");
                    let msg = case.error_message.as_deref().unwrap_or("Test failed");
                    xml.push_str(&format!(
                        "    <failure message=\"{}\" type=\"failure\"/>\n",
                        msg
                    ));
                    xml.push_str("  </testcase>\n");
                }
                TestStatus::Error => {
                    xml.push_str(">\n");
                    let msg = case.error_message.as_deref().unwrap_or("Error");
                    xml.push_str(&format!(
                        "    <error message=\"{}\" type=\"error\"/>\n",
                        msg
                    ));
                    xml.push_str("  </testcase>\n");
                }
                TestStatus::Skipped => {
                    xml.push_str(">\n");
                    let msg = case.error_message.as_deref().unwrap_or("Skipped");
                    xml.push_str(&format!("    <skipped message=\"{}\"/>\n", msg));
                    xml.push_str("  </testcase>\n");
                }
            }
        }

        xml.push_str("</testsuite>\n");
        xml
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_test_decl() {
        let toml_str = r#"
name = "basic_loopback"
top = "tb_loopback"
description = "Basic loopback test"
timeout_seconds = 300
tags = ["smoke", "regression"]
[requires]
uvm = false
[sim_options]
defines = ["SIM=1"]
plusargs = ["VERBOSE"]
"#;
        let test: TestDecl = toml::from_str(toml_str).unwrap();
        assert_eq!(test.name, "basic_loopback");
        assert_eq!(test.top, "tb_loopback");
        assert_eq!(test.tags.len(), 2);
        assert!(test.requires.is_some());
        assert!(test.sim_options.is_some());
    }

    #[test]
    fn test_parse_test_suite() {
        let toml_str = r#"
description = "Quick smoke tests"
tags = ["smoke"]
components = ["acmecorp/axi_fifo"]
"#;
        let suite: TestSuiteDecl = toml::from_str(toml_str).unwrap();
        assert_eq!(suite.tags, vec!["smoke"]);
        assert_eq!(suite.components, vec!["acmecorp/axi_fifo"]);
    }

    #[test]
    fn test_suite_report_junit_xml() {
        let report = TestSuiteReport {
            suite: "smoke".to_string(),
            simulator: "xsim".to_string(),
            total: 3,
            passed: 2,
            failed: 1,
            errors: 0,
            skipped: 0,
            duration_secs: 42.5,
            coverage: None,
            cases: vec![
                TestCaseResult {
                    name: "test_a".to_string(),
                    component: "comp_a".to_string(),
                    status: TestStatus::Passed,
                    duration_secs: 10.0,
                    error_message: None,
                    log_path: None,
                },
                TestCaseResult {
                    name: "test_b".to_string(),
                    component: "comp_a".to_string(),
                    status: TestStatus::Failed,
                    duration_secs: 20.0,
                    error_message: Some("Assertion failed".to_string()),
                    log_path: None,
                },
                TestCaseResult {
                    name: "test_c".to_string(),
                    component: "comp_b".to_string(),
                    status: TestStatus::Passed,
                    duration_secs: 12.5,
                    error_message: None,
                    log_path: None,
                },
            ],
        };

        let xml = report.to_junit_xml();
        assert!(xml.contains("testsuite name=\"smoke\""));
        assert!(xml.contains("failures=\"1\""));
        assert!(xml.contains("<failure"));
        assert!(xml.contains("Assertion failed"));
    }

    #[test]
    fn test_suite_report_skipped() {
        let report = TestSuiteReport {
            suite: "compat".to_string(),
            simulator: "verilator".to_string(),
            total: 1,
            passed: 0,
            failed: 0,
            errors: 0,
            skipped: 1,
            duration_secs: 0.0,
            coverage: None,
            cases: vec![TestCaseResult {
                name: "uvm_test".to_string(),
                component: "comp".to_string(),
                status: TestStatus::Skipped,
                duration_secs: 0.0,
                error_message: Some("requires UVM support".to_string()),
                log_path: None,
            }],
        };

        let xml = report.to_junit_xml();
        assert!(xml.contains("skipped=\"1\""));
        assert!(xml.contains("<skipped"));
    }
}
