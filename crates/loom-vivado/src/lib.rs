pub mod env_check;
pub mod executor;
pub mod ooc;
pub mod tcl_gen;

use std::path::PathBuf;

use loom_core::assemble::fileset::AssembledFilesets;
use loom_core::build::context::BuildContext;
use loom_core::error::LoomError;
use loom_core::plugin::backend::{BackendPlugin, BuildResult, Diagnostic, EnvironmentStatus};
use loom_core::resolve::resolver::ResolvedProject;

pub struct VivadoBackend;

impl BackendPlugin for VivadoBackend {
    fn plugin_name(&self) -> &str {
        "vivado"
    }

    fn check_environment(
        &self,
        required_version: Option<&str>,
    ) -> Result<EnvironmentStatus, LoomError> {
        env_check::check_vivado_environment(required_version)
    }

    fn validate(
        &self,
        _project: &ResolvedProject,
        _filesets: &AssembledFilesets,
        _context: &BuildContext,
    ) -> Result<Vec<Diagnostic>, LoomError> {
        Ok(vec![])
    }

    fn generate_build_scripts(
        &self,
        project: &ResolvedProject,
        filesets: &AssembledFilesets,
        context: &BuildContext,
    ) -> Result<Vec<PathBuf>, LoomError> {
        let script = tcl_gen::generate_tcl(project, filesets, context)
            .map_err(|e| LoomError::Internal(e.to_string()))?;

        let script_path = tcl_gen::write_tcl_script(&script, context)
            .map_err(|e| LoomError::Internal(e.to_string()))?;

        Ok(vec![script_path])
    }

    fn execute_build(
        &self,
        scripts: &[PathBuf],
        context: &BuildContext,
    ) -> Result<BuildResult, LoomError> {
        executor::run_vivado_batch(scripts, context)
    }
}
