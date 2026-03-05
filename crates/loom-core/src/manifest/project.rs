use std::collections::HashMap;

use serde::Deserialize;

use super::component::{DependencySpec, FileSet};
use super::generator::GeneratorDecl;

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectManifest {
    pub project: ProjectMeta,
    pub target: Option<TargetSpec>,
    #[serde(default)]
    pub filesets: HashMap<String, FileSet>,
    #[serde(default)]
    pub dependencies: HashMap<String, DependencySpec>,
    pub build: Option<BuildConfig>,
    #[serde(rename = "generators", default)]
    pub generators: Vec<GeneratorDecl>,
    /// Simple profiles.
    #[serde(default)]
    pub profiles: HashMap<String, ProfileOverlay>,
    /// Dimensional profiles.
    #[serde(default)]
    pub profile_dimensions: HashMap<String, ProfileDimension>,
    /// Profile exclusion rules.
    pub profile_exclusions: Option<ProfileExclusions>,
}

/// A profile overlay that modifies the base project.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProfileOverlay {
    pub description: Option<String>,
    pub platform: Option<String>,
    #[serde(default)]
    pub params: HashMap<String, toml::Value>,
    pub filesets: Option<ProfileFilesetOverlay>,
    pub build: Option<BuildConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProfileFilesetOverlay {
    pub synth: Option<ProfileFileset>,
    pub sim: Option<ProfileFileset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProfileFileset {
    #[serde(default)]
    pub add_files: Vec<String>,
    #[serde(default)]
    pub add_constraints: Vec<String>,
}

/// A dimensional profile.
#[derive(Debug, Clone, Deserialize)]
pub struct ProfileDimension {
    pub description: Option<String>,
    pub default: String,
    pub choices: HashMap<String, ProfileOverlay>,
}

/// Profile exclusion rules.
#[derive(Debug, Clone, Deserialize)]
pub struct ProfileExclusions {
    #[serde(default)]
    pub rules: Vec<HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub top_module: String,
    pub description: Option<String>,
    /// Platform name (resolved from workspace platforms).
    pub platform: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TargetSpec {
    pub part: String,
    pub backend: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BuildConfig {
    pub build_dir: Option<String>,
    pub default_strategy: Option<String>,
    pub reports: Option<ReportConfig>,
    pub checkpoints: Option<CheckpointConfig>,
    pub timing: Option<TimingConfig>,
}

/// Configuration for which Vivado reports to generate as files.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ReportConfig {
    /// Generate utilization reports (default: true)
    pub utilization: Option<bool>,
    /// Generate timing reports (default: true)
    pub timing: Option<bool>,
    /// Generate power report (default: false)
    pub power: Option<bool>,
    /// Generate DRC report (default: false)
    pub drc: Option<bool>,
}

impl ReportConfig {
    pub fn utilization(&self) -> bool {
        self.utilization.unwrap_or(true)
    }
    pub fn timing(&self) -> bool {
        self.timing.unwrap_or(true)
    }
    pub fn power(&self) -> bool {
        self.power.unwrap_or(false)
    }
    pub fn drc(&self) -> bool {
        self.drc.unwrap_or(false)
    }
}

/// Configuration for which DCP checkpoint files to save.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CheckpointConfig {
    /// Save checkpoint after synthesis (default: false)
    pub post_synth: Option<bool>,
    /// Save checkpoint after optimization (default: false)
    pub post_opt: Option<bool>,
    /// Save checkpoint after placement (default: true)
    pub post_place: Option<bool>,
    /// Save checkpoint after routing (default: true)
    pub post_route: Option<bool>,
}

impl CheckpointConfig {
    pub fn post_synth(&self) -> bool {
        self.post_synth.unwrap_or(false)
    }
    pub fn post_opt(&self) -> bool {
        self.post_opt.unwrap_or(false)
    }
    pub fn post_place(&self) -> bool {
        self.post_place.unwrap_or(true)
    }
    pub fn post_route(&self) -> bool {
        self.post_route.unwrap_or(true)
    }
}

/// Configuration for clock display filtering in the terminal.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TimingConfig {
    /// Hide auto-generated clocks (MMCM/PLL) from the terminal clock table.
    /// Default: false (show all, but dim generated clocks).
    pub hide_generated: Option<bool>,
    /// Exclude specific clock names from the terminal clock table.
    #[serde(default)]
    pub exclude_clocks: Vec<String>,
}

