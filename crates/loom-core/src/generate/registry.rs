use std::collections::HashMap;

use crate::error::LoomError;
use crate::generate::plugins::command::CommandGenerator;
use crate::manifest::GeneratorDecl;
use crate::plugin::generator::GeneratorPlugin;

type PluginFactory =
    Box<dyn Fn(&GeneratorDecl) -> Result<Box<dyn GeneratorPlugin>, LoomError> + Send + Sync>;

/// Centralized registry for generator plugin factories.
///
/// Each plugin type is registered with a factory closure that creates
/// per-instance plugin objects from a `GeneratorDecl`.
pub struct PluginRegistry {
    factories: HashMap<String, PluginFactory>,
}

impl PluginRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Create a registry pre-loaded with built-in plugins ("command", "python").
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();

        registry.register("command", |_decl| Ok(Box::new(CommandGenerator)));

        // Python plugin requires per-instance config (script path).
        // Registered here with a factory that reads from decl.config.
        registry.register("python", |decl| {
            let script = decl
                .config
                .as_ref()
                .and_then(|c| c.get("script"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    LoomError::Internal(format!(
                        "Python generator '{}' requires config.script field",
                        decl.name
                    ))
                })?;
            Ok(Box::new(
                crate::generate::plugins::python::PythonGenerator::new(
                    &decl.name,
                    std::path::PathBuf::from(script),
                ),
            ))
        });

        registry
    }

    /// Register a plugin factory under the given name.
    pub fn register<F>(&mut self, name: &str, factory: F)
    where
        F: Fn(&GeneratorDecl) -> Result<Box<dyn GeneratorPlugin>, LoomError>
            + Send
            + Sync
            + 'static,
    {
        self.factories.insert(name.to_string(), Box::new(factory));
    }

    /// Look up and instantiate a plugin for the given declaration.
    pub fn get(&self, decl: &GeneratorDecl) -> Result<Box<dyn GeneratorPlugin>, LoomError> {
        let factory = self.factories.get(&decl.plugin).ok_or_else(|| {
            LoomError::Internal(format!(
                "No generator plugin found for '{}'. Available: {}",
                decl.plugin,
                self.available_plugins().join(", ")
            ))
        })?;
        factory(decl)
    }

    /// List available plugin names.
    pub fn available_plugins(&self) -> Vec<String> {
        let mut names: Vec<_> = self.factories.keys().cloned().collect();
        names.sort();
        names
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_command_plugin() {
        let registry = PluginRegistry::with_builtins();
        let decl = GeneratorDecl {
            name: "test".to_string(),
            plugin: "command".to_string(),
            command: Some("echo hi".to_string()),
            command_windows: None,
            inputs: vec![],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: None,
        };
        let plugin = registry.get(&decl).unwrap();
        assert_eq!(plugin.plugin_name(), "command");
    }

    #[test]
    fn test_unknown_plugin_error() {
        let registry = PluginRegistry::with_builtins();
        let decl = GeneratorDecl {
            name: "test".to_string(),
            plugin: "nonexistent".to_string(),
            command: None,
            command_windows: None,
            inputs: vec![],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: None,
        };
        match registry.get(&decl) {
            Err(e) => assert!(e.to_string().contains("nonexistent")),
            Ok(_) => panic!("Expected error for unknown plugin"),
        }
    }

    #[test]
    fn test_custom_plugin_registration() {
        let mut registry = PluginRegistry::new();
        registry.register("custom", |_decl| Ok(Box::new(CommandGenerator)));

        let decl = GeneratorDecl {
            name: "test".to_string(),
            plugin: "custom".to_string(),
            command: Some("echo hi".to_string()),
            command_windows: None,
            inputs: vec![],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: None,
        };
        assert!(registry.get(&decl).is_ok());
    }

    #[test]
    fn test_available_plugins() {
        let registry = PluginRegistry::with_builtins();
        let plugins = registry.available_plugins();
        assert!(plugins.contains(&"command".to_string()));
        assert!(plugins.contains(&"python".to_string()));
    }

    #[test]
    fn test_python_plugin_with_valid_config() {
        let registry = PluginRegistry::with_builtins();
        let mut config_table = toml::map::Map::new();
        config_table.insert(
            "script".to_string(),
            toml::Value::String("gen_regs.py".to_string()),
        );
        let decl = GeneratorDecl {
            name: "regmap".to_string(),
            plugin: "python".to_string(),
            command: None,
            command_windows: None,
            inputs: vec![],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: Some(toml::Value::Table(config_table)),
        };
        let plugin = registry.get(&decl).unwrap();
        // PythonGenerator's plugin_name() returns the generator instance name
        assert_eq!(plugin.plugin_name(), "regmap");
    }

    #[test]
    fn test_register_overwrite() {
        // Re-registering the same name replaces the factory
        let mut registry = PluginRegistry::new();
        registry.register("custom", |_decl| Ok(Box::new(CommandGenerator)));
        registry.register("custom", |_decl| {
            Err(LoomError::Internal("replaced".to_string()))
        });

        let decl = GeneratorDecl {
            name: "test".to_string(),
            plugin: "custom".to_string(),
            command: None,
            command_windows: None,
            inputs: vec![],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: None,
        };
        match registry.get(&decl) {
            Err(e) => assert!(e.to_string().contains("replaced")),
            Ok(_) => panic!("Expected replaced factory to return error"),
        }
    }

    #[test]
    fn test_error_message_lists_available_plugins() {
        let registry = PluginRegistry::with_builtins();
        let decl = GeneratorDecl {
            name: "test".to_string(),
            plugin: "nonexistent".to_string(),
            command: None,
            command_windows: None,
            inputs: vec![],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: None,
        };
        match registry.get(&decl) {
            Err(e) => {
                let msg = e.to_string();
                assert!(msg.contains("command"), "Should list 'command': {}", msg);
                assert!(msg.contains("python"), "Should list 'python': {}", msg);
            }
            Ok(_) => panic!("Expected error"),
        }
    }

    #[test]
    fn test_default_is_with_builtins() {
        let registry = PluginRegistry::default();
        let plugins = registry.available_plugins();
        assert!(plugins.contains(&"command".to_string()));
        assert!(plugins.contains(&"python".to_string()));
    }

    #[test]
    fn test_empty_registry_has_no_plugins() {
        let registry = PluginRegistry::new();
        assert!(registry.available_plugins().is_empty());
    }

    #[test]
    fn test_python_plugin_requires_script() {
        let registry = PluginRegistry::with_builtins();
        let decl = GeneratorDecl {
            name: "test".to_string(),
            plugin: "python".to_string(),
            command: None,
            command_windows: None,
            inputs: vec![],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: None,
        };
        match registry.get(&decl) {
            Err(e) => assert!(e.to_string().contains("config.script")),
            Ok(_) => panic!("Expected error for python plugin without script"),
        }
    }
}
