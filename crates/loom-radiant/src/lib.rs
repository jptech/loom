pub mod env_check;
pub mod executor;
pub mod tcl_gen;

use std::path::PathBuf;

use loom_core::assemble::fileset::AssembledFilesets;
use loom_core::build::context::BuildContext;
use loom_core::error::LoomError;
use loom_core::plugin::backend::{
    BackendCapabilities, BackendPlugin, BuildResult, Diagnostic, EnvironmentStatus,
};
use loom_core::resolve::resolver::ResolvedProject;

/// Supported Lattice device families.
#[derive(Debug, Clone, PartialEq)]
pub enum RadiantFamily {
    Ice40Ultra,
    CrossLinkNx,
    CertusProNx,
}

impl RadiantFamily {
    pub fn from_part(part: &str) -> Option<Self> {
        let lower = part.to_lowercase();
        if lower.starts_with("ice40up") || lower.starts_with("ice40ultra") {
            Some(RadiantFamily::Ice40Ultra)
        } else if lower.starts_with("lifcl") || lower.starts_with("crosslink") {
            Some(RadiantFamily::CrossLinkNx)
        } else if lower.starts_with("lfcpnx") || lower.starts_with("certuspro") {
            Some(RadiantFamily::CertusProNx)
        } else {
            None
        }
    }

    pub fn constraint_format(&self) -> &str {
        match self {
            RadiantFamily::Ice40Ultra => "pdc",
            RadiantFamily::CrossLinkNx => "pdc",
            RadiantFamily::CertusProNx => "pdc",
        }
    }
}

pub struct RadiantBackend;

impl BackendPlugin for RadiantBackend {
    fn plugin_name(&self) -> &str {
        "radiant"
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            supports_ooc: false,
            supports_incremental: false,
            supports_ip_generation: true,
            supports_block_design: false,
            supports_strategy_sweep: false,
            checkpoint_format: None,
            constraint_formats: vec!["pdc".to_string(), "lpf".to_string()],
            sub_phases: vec![
                "synthesis".to_string(),
                "map".to_string(),
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
        env_check::check_radiant_environment(required_version)
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
        let script = tcl_gen::generate_radiant_tcl(project, filesets, context)?;
        let script_path = tcl_gen::write_radiant_tcl(&script, context)?;
        Ok(vec![script_path])
    }

    fn execute_build(
        &self,
        scripts: &[PathBuf],
        context: &BuildContext,
    ) -> Result<BuildResult, LoomError> {
        executor::run_radiant_batch(scripts, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_from_part_ice40ultra() {
        assert_eq!(
            RadiantFamily::from_part("iCE40UP5K"),
            Some(RadiantFamily::Ice40Ultra)
        );
    }

    #[test]
    fn test_family_from_part_crosslink() {
        assert_eq!(
            RadiantFamily::from_part("LIFCL-40"),
            Some(RadiantFamily::CrossLinkNx)
        );
    }

    #[test]
    fn test_family_from_part_certuspro() {
        assert_eq!(
            RadiantFamily::from_part("LFCPNX-100"),
            Some(RadiantFamily::CertusProNx)
        );
    }

    #[test]
    fn test_family_from_part_unknown() {
        assert_eq!(RadiantFamily::from_part("xc7a35t"), None);
    }

    #[test]
    fn test_capabilities() {
        let backend = RadiantBackend;
        let caps = backend.capabilities();
        assert!(!caps.supports_ooc);
        assert!(caps.supports_ip_generation);
        assert!(caps.constraint_formats.contains(&"pdc".to_string()));
    }
}
