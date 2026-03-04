use std::path::PathBuf;

use crate::error::LoomError;
use crate::manifest::platform::PlatformManifest;

use super::workspace::MemberPath;

/// Resolved platform data merged from platform.toml.
#[derive(Debug, Clone)]
pub struct ResolvedPlatform {
    pub name: String,
    pub part: Option<String>,
    pub board: Option<String>,
    pub backend: Option<String>,
    pub backend_version: Option<String>,
    pub virtual_platform: bool,
    pub clocks: std::collections::HashMap<String, crate::manifest::platform::ClockDef>,
    pub params: std::collections::HashMap<String, toml::Value>,
    pub constraint_files: Vec<PathBuf>,
    pub variant_tags: Vec<String>,
    /// The directory containing platform.toml (for resolving relative constraint paths).
    pub platform_root: PathBuf,
}

/// Find and load a platform by name from discovered workspace members.
pub fn find_platform(
    members: &[MemberPath],
    platform_name: &str,
) -> Result<(PathBuf, PlatformManifest), LoomError> {
    use super::workspace::MemberKind;

    for member in members {
        if member.kind != MemberKind::Platform {
            continue;
        }
        let manifest_path = member.path.join("platform.toml");
        if manifest_path.exists() {
            if let Ok(manifest) = crate::manifest::load_platform_manifest(&manifest_path) {
                if manifest.platform.name == platform_name {
                    return Ok((member.path.clone(), manifest));
                }
            }
        }
    }

    Err(LoomError::Internal(format!(
        "Platform '{}' not found in workspace. Check that a platform.toml with [platform].name = \"{}\" exists.",
        platform_name, platform_name
    )))
}

/// Resolve a platform manifest into a ResolvedPlatform.
pub fn resolve_platform(
    manifest: &PlatformManifest,
    platform_root: &std::path::Path,
) -> ResolvedPlatform {
    let constraint_files = manifest
        .platform
        .constraints
        .as_ref()
        .map(|c| c.files.iter().map(|f| platform_root.join(f)).collect())
        .unwrap_or_default();

    let variant_tags = manifest
        .platform
        .variant_defaults
        .as_ref()
        .map(|v| v.tags.clone())
        .unwrap_or_default();

    ResolvedPlatform {
        name: manifest.platform.name.clone(),
        part: manifest.platform.part.clone(),
        board: manifest.platform.board.clone(),
        backend: manifest.platform.tool.as_ref().map(|t| t.backend.clone()),
        backend_version: manifest
            .platform
            .tool
            .as_ref()
            .and_then(|t| t.version.clone()),
        virtual_platform: manifest.platform.virtual_platform,
        clocks: manifest.platform.clocks.clone(),
        params: manifest.platform.params.clone(),
        constraint_files,
        variant_tags,
        platform_root: platform_root.to_owned(),
    }
}

/// Substitute `${platform.*}` parameter references in a string.
///
/// Supports:
/// - `${platform.clocks.<name>.frequency_mhz}`
/// - `${platform.clocks.<name>.period_ns}`
/// - `${platform.clocks.<name>.pin}`
/// - `${platform.clocks.<name>.standard}`
/// - `${platform.params.<name>}`
/// - `${platform.part}`
/// - `${platform.name}`
pub fn substitute_platform_params(
    input: &str,
    platform: &ResolvedPlatform,
) -> Result<String, LoomError> {
    let re = regex::Regex::new(r"\$\{platform\.([^}]+)\}").unwrap();
    let mut result = input.to_string();
    let mut errors = Vec::new();

    for cap in re.captures_iter(input) {
        let full_match = &cap[0];
        let path = &cap[1];

        let replacement = resolve_platform_path(path, platform);
        match replacement {
            Some(val) => {
                result = result.replacen(full_match, &val, 1);
            }
            None => {
                errors.push(format!("Unknown platform parameter: {}", full_match));
            }
        }
    }

    if !errors.is_empty() {
        return Err(LoomError::Internal(errors.join("; ")));
    }

    Ok(result)
}

fn resolve_platform_path(path: &str, platform: &ResolvedPlatform) -> Option<String> {
    let parts: Vec<&str> = path.split('.').collect();

    match parts.as_slice() {
        ["name"] => Some(platform.name.clone()),
        ["part"] => platform.part.clone(),
        ["clocks", clock_name, field] => {
            let clock = platform.clocks.get(*clock_name)?;
            match *field {
                "frequency_mhz" => Some(format!("{}", clock.frequency_mhz)),
                "period_ns" => Some(format!("{}", clock.period_ns)),
                "pin" => clock.pin.clone(),
                "standard" => clock.standard.clone(),
                _ => None,
            }
        }
        ["params", name] => {
            let val = platform.params.get(*name)?;
            Some(toml_value_to_string(val))
        }
        _ => None,
    }
}

fn toml_value_to_string(val: &toml::Value) -> String {
    match val {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::platform::ClockDef;

    fn make_test_platform() -> ResolvedPlatform {
        ResolvedPlatform {
            name: "zcu104".to_string(),
            part: Some("xczu7ev-ffvc1156-2-e".to_string()),
            board: None,
            backend: Some("vivado".to_string()),
            backend_version: Some("2023.2".to_string()),
            virtual_platform: false,
            clocks: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "sys_clk".to_string(),
                    ClockDef {
                        frequency_mhz: 125.0,
                        period_ns: 8.0,
                        pin: Some("H9".to_string()),
                        standard: Some("LVDS".to_string()),
                        description: None,
                    },
                );
                m
            },
            params: {
                let mut m = std::collections::HashMap::new();
                m.insert("ddr4_data_width".to_string(), toml::Value::Integer(64));
                m.insert("pcie_lanes".to_string(), toml::Value::Integer(4));
                m
            },
            constraint_files: vec![],
            variant_tags: vec!["vendor:xilinx".to_string()],
            platform_root: PathBuf::from("/platforms/zcu104"),
        }
    }

    #[test]
    fn test_substitute_clock_param() {
        let platform = make_test_platform();
        let result =
            substitute_platform_params("${platform.clocks.sys_clk.frequency_mhz}", &platform)
                .unwrap();
        assert_eq!(result, "125");
    }

    #[test]
    fn test_substitute_params() {
        let platform = make_test_platform();
        let result =
            substitute_platform_params("WIDTH=${platform.params.ddr4_data_width}", &platform)
                .unwrap();
        assert_eq!(result, "WIDTH=64");
    }

    #[test]
    fn test_substitute_part() {
        let platform = make_test_platform();
        let result = substitute_platform_params("${platform.part}", &platform).unwrap();
        assert_eq!(result, "xczu7ev-ffvc1156-2-e");
    }

    #[test]
    fn test_substitute_name() {
        let platform = make_test_platform();
        let result = substitute_platform_params("${platform.name}", &platform).unwrap();
        assert_eq!(result, "zcu104");
    }

    #[test]
    fn test_unknown_param_error() {
        let platform = make_test_platform();
        let result = substitute_platform_params("${platform.nonexistent}", &platform);
        assert!(result.is_err());
    }

    #[test]
    fn test_no_substitution_needed() {
        let platform = make_test_platform();
        let result = substitute_platform_params("plain text no params", &platform).unwrap();
        assert_eq!(result, "plain text no params");
    }

    #[test]
    fn test_multiple_substitutions() {
        let platform = make_test_platform();
        let result = substitute_platform_params(
            "FREQ=${platform.clocks.sys_clk.frequency_mhz} WIDTH=${platform.params.ddr4_data_width}",
            &platform,
        )
        .unwrap();
        assert_eq!(result, "FREQ=125 WIDTH=64");
    }
}
