pub mod env_check;
pub mod executor;
pub mod tcl_gen;

use std::path::PathBuf;

use loom_core::assemble::fileset::AssembledFilesets;
use loom_core::build::context::BuildContext;
use loom_core::build::progress::BuildEvent;
use loom_core::error::LoomError;
use loom_core::plugin::backend::{
    BackendCapabilities, BackendPlugin, BuildResult, Diagnostic, EnvironmentStatus,
};
use loom_core::resolve::resolver::ResolvedProject;

pub struct QuartusBackend;

impl BackendPlugin for QuartusBackend {
    fn plugin_name(&self) -> &str {
        "quartus"
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            supports_ooc: false,
            supports_incremental: true,
            supports_ip_generation: true,
            supports_block_design: true,
            supports_strategy_sweep: false,
            checkpoint_format: None,
            constraint_formats: vec!["sdc".to_string(), "qsf".to_string()],
            sub_phases: vec![
                "synthesis".to_string(),
                "place".to_string(),
                "route".to_string(),
                "bitstream".to_string(),
            ],
        }
    }

    fn check_environment(
        &self,
        required_version: Option<&str>,
    ) -> Result<EnvironmentStatus, LoomError> {
        env_check::check_quartus_environment(required_version)
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
        let script = tcl_gen::generate_quartus_tcl(project, filesets, context)?;
        let script_path = tcl_gen::write_quartus_tcl(&script, context)?;
        Ok(vec![script_path])
    }

    fn execute_build(
        &self,
        scripts: &[PathBuf],
        context: &BuildContext,
        _progress: Option<&(dyn Fn(BuildEvent) + Send + Sync)>,
    ) -> Result<BuildResult, LoomError> {
        executor::run_quartus_batch(scripts, context)
    }
}
