use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::build::context::BuildContext;
use crate::error::LoomError;
use crate::plugin::backend::Diagnostic;
use crate::plugin::generator::{GeneratorPlugin, GeneratorResult};

/// A Python generator plugin loaded via subprocess execution.
pub struct PythonGenerator {
    /// Path to the Python plugin script.
    plugin_path: PathBuf,
    /// Name of the plugin.
    name: String,
}

impl PythonGenerator {
    pub fn new(name: &str, plugin_path: PathBuf) -> Self {
        Self {
            plugin_path,
            name: name.to_string(),
        }
    }

    fn run_action(
        &self,
        action: &str,
        config: &toml::Value,
        context_json: Option<&str>,
        extra_args: &[(&str, &str)],
    ) -> Result<serde_json::Value, LoomError> {
        let config_json = serde_json::to_string(config)
            .map_err(|e| LoomError::Internal(format!("Failed to serialize config: {}", e)))?;

        let python = find_python()?;
        let mut cmd = Command::new(&python);
        cmd.arg(&self.plugin_path)
            .arg("--action")
            .arg(action)
            .arg("--config")
            .arg(&config_json);

        if let Some(ctx) = context_json {
            cmd.arg("--context").arg(ctx);
        }

        for (key, value) in extra_args {
            cmd.arg(format!("--{}", key)).arg(value);
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = cmd.output().map_err(|e| LoomError::ToolNotFound {
            tool: "python".to_string(),
            message: format!("Failed to execute Python plugin '{}': {}", self.name, e),
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(LoomError::Internal(format!(
                "Python plugin '{}' failed (exit {}): {}",
                self.name,
                output.status.code().unwrap_or(-1),
                stderr.lines().next().unwrap_or("no error output")
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str(&stdout).map_err(|e| {
            LoomError::Internal(format!(
                "Python plugin '{}' returned invalid JSON: {}",
                self.name, e
            ))
        })
    }
}

impl GeneratorPlugin for PythonGenerator {
    fn plugin_name(&self) -> &str {
        &self.name
    }

    fn check_environment(&self) -> Result<Vec<Diagnostic>, LoomError> {
        match find_python() {
            Ok(_) => Ok(vec![]),
            Err(_) => Ok(vec![Diagnostic {
                severity: crate::plugin::backend::DiagnosticSeverity::Error,
                message: "Python not found. Install Python 3 and ensure 'python3' or 'python' is on PATH.".to_string(),
                source_path: None,
                line: None,
            }]),
        }
    }

    fn validate_config(&self, config: &toml::Value) -> Result<Vec<Diagnostic>, LoomError> {
        let result = self.run_action("validate", config, None, &[])?;
        // Parse diagnostics from JSON array
        if let Some(arr) = result.as_array() {
            let diagnostics: Vec<Diagnostic> = arr
                .iter()
                .filter_map(|v| {
                    Some(Diagnostic {
                        severity: crate::plugin::backend::DiagnosticSeverity::Warning,
                        message: v.get("message")?.as_str()?.to_string(),
                        source_path: None,
                        line: None,
                    })
                })
                .collect();
            Ok(diagnostics)
        } else {
            Ok(vec![])
        }
    }

    fn compute_cache_key(
        &self,
        config: &toml::Value,
        input_hashes: &HashMap<String, String>,
    ) -> Result<String, LoomError> {
        let hashes_json =
            serde_json::to_string(input_hashes).map_err(|e| LoomError::Internal(e.to_string()))?;

        let result =
            self.run_action("cache_key", config, None, &[("input_hashes", &hashes_json)])?;

        result
            .get("cache_key")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                LoomError::Internal(format!(
                    "Python plugin '{}' did not return cache_key",
                    self.name
                ))
            })
    }

    fn execute(
        &self,
        config: &toml::Value,
        context: &BuildContext,
    ) -> Result<GeneratorResult, LoomError> {
        let context_json = serde_json::json!({
            "build_dir": context.build_dir.display().to_string(),
            "workspace_root": context.workspace_root.display().to_string(),
            "project_root": context.project.project_root.display().to_string(),
            "project_name": context.project.project.project.name,
        });
        let ctx_str = context_json.to_string();

        let result = self.run_action("execute", config, Some(&ctx_str), &[])?;

        let success = result
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let produced_files = result
            .get("produced_files")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(PathBuf::from))
                    .collect()
            })
            .unwrap_or_default();

        let log = result
            .get("log")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        if !success {
            let msg = result
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            return Err(LoomError::Internal(format!(
                "Python plugin '{}' execution failed: {}",
                self.name, msg
            )));
        }

        Ok(GeneratorResult {
            success,
            produced_files,
            log,
        })
    }

    fn clean(&self, config: &toml::Value, context: &BuildContext) -> Result<(), LoomError> {
        let context_json = serde_json::json!({
            "build_dir": context.build_dir.display().to_string(),
        });
        let ctx_str = context_json.to_string();

        let _ = self.run_action("clean", config, Some(&ctx_str), &[])?;
        Ok(())
    }
}

/// Find the Python executable.
fn find_python() -> Result<PathBuf, LoomError> {
    // Try python3 first, then python
    for name in &["python3", "python"] {
        let result = Command::new(name).arg("--version").output();
        if let Ok(output) = result {
            if output.status.success() {
                return Ok(PathBuf::from(name));
            }
        }
    }

    Err(LoomError::ToolNotFound {
        tool: "python".to_string(),
        message: "Python not found. Install Python 3 and ensure it's on PATH.".to_string(),
    })
}

/// Discover Python plugin paths from workspace.
pub fn discover_python_plugins(workspace_root: &Path) -> Vec<(String, PathBuf)> {
    let plugins_dir = workspace_root.join("plugins");
    let mut found = Vec::new();

    if plugins_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&plugins_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("py") {
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    found.push((name, path));
                }
            }
        }
    }

    found
}
