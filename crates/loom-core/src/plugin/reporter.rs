use crate::build::report::BuildReport;
use crate::error::LoomError;

/// Output from a reporter plugin.
#[derive(Debug, Clone)]
pub struct ReporterOutput {
    pub content: Vec<u8>,
    pub suggested_filename: Option<String>,
    pub content_type: String,
}

/// Trait for report formatting plugins.
pub trait ReporterPlugin: Send + Sync {
    fn plugin_name(&self) -> &str;

    fn format_report(
        &self,
        report: &BuildReport,
        options: &toml::Value,
    ) -> Result<ReporterOutput, LoomError>;
}

/// Console (human-readable) reporter.
pub struct ConsoleReporter;

impl ReporterPlugin for ConsoleReporter {
    fn plugin_name(&self) -> &str {
        "console"
    }

    fn format_report(
        &self,
        report: &BuildReport,
        _options: &toml::Value,
    ) -> Result<ReporterOutput, LoomError> {
        let mut out = String::new();

        let status_str = if report.status.success {
            "PASSED"
        } else {
            "FAILED"
        };
        let duration = report
            .metrics
            .duration_secs
            .map(|d| format!(" ({:.0}s)", d))
            .unwrap_or_default();

        out.push_str(&format!(
            "Build: {} ({}) — {}{}\n",
            report.project, report.target.part, status_str, duration
        ));

        if let Some(ref timing) = report.metrics.timing {
            out.push_str("\n  Timing:\n");
            let wns_ok = if timing.wns >= 0.0 { " ✓" } else { " ✗" };
            let whs_ok = if timing.whs >= 0.0 { " ✓" } else { " ✗" };
            out.push_str(&format!("    WNS:  {:.3}ns{}\n", timing.wns, wns_ok));
            out.push_str(&format!("    WHS:  {:.3}ns{}\n", timing.whs, whs_ok));
        }

        if let Some(ref util) = report.metrics.utilization {
            out.push_str("\n  Utilization:\n");
            out.push_str(&format!(
                "    LUT:   {:5.1}%  {}\n",
                util.lut_percent,
                bar(util.lut_percent)
            ));
            out.push_str(&format!(
                "    FF:    {:5.1}%  {}\n",
                util.ff_percent,
                bar(util.ff_percent)
            ));
            out.push_str(&format!(
                "    BRAM:  {:5.1}%  {}\n",
                util.bram_percent,
                bar(util.bram_percent)
            ));
        }

        Ok(ReporterOutput {
            content: out.into_bytes(),
            suggested_filename: None,
            content_type: "text/plain".to_string(),
        })
    }
}

