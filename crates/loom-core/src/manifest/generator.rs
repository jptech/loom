use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A generator declaration in a manifest.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GeneratorDecl {
    pub name: String,
    pub plugin: String,

    /// Shell command to run (for `command` plugin).
    pub command: Option<String>,
    /// Windows-specific command override.
    pub command_windows: Option<String>,

    /// Input files the generator reads.
    #[serde(default)]
    pub inputs: Vec<PathBuf>,
    /// Output files the generator produces.
    #[serde(default)]
    pub outputs: Vec<PathBuf>,

    /// Which fileset receives the generated outputs (default: "synth").
    #[serde(default = "default_fileset")]
    pub fileset: String,

    /// Explicit ordering dependency on other generators by name.
    #[serde(default)]
    pub depends_on: Vec<String>,

    /// If true, skip this generator if cache key matches.
    #[serde(default = "default_true")]
    pub cacheable: bool,

    /// If true, framework cannot verify outputs (disables caching).
    #[serde(default)]
    pub outputs_unknown: bool,

    /// Plugin-specific configuration (arbitrary TOML table).
    pub config: Option<toml::Value>,
}

fn default_fileset() -> String {
    "synth".to_string()
}

fn default_true() -> bool {
    true
}

impl GeneratorDecl {
    /// The effective command, accounting for platform.
    pub fn effective_command(&self) -> Option<&str> {
        #[cfg(target_os = "windows")]
        {
            self.command_windows.as_deref().or(self.command.as_deref())
        }
        #[cfg(not(target_os = "windows"))]
        {
            self.command.as_deref()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generator_decl_defaults() {
        let toml_str = r#"
name = "gen"
plugin = "command"
command = "echo hi"
"#;
        let decl: GeneratorDecl = toml::from_str(toml_str).unwrap();
        assert_eq!(decl.fileset, "synth");
        assert!(decl.cacheable);
        assert!(!decl.outputs_unknown);
        assert!(decl.inputs.is_empty());
        assert!(decl.outputs.is_empty());
        assert!(decl.depends_on.is_empty());
        assert!(decl.config.is_none());
    }

    #[test]
    fn test_generator_decl_full() {
        let toml_str = r#"
name = "regmap"
plugin = "command"
command = "python gen.py"
command_windows = "python.exe gen.py"
inputs = ["regs.yaml"]
outputs = ["generated/regs.sv"]
fileset = "synth"
depends_on = ["other_gen"]
cacheable = false
outputs_unknown = true
[config]
key = "value"
"#;
        let decl: GeneratorDecl = toml::from_str(toml_str).unwrap();
        assert_eq!(decl.name, "regmap");
        assert_eq!(decl.plugin, "command");
        assert_eq!(decl.command.as_deref(), Some("python gen.py"));
        assert_eq!(decl.command_windows.as_deref(), Some("python.exe gen.py"));
        assert_eq!(decl.inputs.len(), 1);
        assert_eq!(decl.outputs.len(), 1);
        assert_eq!(decl.depends_on, vec!["other_gen"]);
        assert!(!decl.cacheable);
        assert!(decl.outputs_unknown);
        assert!(decl.config.is_some());
    }

    #[test]
    fn test_effective_command() {
        let decl = GeneratorDecl {
            name: "test".to_string(),
            plugin: "command".to_string(),
            command: Some("sh gen.sh".to_string()),
            command_windows: Some("gen.bat".to_string()),
            inputs: vec![],
            outputs: vec![],
            fileset: "synth".to_string(),
            depends_on: vec![],
            cacheable: true,
            outputs_unknown: false,
            config: None,
        };

        // On Windows, should return command_windows; on Unix, command
        let cmd = decl.effective_command();
        assert!(cmd.is_some());
    }
}
