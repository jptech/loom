use std::collections::HashMap;
use std::path::PathBuf;

use crate::build::context::BuildContext;
use crate::error::LoomError;
use crate::plugin::backend::Diagnostic;

/// A generator produces derived files from inputs.
/// Phase 1: interface defined but no generators are used.
pub trait GeneratorPlugin: Send + Sync {
    fn plugin_name(&self) -> &str;

    fn validate_config(&self, config: &toml::Value) -> Result<Vec<Diagnostic>, LoomError>;

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
