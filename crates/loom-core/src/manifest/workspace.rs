use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceManifest {
    pub workspace: WorkspaceMeta,
    #[serde(default)]
    pub settings: WorkspaceSettings,
    #[serde(default)]
    pub resolution: ResolutionConfig,
    #[serde(default)]
    pub plugins: HashMap<String, PluginDecl>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceMeta {
    pub name: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WorkspaceSettings {
    pub default_tool_version: Option<String>,
    pub build_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResolutionConfig {
    #[serde(default)]
    pub overrides: HashMap<String, ResolutionOverride>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResolutionOverride {
    pub path: Option<std::path::PathBuf>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginDecl {
    pub path: Option<std::path::PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_workspace_manifest() {
        let toml_str = r#"
[workspace]
name = "my_fpga_repo"
members = ["lib/*", "projects/*"]

[settings]
default_tool_version = "2023.2"
"#;
        let manifest: WorkspaceManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.workspace.name, "my_fpga_repo");
        assert_eq!(manifest.workspace.members.len(), 2);
        assert_eq!(
            manifest.settings.default_tool_version.as_deref(),
            Some("2023.2")
        );
    }

    #[test]
    fn test_parse_fixture_workspace() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/simple_project/workspace.toml");
        let content = std::fs::read_to_string(&path).unwrap();
        let manifest: WorkspaceManifest = toml::from_str(&content).unwrap();
        assert_eq!(manifest.workspace.name, "test_workspace");
        assert_eq!(manifest.workspace.members, vec!["lib/*", "projects/*"]);
    }
}
