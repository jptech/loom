use std::collections::HashMap;
use std::path::PathBuf;

use crate::build::context::BuildContext;
use crate::error::LoomError;
use crate::plugin::backend::Diagnostic;

/// A generator produces derived files from inputs.
pub trait GeneratorPlugin: Send + Sync {
    fn plugin_name(&self) -> &str;

    /// Validate generator configuration before execution.
    ///
    /// Called during the pre-flight check phase, before any generator runs.
    /// Return diagnostics for any config problems (missing fields, invalid formats).
    fn validate_config(&self, config: &toml::Value) -> Result<Vec<Diagnostic>, LoomError>;

    /// Check that required external tools are available.
    ///
    /// Called during the pre-flight check phase so that missing tools are
    /// reported before any generator executes. The default implementation
    /// returns no diagnostics (suitable for plugins with no external deps).
    fn check_environment(&self) -> Result<Vec<Diagnostic>, LoomError> {
        Ok(vec![])
    }

    fn compute_cache_key(
        &self,
        config: &toml::Value,
        input_hashes: &HashMap<String, String>,
    ) -> Result<String, LoomError>;

    fn execute(
        &self,
        config: &toml::Value,
        context: &BuildContext,
    ) -> Result<GeneratorResult, LoomError>;

    fn clean(&self, config: &toml::Value, context: &BuildContext) -> Result<(), LoomError>;
}

#[derive(Debug, Clone)]
pub struct GeneratorResult {
    pub success: bool,
    pub produced_files: Vec<PathBuf>,
    pub log: Vec<String>,
}