fn bar(percent: f64) -> String {
    let filled = (percent / 5.0).round() as usize;
    let empty = 20usize.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

/// JSON reporter — outputs raw report as JSON.
pub struct JsonReporter;

impl ReporterPlugin for JsonReporter {
    fn plugin_name(&self) -> &str {
        "json"
    }

    fn format_report(
        &self,
        report: &BuildReport,
        _options: &toml::Value,
    ) -> Result<ReporterOutput, LoomError> {
        let json =
            serde_json::to_string_pretty(report).map_err(|e| LoomError::Internal(e.to_string()))?;
        Ok(ReporterOutput {
            content: json.into_bytes(),
            suggested_filename: Some("report.json".to_string()),
            content_type: "application/json".to_string(),
        })
    }
}

/// GitHub Actions reporter — outputs annotations.
pub struct GitHubActionsReporter;

impl ReporterPlugin for GitHubActionsReporter {
    fn plugin_name(&self) -> &str {
        "github-actions"
    }

    fn format_report(
        &self,
        report: &BuildReport,
        _options: &toml::Value,
    ) -> Result<ReporterOutput, LoomError> {
        let mut out = String::new();

        if report.status.success {
            let mut details = vec![report.project.clone()];
            if let Some(ref timing) = report.metrics.timing {
                details.push(format!("WNS={:.3}ns", timing.wns));
            }
            if let Some(ref util) = report.metrics.utilization {
                details.push(format!("LUT={:.1}%", util.lut_percent));
                details.push(format!("FF={:.1}%", util.ff_percent));
            }
            out.push_str(&format!(
                "::notice title=Build Passed::{}\n",
                details.join(", ")
            ));
        } else {
            let msg = report
                .status
                .failure_message
                .as_deref()
                .unwrap_or("Build failed");
            out.push_str(&format!(
                "::error title=Build Failed::{}: {}\n",
                report.project, msg
            ));
        }

        Ok(ReporterOutput {
            content: out.into_bytes(),
            suggested_filename: None,
            content_type: "text/plain".to_string(),
        })
    }
}

/// JUnit XML reporter for CI systems.
pub struct JUnitReporter;

impl ReporterPlugin for JUnitReporter {
    fn plugin_name(&self) -> &str {
        "junit"
    }

    fn format_report(
        &self,
        report: &BuildReport,
        _options: &toml::Value,
    ) -> Result<ReporterOutput, LoomError> {
        let status = if report.status.success {
            "pass"
        } else {
            "fail"
        };
        let duration = report.metrics.duration_secs.unwrap_or(0.0);

        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str(&format!(
            "<testsuite name=\"{}\" tests=\"1\" failures=\"{}\" time=\"{:.1}\">\n",
            report.project,
            if report.status.success { 0 } else { 1 },
            duration
        ));
        xml.push_str(&format!(
            "  <testcase name=\"build\" classname=\"{}\" time=\"{:.1}\"",
            report.project, duration
        ));

        if report.status.success {
            xml.push_str("/>\n");
        } else {
            xml.push_str(">\n");
            let msg = report
                .status
                .failure_message
                .as_deref()
                .unwrap_or("Build failed");
            xml.push_str(&format!(
                "    <failure message=\"{}\" type=\"{}\"/>\n",
                msg, status
            ));
            xml.push_str("  </testcase>\n");
        }

        xml.push_str("</testsuite>\n");

        Ok(ReporterOutput {
            content: xml.into_bytes(),
            suggested_filename: Some("report.xml".to_string()),
            content_type: "application/xml".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::report::*;

    fn make_test_report(success: bool) -> BuildReport {
        BuildReport {
            project: "test_project".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            tool: ToolInfo {
                name: "vivado".to_string(),
                version: "2023.2".to_string(),
            },
            target: TargetInfo {
                part: "xc7a35t".to_string(),
                backend: "vivado".to_string(),
            },
            strategy: "default".to_string(),
            status: BuildStatus {
                success,
                exit_code: if success { 0 } else { 1 },
                phases_completed: vec!["synthesis".to_string()],
                failure_phase: if success {
                    None
                } else {
                    Some("route".to_string())
                },
                failure_message: if success {
                    None
                } else {
                    Some("Timing not met".to_string())
                },
            },
            git: None,
            metrics: BuildMetrics {
                timing: Some(TimingMetrics {
                    wns: 0.142,
                    tns: 0.0,
                    whs: 0.021,
                    ths: 0.0,
                    failing_endpoints: 0,
                }),
                utilization: Some(UtilizationMetrics {
                    lut_used: 4230,
                    lut_available: 10000,
                    lut_percent: 42.3,
                    ff_used: 2810,
                    ff_available: 10000,
                    ff_percent: 28.1,
                    bram_used: 65,
                    bram_available: 100,
                    bram_percent: 65.0,
                }),
                power: None,
                duration_secs: Some(2305.0),
            },
        }
    }

    #[test]
    fn test_console_reporter() {
        let report = make_test_report(true);
        let options = toml::Value::Table(toml::map::Map::new());
        let output = ConsoleReporter.format_report(&report, &options).unwrap();
        let text = String::from_utf8(output.content).unwrap();
        assert!(text.contains("PASSED"));
        assert!(text.contains("0.142ns"));
        assert!(text.contains("42.3%"));
    }

    #[test]
    fn test_json_reporter() {
        let report = make_test_report(true);
        let options = toml::Value::Table(toml::map::Map::new());
        let output = JsonReporter.format_report(&report, &options).unwrap();
        let text = String::from_utf8(output.content).unwrap();
        assert!(text.contains("test_project"));
        assert!(text.contains("xc7a35t"));
    }

    #[test]
    fn test_github_actions_reporter() {
        let report = make_test_report(true);
        let options = toml::Value::Table(toml::map::Map::new());
        let output = GitHubActionsReporter
            .format_report(&report, &options)
            .unwrap();
        let text = String::from_utf8(output.content).unwrap();
        assert!(text.contains("::notice"));
        assert!(text.contains("Build Passed"));
    }

    #[test]
    fn test_github_actions_failure() {
        let report = make_test_report(false);
        let options = toml::Value::Table(toml::map::Map::new());
        let output = GitHubActionsReporter
            .format_report(&report, &options)
            .unwrap();
        let text = String::from_utf8(output.content).unwrap();
        assert!(text.contains("::error"));
        assert!(text.contains("Build Failed"));
    }

    #[test]
    fn test_junit_reporter() {
        let report = make_test_report(true);
        let options = toml::Value::Table(toml::map::Map::new());
        let output = JUnitReporter.format_report(&report, &options).unwrap();
        let text = String::from_utf8(output.content).unwrap();
        assert!(text.contains("<testsuite"));
        assert!(text.contains("failures=\"0\""));
    }

    #[test]
    fn test_junit_failure() {
        let report = make_test_report(false);
        let options = toml::Value::Table(toml::map::Map::new());
        let output = JUnitReporter.format_report(&report, &options).unwrap();
        let text = String::from_utf8(output.content).unwrap();
        assert!(text.contains("<failure"));
        assert!(text.contains("Timing not met"));
    }

    #[test]
    fn test_utilization_bar() {
        assert_eq!(bar(50.0).chars().count(), 20);
        assert_eq!(bar(0.0).chars().count(), 20);
        assert_eq!(bar(100.0).chars().count(), 20);
    }
}
