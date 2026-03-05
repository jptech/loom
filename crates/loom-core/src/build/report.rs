use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::LoomError;
use crate::plugin::backend::BuildResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildReport {
    pub project: String,
    pub timestamp: String,
    pub tool: ToolInfo,
    pub target: TargetInfo,
    pub strategy: String,
    pub status: BuildStatus,
    pub git: Option<GitInfo>,
    pub metrics: BuildMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetInfo {
    pub part: String,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildStatus {
    pub success: bool,
    pub exit_code: i32,
    pub phases_completed: Vec<String>,
    pub failure_phase: Option<String>,
    pub failure_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitInfo {
    pub commit: String,
    pub branch: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildMetrics {
    pub timing: Option<TimingMetrics>,
    pub utilization: Option<UtilizationMetrics>,
    pub power: Option<PowerMetrics>,
    pub duration_secs: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingMetrics {
    pub wns: f64,
    pub tns: f64,
    pub whs: f64,
    pub ths: f64,
    pub failing_endpoints: u32,
    /// Per-clock-domain timing data (populated from timing report).
    #[serde(default)]
    pub clocks: Vec<ClockTiming>,
}

/// Per-clock-domain timing information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockTiming {
    /// Clock name (e.g., "clk_sys", "sys_clk").
    pub name: String,
    /// Constraint period in nanoseconds (from Clock Summary).
    pub period_ns: Option<f64>,
    /// Target frequency in MHz (= 1000 / period_ns).
    pub frequency_mhz: Option<f64>,
    /// Worst negative slack (setup) in ns.
    pub wns: f64,
    /// Total negative slack (setup) in ns.
    pub tns: f64,
    /// Worst hold slack in ns.
    pub whs: f64,
    /// Total hold slack in ns.
    pub ths: f64,
    /// Number of failing timing endpoints.
    pub failing_endpoints: u32,
    /// Total timing endpoints.
    pub total_endpoints: u32,
    /// Realized Fmax in MHz = 1000 / (period_ns - WNS).
    /// Positive WNS (slack) → faster than target; negative WNS (violation) → slower.
    pub achieved_mhz: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilizationMetrics {
    pub lut_used: u64,
    pub lut_available: u64,
    pub lut_percent: f64,
    pub ff_used: u64,
    pub ff_available: u64,
    pub ff_percent: f64,
    pub bram_used: u64,
    pub bram_available: u64,
    pub bram_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerMetrics {
    pub total_watts: f64,
    pub dynamic_watts: f64,
    pub static_watts: f64,
}

impl BuildReport {
    /// Create a report from a build result.
    pub fn from_build_result(
        project_name: &str,
        backend_name: &str,
        backend_version: &str,
        part: &str,
        strategy: &str,
        result: &BuildResult,
        workspace_root: &Path,
    ) -> Self {
        Self {
            project: project_name.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            tool: ToolInfo {
                name: backend_name.to_string(),
                version: backend_version.to_string(),
            },
            target: TargetInfo {
                part: part.to_string(),
                backend: backend_name.to_string(),
            },
            strategy: strategy.to_string(),
            status: BuildStatus {
                success: result.success,
                exit_code: result.exit_code,
                phases_completed: result.phases_completed.clone(),
                failure_phase: result.failure_phase.clone(),
                failure_message: result.failure_message.clone(),
            },
            git: get_git_info(workspace_root),
            metrics: BuildMetrics::default(),
        }
    }

    /// Write the report as JSON to a file.
    pub fn write_to_file(&self, path: &Path) -> Result<(), LoomError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| LoomError::Io {
                path: parent.to_owned(),
                source: e,
            })?;
        }

        let content =
            serde_json::to_string_pretty(self).map_err(|e| LoomError::Internal(e.to_string()))?;

        std::fs::write(path, content).map_err(|e| LoomError::Io {
            path: path.to_owned(),
            source: e,
        })
    }

    /// Load a report from a JSON file.
    pub fn load_from_file(path: &Path) -> Result<Self, LoomError> {
        let content = std::fs::read_to_string(path).map_err(|e| LoomError::Io {
            path: path.to_owned(),
            source: e,
        })?;
        serde_json::from_str(&content)
            .map_err(|e| LoomError::Internal(format!("Failed to parse build report: {}", e)))
    }
}

/// Report file path within a build directory.
pub fn report_path(build_dir: &Path) -> PathBuf {
    build_dir.join("report.json")
}

/// Extract git info from a workspace directory.
fn get_git_info(workspace_root: &Path) -> Option<GitInfo> {
    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())?;

    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(workspace_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            let b = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if b == "HEAD" {
                None
            } else {
                Some(b)
            }
        })
        .unwrap_or(None);

    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(workspace_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    Some(GitInfo {
        commit,
        branch,
        dirty,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_serialization() {
        let result = BuildResult {
            success: true,
            exit_code: 0,
            log_paths: vec![],
            bitstream_path: Some(PathBuf::from("/build/top.bit")),
            phases_completed: vec![
                "synthesis".to_string(),
                "place".to_string(),
                "route".to_string(),
            ],
            failure_phase: None,
            failure_message: None,
        };

        let report = BuildReport::from_build_result(
            "test_project",
            "vivado",
            "2023.2",
            "xc7a35t",
            "default",
            &result,
            Path::new("/tmp"),
        );

        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("test_project"));
        assert!(json.contains("vivado"));
        assert!(json.contains("xc7a35t"));
    }

    #[test]
    fn test_report_write_and_load() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = BuildResult {
            success: true,
            exit_code: 0,
            log_paths: vec![],
            bitstream_path: None,
            phases_completed: vec!["synthesis".to_string()],
            failure_phase: None,
            failure_message: None,
        };

        let report = BuildReport::from_build_result(
            "test",
            "vivado",
            "2023.2",
            "xc7a35t",
            "default",
            &result,
            Path::new("/tmp"),
        );

        let path = tmp.path().join("report.json");
        report.write_to_file(&path).unwrap();

        let loaded = BuildReport::load_from_file(&path).unwrap();
        assert_eq!(loaded.project, "test");
        assert!(loaded.status.success);
    }
}
