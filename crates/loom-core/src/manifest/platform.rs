use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A platform manifest (`platform.toml`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlatformManifest {
    pub platform: PlatformMeta,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlatformMeta {
    pub name: String,
    pub description: Option<String>,
    pub part: Option<String>,
    pub board: Option<String>,

    /// If true, this platform is simulation-only (no part, no synthesis).
    #[serde(default)]
    pub virtual_platform: bool,

    #[serde(default)]
    pub clocks: HashMap<String, ClockDef>,

    pub constraints: Option<PlatformConstraints>,

    #[serde(default)]
    pub params: HashMap<String, toml::Value>,

    pub variant_defaults: Option<VariantDefaults>,

    pub tool: Option<PlatformToolSpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClockDef {
    pub frequency_mhz: f64,
    pub period_ns: f64,
    pub pin: Option<String>,
    pub standard: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlatformConstraints {
    #[serde(default)]
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VariantDefaults {
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlatformToolSpec {
    pub backend: String,
    pub version: Option<String>,
}

impl PlatformManifest {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.platform.name.is_empty() {
            errors.push("Platform name cannot be empty.".to_string());
        }
        if !self.platform.virtual_platform && self.platform.part.is_none() {
            errors.push(
                "Non-virtual platform must specify a 'part'. Set virtual_platform = true for simulation-only."
                    .to_string(),
            );
        }
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_platform_manifest() {
        let toml_str = r#"
[platform]
name = "zcu104"
description = "Xilinx ZCU104 Evaluation Board"
part = "xczu7ev-ffvc1156-2-e"
board = "xilinx.com:zcu104:part0:1.1"

[platform.tool]
backend = "vivado"
version = "2023.2"

[platform.clocks.sys_clk]
frequency_mhz = 125.0
period_ns = 8.0
pin = "H9"
standard = "LVDS"
description = "125 MHz system clock"

[platform.clocks.user_clk]
frequency_mhz = 300.0
period_ns = 3.333
pin = "G10"
standard = "DIFF_SSTL12"

[platform.constraints]
files = ["constraints/pins.xdc", "constraints/clocks.xdc"]

[platform.params]
ddr4_data_width = 64
pcie_lanes = 4

[platform.variant_defaults]
tags = ["vendor:xilinx"]
"#;
        let manifest: PlatformManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.platform.name, "zcu104");
        assert_eq!(
            manifest.platform.part.as_deref(),
            Some("xczu7ev-ffvc1156-2-e")
        );
        assert!(!manifest.platform.virtual_platform);
        assert_eq!(manifest.platform.clocks.len(), 2);
        assert_eq!(manifest.platform.clocks["sys_clk"].frequency_mhz, 125.0);
        assert_eq!(
            manifest.platform.clocks["sys_clk"].pin.as_deref(),
            Some("H9")
        );
        assert_eq!(
            manifest.platform.constraints.as_ref().unwrap().files.len(),
            2
        );
        assert_eq!(manifest.platform.params.len(), 2);
        assert_eq!(
            manifest.platform.variant_defaults.as_ref().unwrap().tags,
            vec!["vendor:xilinx"]
        );
        assert!(manifest.validate().is_empty());
    }

    #[test]
    fn test_virtual_platform() {
        let toml_str = r#"
[platform]
name = "simulation_generic"
description = "Generic simulation environment"
virtual_platform = true

[platform.clocks.sys_clk]
frequency_mhz = 100.0
period_ns = 10.0

[platform.params]
data_width = 32
"#;
        let manifest: PlatformManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.platform.virtual_platform);
        assert!(manifest.platform.part.is_none());
        assert!(manifest.validate().is_empty());
    }

    #[test]
    fn test_non_virtual_without_part_fails() {
        let toml_str = r#"
[platform]
name = "broken"
"#;
        let manifest: PlatformManifest = toml::from_str(toml_str).unwrap();
        let errors = manifest.validate();
        assert!(!errors.is_empty());
        assert!(errors[0].contains("part"));
    }

    #[test]
    fn test_minimal_platform() {
        let toml_str = r#"
[platform]
name = "simple"
part = "xc7a35t"
"#;
        let manifest: PlatformManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.platform.name, "simple");
        assert!(manifest.platform.clocks.is_empty());
        assert!(manifest.platform.params.is_empty());
        assert!(manifest.validate().is_empty());
    }
}
