use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::LoomError;

/// Metadata about a package in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryPackage {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub checksum: String,
    pub download_url: String,
    pub dependencies: HashMap<String, String>,
}

/// Registry configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub url: String,
    pub token: Option<String>,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            url: "https://registry.loom-fpga.dev".to_string(),
            token: None,
        }
    }
}

/// A dependency source that resolves against a remote package registry.
pub struct RegistryDependencySource {
    config: RegistryConfig,
    cache_dir: PathBuf,
}

impl RegistryDependencySource {
    pub fn new(config: RegistryConfig, cache_dir: PathBuf) -> Self {
        Self { config, cache_dir }
    }

    /// Search for packages matching a query.
    pub fn search(&self, query: &str) -> Result<Vec<RegistryPackage>, LoomError> {
        let url = format!("{}/api/v1/search?q={}", self.config.url, query);
        // In a real implementation, this would make an HTTP request.
        // For now, return an error indicating the registry is not yet available.
        Err(LoomError::Internal(format!(
            "Registry search not yet implemented (would query {})",
            url
        )))
    }

    /// List available versions of a package.
    pub fn list_versions(&self, name: &str) -> Result<Vec<String>, LoomError> {
        let url = format!("{}/api/v1/packages/{}/versions", self.config.url, name);
        Err(LoomError::Internal(format!(
            "Registry version listing not yet implemented (would query {})",
            url
        )))
    }

    /// Download and cache a package.
    pub fn download(&self, name: &str, version: &str) -> Result<PathBuf, LoomError> {
        let cache_path = self
            .cache_dir
            .join("registry")
            .join(name.replace('/', "__"))
            .join(version);

        if cache_path.exists() {
            return Ok(cache_path);
        }

        let url = format!(
            "{}/api/v1/packages/{}/{}/download",
            self.config.url, name, version
        );
        Err(LoomError::Internal(format!(
            "Registry download not yet implemented (would fetch {})",
            url
        )))
    }

    /// Publish a package to the registry.
    pub fn publish(&self, tarball_path: &std::path::Path) -> Result<String, LoomError> {
        let token = self.config.token.as_deref().ok_or_else(|| {
            LoomError::Internal(
                "No registry token configured. Set LOOM_REGISTRY_TOKEN or add token to config."
                    .to_string(),
            )
        })?;

        let _url = format!("{}/api/v1/packages/publish", self.config.url);
        let _ = token;
        let _ = tarball_path;

        Err(LoomError::Internal(
            "Registry publish not yet implemented".to_string(),
        ))
    }
}

/// Create a tarball of a component for publishing.
pub fn create_package_tarball(
    component_root: &std::path::Path,
    output_dir: &std::path::Path,
) -> Result<PathBuf, LoomError> {
    let manifest_path = component_root.join("component.toml");
    if !manifest_path.exists() {
        return Err(LoomError::Internal(format!(
            "No component.toml found at {}",
            component_root.display()
        )));
    }

    let manifest: crate::manifest::ComponentManifest =
        crate::manifest::load_component_manifest(&manifest_path)?;
    let name = manifest.component.name.replace('/', "__");
    let version = &manifest.component.version;
    let tarball_name = format!("{}-{}.tar.gz", name, version);
    let tarball_path = output_dir.join(&tarball_name);

    // In a real implementation, we would create a tar.gz here.
    // For now, just report what would be created.
    Err(LoomError::Internal(format!(
        "Package creation not yet implemented (would create {})",
        tarball_path.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_config_default() {
        let config = RegistryConfig::default();
        assert!(config.url.contains("loom-fpga"));
        assert!(config.token.is_none());
    }

    #[test]
    fn test_registry_source_search_not_implemented() {
        let source =
            RegistryDependencySource::new(RegistryConfig::default(), PathBuf::from("/tmp/cache"));
        let result = source.search("axi");
        assert!(result.is_err());
    }

    #[test]
    fn test_cache_path_format() {
        let source =
            RegistryDependencySource::new(RegistryConfig::default(), PathBuf::from("/tmp/cache"));
        // The download method builds paths with / replaced by __
        let result = source.download("acmecorp/axi_fifo", "1.0.0");
        assert!(result.is_err()); // Not implemented yet
    }
}