impl TimingConfig {
    /// Whether to hide auto-generated clocks entirely.
    pub fn hide_generated(&self) -> bool {
        self.hide_generated.unwrap_or(false)
    }
}

impl ProjectManifest {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.project.name.is_empty() {
            errors.push("Project name cannot be empty.".to_string());
        }
        if self.project.top_module.is_empty() {
            errors.push("Project top_module cannot be empty.".to_string());
        }
        // Either [target] or platform must be specified
        if self.target.is_none() && self.project.platform.is_none() {
            errors.push(
                "Project must specify either [target] (part + backend) or platform in [project]."
                    .to_string(),
            );
        }

        errors
    }

    pub fn build_dir(&self) -> &str {
        self.build
            .as_ref()
            .and_then(|b| b.build_dir.as_deref())
            .unwrap_or(".build")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_project_manifest() {
        let toml_str = r#"
[project]
name = "radar_processor"
top_module = "radar_top"

[target]
part = "xczu7ev-ffvc1156-2-e"
backend = "vivado"
version = "2023.2"

[filesets.synth]
files = ["src/radar_top.sv"]
constraints = ["constraints/timing.xdc"]

[dependencies]
axi_async_fifo = ">=1.0.0"
"#;
        let manifest: ProjectManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.project.name, "radar_processor");
        assert_eq!(manifest.project.top_module, "radar_top");
        let target = manifest.target.as_ref().unwrap();
        assert_eq!(target.part, "xczu7ev-ffvc1156-2-e");
        assert_eq!(target.backend, "vivado");
        assert!(manifest.dependencies.contains_key("axi_async_fifo"));
    }

    #[test]
    fn test_validate_missing_target() {
        let toml_str = r#"
[project]
name = "my_proj"
top_module = "top"
[filesets.synth]
files = []
"#;
        let manifest: ProjectManifest = toml::from_str(toml_str).unwrap();
        let errors = manifest.validate();
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_build_dir_default() {
        let toml_str = r#"
[project]
name = "test"
top_module = "top"
"#;
        let manifest: ProjectManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.build_dir(), ".build");
    }

    #[test]
    fn test_build_dir_custom() {
        let toml_str = r#"
[project]
name = "test"
top_module = "top"
[build]
build_dir = "output"
"#;
        let manifest: ProjectManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.build_dir(), "output");
    }

    #[test]
    fn test_parse_project_with_platform() {
        let toml_str = r#"
[project]
name = "radar_processor"
top_module = "radar_top"
platform = "zcu104"

[filesets.synth]
files = ["src/radar_top.sv"]
"#;
        let manifest: ProjectManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.project.platform.as_deref(), Some("zcu104"));
        assert!(manifest.target.is_none());
        assert!(manifest.validate().is_empty()); // platform = valid alternative to [target]
    }

    #[test]
    fn test_parse_project_with_profiles() {
        let toml_str = r#"
[project]
name = "radar"
top_module = "radar_top"
platform = "zcu104"

[filesets.synth]
files = ["src/top.sv"]

[profiles.kcu116_port]
description = "Port to KCU116"
platform = "kcu116"

[profiles.reduced]
description = "2-channel version"
[profiles.reduced.params]
num_channels = 2
"#;
        let manifest: ProjectManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.profiles.len(), 2);
        assert_eq!(
            manifest.profiles["kcu116_port"].platform.as_deref(),
            Some("kcu116")
        );
        assert!(manifest.profiles["reduced"]
            .params
            .contains_key("num_channels"));
    }

    #[test]
    fn test_parse_profile_dimensions() {
        let toml_str = r#"
[project]
name = "test"
top_module = "top"
platform = "zcu104"

[profile_dimensions.board]
description = "Target board"
default = "zcu104"

[profile_dimensions.board.choices.zcu104]
platform = "zcu104"

[profile_dimensions.board.choices.kcu116]
platform = "kcu116"

[profile_dimensions.tier]
description = "Feature tier"
default = "full"

[profile_dimensions.tier.choices.full]
[profile_dimensions.tier.choices.full.params]
num_channels = 8

[profile_dimensions.tier.choices.reduced]
[profile_dimensions.tier.choices.reduced.params]
num_channels = 2
"#;
        let manifest: ProjectManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.profile_dimensions.len(), 2);
        assert_eq!(manifest.profile_dimensions["board"].default, "zcu104");
        assert_eq!(manifest.profile_dimensions["board"].choices.len(), 2);
        assert_eq!(manifest.profile_dimensions["tier"].choices.len(), 2);
    }

    #[test]
    fn test_parse_fixture_project() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/simple_project/projects/my_design/project.toml");
        let content = std::fs::read_to_string(&path).unwrap();
        let manifest: ProjectManifest = toml::from_str(&content).unwrap();
        assert_eq!(manifest.project.name, "my_design");
        assert!(manifest.validate().is_empty());
    }

    #[test]
    fn test_report_config_defaults() {
        let config = ReportConfig::default();
        assert!(config.utilization());
        assert!(config.timing());
        assert!(!config.power());
        assert!(!config.drc());
    }

    #[test]
    fn test_checkpoint_config_defaults() {
        let config = CheckpointConfig::default();
        assert!(!config.post_synth());
        assert!(!config.post_opt());
        assert!(config.post_place());
        assert!(config.post_route());
    }

    #[test]
    fn test_parse_build_config_with_reports() {
        let toml_str = r#"
[project]
name = "test"
top_module = "top"

[target]
part = "xc7a35t"
backend = "vivado"

[build.reports]
utilization = true
timing = true
power = true
drc = false

[build.checkpoints]
post_synth = true
post_place = true
post_route = true
"#;
        let manifest: ProjectManifest = toml::from_str(toml_str).unwrap();
        let reports = manifest.build.as_ref().unwrap().reports.as_ref().unwrap();
        assert!(reports.utilization());
        assert!(reports.timing());
        assert!(reports.power());
        assert!(!reports.drc());

        let checkpoints = manifest
            .build
            .as_ref()
            .unwrap()
            .checkpoints
            .as_ref()
            .unwrap();
        assert!(checkpoints.post_synth());
        assert!(checkpoints.post_place());
        assert!(checkpoints.post_route());
        assert!(!checkpoints.post_opt()); // not set, defaults to false
    }

    #[test]
    fn test_timing_config_defaults() {
        let config = TimingConfig::default();
        assert!(!config.hide_generated());
        assert!(config.exclude_clocks.is_empty());
    }

    #[test]
    fn test_parse_timing_config() {
        let toml_str = r#"
[project]
name = "test"
top_module = "top"

[target]
part = "xc7a35t"
backend = "vivado"

[build.timing]
hide_generated = true
exclude_clocks = ["clk_fb", "clk_div2"]
"#;
        let manifest: ProjectManifest = toml::from_str(toml_str).unwrap();
        let timing = manifest.build.as_ref().unwrap().timing.as_ref().unwrap();
        assert!(timing.hide_generated());
        assert_eq!(timing.exclude_clocks.len(), 2);
        assert!(timing.exclude_clocks.contains(&"clk_fb".to_string()));
        assert!(timing.exclude_clocks.contains(&"clk_div2".to_string()));
    }

    #[test]
    fn test_parse_timing_config_partial() {
        let toml_str = r#"
[project]
name = "test"
top_module = "top"

[target]
part = "xc7a35t"
backend = "vivado"

[build.timing]
exclude_clocks = ["clk_fb"]
"#;
        let manifest: ProjectManifest = toml::from_str(toml_str).unwrap();
        let timing = manifest.build.as_ref().unwrap().timing.as_ref().unwrap();
        assert!(!timing.hide_generated()); // default false
        assert_eq!(timing.exclude_clocks.len(), 1);
    }
}
