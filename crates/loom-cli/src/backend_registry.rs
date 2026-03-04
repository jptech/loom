use loom_core::error::LoomError;
use loom_core::plugin::backend::BackendPlugin;

pub fn get_backend(name: &str) -> Result<Box<dyn BackendPlugin>, LoomError> {
    match name {
        "vivado" => Ok(Box::new(loom_vivado::VivadoBackend)),
        "quartus" => Ok(Box::new(loom_quartus::QuartusBackend)),
        "yosys" => Ok(Box::new(loom_yosys::YosysNextpnrBackend)),
        _ => Err(LoomError::ToolNotFound {
            tool: name.to_string(),
            message: format!(
                "Unknown backend '{}'. Supported backends: vivado, quartus, yosys. \
                 Check your project.toml [target].backend setting.",
                name
            ),
        }),
    }
}
