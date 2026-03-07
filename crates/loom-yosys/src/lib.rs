pub mod env_check;
pub mod output_parser;
pub mod pack;
pub mod pnr;
pub mod synth;

use std::path::PathBuf;

use loom_core::assemble::fileset::AssembledFilesets;
use loom_core::build::context::BuildContext;
use loom_core::build::progress::BuildEvent;
use loom_core::error::LoomError;
use loom_core::plugin::backend::{
    BackendCapabilities, BackendPlugin, BuildResult, Diagnostic, EnvironmentStatus,
};
use loom_core::resolve::resolver::ResolvedProject;

/// Supported yosys/nextpnr architectures.
#[derive(Debug, Clone, PartialEq)]
pub enum YosysArchitecture {
    Ice40,
    Ecp5,
    Gowin,
}

impl YosysArchitecture {
    pub fn from_part(part: &str) -> Option<Self> {
        let lower = part.to_lowercase();
        if lower.starts_with("ice40")
            || lower.starts_with("lp")
            || lower.starts_with("hx")
            || lower.starts_with("up5k")
        {
            Some(YosysArchitecture::Ice40)
        } else if lower.starts_with("lfe5") || lower.starts_with("ecp5") {
            Some(YosysArchitecture::Ecp5)
        } else if lower.starts_with("gw") {
            Some(YosysArchitecture::Gowin)
        } else {
            None
        }
    }

    pub fn synth_command(&self) -> &str {
        match self {
            YosysArchitecture::Ice40 => "synth_ice40",
            YosysArchitecture::Ecp5 => "synth_ecp5",
            YosysArchitecture::Gowin => "synth_gowin",
        }
    }

    pub fn nextpnr_binary(&self) -> &str {
        match self {
            YosysArchitecture::Ice40 => "nextpnr-ice40",
            YosysArchitecture::Ecp5 => "nextpnr-ecp5",
            YosysArchitecture::Gowin => "nextpnr-gowin",
        }
    }

    pub fn pack_binary(&self) -> &str {
        match self {
            YosysArchitecture::Ice40 => "icepack",
            YosysArchitecture::Ecp5 => "ecppack",
            YosysArchitecture::Gowin => "gowin_pack",
        }
    }

    pub fn constraint_format(&self) -> &str {
        match self {
            YosysArchitecture::Ice40 => "pcf",
            YosysArchitecture::Ecp5 => "lpf",
            YosysArchitecture::Gowin => "cst",
        }
    }
}

pub struct YosysNextpnrBackend;

impl BackendPlugin for YosysNextpnrBackend {
    fn plugin_name(&self) -> &str {
        "yosys"
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            supports_ooc: false,
            supports_incremental: false,
            supports_ip_generation: false,
            supports_block_design: false,
            supports_strategy_sweep: false,
            checkpoint_format: Some("json".to_string()),
            constraint_formats: vec!["pcf".to_string(), "lpf".to_string(), "cst".to_string()],
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
        env_check::check_yosys_environment(required_version)
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
        let target = project
            .project
            .target
            .as_ref()
            .ok_or_else(|| LoomError::Internal("Project has no target".to_string()))?;

        let arch = YosysArchitecture::from_part(&target.part).ok_or_else(|| {
            LoomError::Internal(format!(
                "Could not determine yosys architecture from part '{}'",
                target.part
            ))
        })?;

        let script = synth::generate_yosys_script(project, filesets, &arch)?;
        let script_path = synth::write_yosys_script(&script, context)?;

        Ok(vec![script_path])
    }

    fn execute_build(
        &self,
        scripts: &[PathBuf],
        context: &BuildContext,
        progress: Option<&(dyn Fn(BuildEvent) + Send + Sync)>,
    ) -> Result<BuildResult, LoomError> {
        if scripts.is_empty() {
            return Err(LoomError::Internal("No build scripts".to_string()));
        }

        let project = &context.project;
        let target = project
            .project
            .target
            .as_ref()
            .ok_or_else(|| LoomError::Internal("Project has no target".to_string()))?;

        let arch = YosysArchitecture::from_part(&target.part).ok_or_else(|| {
            LoomError::Internal(format!("Unknown arch for part '{}'", target.part))
        })?;

        let mut all_phases = Vec::new();
        let mut all_logs = Vec::new();

        // Step 1: Run yosys synthesis
        let synth_result = synth::run_yosys(&scripts[0], context, progress)?;
        all_logs.extend(synth_result.log_paths.clone());
        if !synth_result.success {
            return Ok(BuildResult {
                log_paths: all_logs,
                ..synth_result
            });
        }
        all_phases.extend(synth_result.phases_completed);

        // Step 2: Run nextpnr place & route
        let json_file = context.build_dir.join("design.json");
        let pnr_result = pnr::run_nextpnr(&arch, &json_file, &target.part, context, progress)?;
        all_logs.extend(pnr_result.log_paths.clone());
        if !pnr_result.success {
            return Ok(BuildResult {
                phases_completed: all_phases,
                log_paths: all_logs,
                ..pnr_result
            });
        }
        all_phases.extend(pnr_result.phases_completed);

        // Step 3: Pack bitstream
        let pack_result = pack::run_pack(&arch, context, progress)?;
        all_logs.extend(pack_result.log_paths.clone());
        all_phases.extend(pack_result.phases_completed.clone());

        Ok(BuildResult {
            phases_completed: all_phases,
            log_paths: all_logs,
            ..pack_result
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arch_from_part_ice40() {
        assert_eq!(
            YosysArchitecture::from_part("lp8k"),
            Some(YosysArchitecture::Ice40)
        );
        assert_eq!(
            YosysArchitecture::from_part("up5k"),
            Some(YosysArchitecture::Ice40)
        );
        assert_eq!(
            YosysArchitecture::from_part("hx8k"),
            Some(YosysArchitecture::Ice40)
        );
    }

    #[test]
    fn test_arch_from_part_ecp5() {
        assert_eq!(
            YosysArchitecture::from_part("LFE5U-85F"),
            Some(YosysArchitecture::Ecp5)
        );
    }

    #[test]
    fn test_arch_from_part_gowin() {
        assert_eq!(
            YosysArchitecture::from_part("GW1NR-9"),
            Some(YosysArchitecture::Gowin)
        );
    }

    #[test]
    fn test_arch_from_part_unknown() {
        assert_eq!(YosysArchitecture::from_part("xc7a35t"), None);
    }

    #[test]
    fn test_capabilities() {
        let backend = YosysNextpnrBackend;
        let caps = backend.capabilities();
        assert!(!caps.supports_ooc);
        assert!(!caps.supports_incremental);
        assert!(caps.constraint_formats.contains(&"pcf".to_string()));
    }
}
