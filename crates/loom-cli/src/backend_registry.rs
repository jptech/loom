use loom_core::error::LoomError;
use loom_core::plugin::backend::BackendPlugin;

pub fn get_backend(name: &str) -> Result<Box<dyn BackendPlugin>, LoomError> {
    match name {
        "vivado" => Ok(Box::new(loom_vivado::VivadoBackend)),
        _ => Err(LoomError::ToolNotFound {
            tool: name.to_string(),
            message: format!(
                "Unknown backend '{}'. Supported backends: vivado. \
                 Check your project.toml [target].backend setting.",
                name
            ),
        }),
    }
}
