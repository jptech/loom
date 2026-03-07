use crate::manifest::test::TestSimRequirements;
use crate::plugin::simulator::{SimRequirements, SimulatorCapabilities, SimulatorPlugin};

/// Check whether a test's requirements are compatible with a simulator's capabilities.
///
/// Returns a list of incompatibility reasons. An empty list means the test is compatible.
pub fn check_compatibility(
    test_reqs: &TestSimRequirements,
    caps: &SimulatorCapabilities,
) -> Vec<String> {
    let sim_reqs = SimRequirements {
        uvm: test_reqs.uvm,
        fork_join: test_reqs.fork_join,
        force_release: test_reqs.force_release,
        vhdl: test_reqs.vhdl,
        mixed_language: test_reqs.mixed_language,
    };

    let mut reasons = sim_reqs.is_compatible_with(caps);

    // Additional check: full SystemVerilog support
    if test_reqs.systemverilog_full && !caps.systemverilog_full {
        reasons.push("requires full SystemVerilog support".to_string());
    }

    reasons
}

/// Check whether a test's runner is compatible with the given simulator.
pub fn check_runner_compatibility(
    runner: Option<&str>,
    simulator: &dyn SimulatorPlugin,
) -> Option<String> {
    match runner {
        None | Some("hdl") => None,
        Some("cocotb") => {
            // Cocotb requires VPI module loading, which needs explicit backend
            // support (passing -m to vvp, or --vpi to verilator). Currently
            // only xsim/questa/vcs/xcelium wire this up.
            let supported_sims = ["xsim", "questa", "vcs", "xcelium"];
            let sim_name = simulator.plugin_name();
            if !supported_sims.contains(&sim_name) {
                return Some(format!(
                    "cocotb not yet supported with {} (VPI module loading not wired up)",
                    sim_name
                ));
            }
            // Check that cocotb is installed
            match std::process::Command::new("cocotb-config")
                .arg("--version")
                .output()
            {
                Ok(output) if output.status.success() => None,
                _ => Some("cocotb not found (install with: pip install cocotb)".to_string()),
            }
        }
        Some("verilator") => {
            if simulator.plugin_name() != "verilator" {
                Some(format!(
                    "runner \"verilator\" requires the verilator simulator, not {}",
                    simulator.plugin_name()
                ))
            } else {
                None
            }
        }
        Some(other) => Some(format!("unknown runner \"{}\"", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps_full() -> SimulatorCapabilities {
        SimulatorCapabilities {
            systemverilog_full: true,
            vhdl: true,
            mixed_language: true,
            uvm: true,
            fork_join: true,
            force_release: true,
            bind_statements: true,
            code_coverage: true,
            functional_coverage: true,
            assertion_coverage: true,
            compilation_model: "event_driven".to_string(),
            supports_gui: true,
            supports_save_restore: true,
            typical_compile_speed: "medium".to_string(),
            typical_sim_speed: "medium".to_string(),
        }
    }

    fn caps_limited() -> SimulatorCapabilities {
        SimulatorCapabilities {
            systemverilog_full: false,
            vhdl: false,
            mixed_language: false,
            uvm: false,
            fork_join: false,
            force_release: true,
            bind_statements: false,
            code_coverage: true,
            functional_coverage: false,
            assertion_coverage: false,
            compilation_model: "cycle_accurate".to_string(),
            supports_gui: false,
            supports_save_restore: false,
            typical_compile_speed: "fast".to_string(),
            typical_sim_speed: "fast".to_string(),
        }
    }

    #[test]
    fn test_compatible_with_full_sim() {
        let reqs = TestSimRequirements {
            uvm: true,
            fork_join: true,
            ..Default::default()
        };
        let reasons = check_compatibility(&reqs, &caps_full());
        assert!(reasons.is_empty());
    }

    #[test]
    fn test_incompatible_uvm() {
        let reqs = TestSimRequirements {
            uvm: true,
            ..Default::default()
        };
        let reasons = check_compatibility(&reqs, &caps_limited());
        assert!(!reasons.is_empty());
        assert!(reasons.iter().any(|r| r.contains("UVM")));
    }

    #[test]
    fn test_incompatible_sv_full() {
        let reqs = TestSimRequirements {
            systemverilog_full: true,
            ..Default::default()
        };
        let reasons = check_compatibility(&reqs, &caps_limited());
        assert!(!reasons.is_empty());
        assert!(reasons.iter().any(|r| r.contains("SystemVerilog")));
    }

    #[test]
    fn test_no_requirements_always_compatible() {
        let reqs = TestSimRequirements::default();
        let reasons = check_compatibility(&reqs, &caps_limited());
        assert!(reasons.is_empty());
    }
}
