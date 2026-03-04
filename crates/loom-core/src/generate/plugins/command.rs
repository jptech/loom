use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use crate::build::context::BuildContext;
use crate::error::LoomError;
use crate::plugin::backend::Diagnostic;
use crate::plugin::generator::{GeneratorPlugin, GeneratorResult};

pub struct CommandGenerator;

impl GeneratorPlugin for CommandGenerator {
    fn plugin_name(&self) -> &str {
        "command"
    }

    fn validate_config(&self, _config: &toml::Value) -> Result<Vec<Diagnostic>, LoomError> {
        Ok(vec![])
    }

    fn compute_cache_key(
        &self,
        config: &toml::Value,
        input_hashes: &HashMap<String, String>,
    ) -> Result<String, LoomError> {
        let mut hasher = Sha256::new();
        hasher.update(b"command\0");

        if let Some(cmd) = config.get("command").and_then(|v| v.as_str()) {
            hasher.update(cmd.as_bytes());
        }
        hasher.update(b"\0");

        let mut sorted: Vec<_> = input_hashes.iter().collect();
        sorted.sort_by_key(|(k, _)| k.as_str());
        for (path, hash) in sorted {
            hasher.update(path.as_bytes());
            hasher.update(b":");
            hasher.update(hash.as_bytes());
            hasher.update(b"\0");
        }

        Ok(hex::encode(hasher.finalize()))
    }

    fn execute(
        &self,
        config: &toml::Value,
        context: &BuildContext,
    ) -> Result<GeneratorResult, LoomError> {
        let command = config
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                LoomError::Internal("CommandGenerator: 'command' field is required".to_string())
            })?;

        let output = execute_shell_command(command, &context.project.project_root, &context.env)?;

        let success = output.status.success();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let mut log = Vec::new();
        for line in stdout.lines() {
            log.push(line.to_string());
        }
        for line in stderr.lines() {
            log.push(format!("[err] {}", line));
        }

        if !success {
            return Err(LoomError::Internal(format!(
                "Generator command failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr.lines().next().unwrap_or("no error output")
            )));
        }

        let produced_files = if let Some(outputs) = config.get("outputs") {
            outputs
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| context.project.project_root.join(s))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            vec![]
        };

        Ok(GeneratorResult {
            success: true,
            produced_files,
            log,
        })
    }

    fn clean(&self, config: &toml::Value, context: &BuildContext) -> Result<(), LoomError> {
        if let Some(outputs) = config.get("outputs").and_then(|v| v.as_array()) {
            for output in outputs {
                if let Some(path_str) = output.as_str() {
                    let path = context.project.project_root.join(path_str);
                    if path.exists() {
                        std::fs::remove_file(&path)
                            .map_err(|e| LoomError::Io { path, source: e })?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Execute a shell command, returning the output.
fn execute_shell_command(
    command: &str,
    working_dir: &Path,
    env: &HashMap<String, String>,
) -> Result<std::process::Output, LoomError> {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/c", command]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };

    cmd.current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (key, value) in env {
        cmd.env(key, value);
    }

    cmd.output()
        .map_err(|e| LoomError::Internal(format!("Failed to spawn command '{}': {}", command, e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_deterministic() {
        let gen = CommandGenerator;
        let config: toml::Value = toml::from_str("command = \"python gen.py\"").unwrap();
        let hashes = HashMap::new();
        let k1 = gen.compute_cache_key(&config, &hashes).unwrap();
        let k2 = gen.compute_cache_key(&config, &hashes).unwrap();
        assert_eq!(k1, k2);
    }

    #[test]
    fn test_cache_key_changes_with_input() {
        let gen = CommandGenerator;
        let config: toml::Value = toml::from_str("command = \"python gen.py\"").unwrap();
        let hashes_empty = HashMap::new();
        let mut hashes_with_file = HashMap::new();
        hashes_with_file.insert("input.yaml".to_string(), "sha256:abc".to_string());

        let k1 = gen.compute_cache_key(&config, &hashes_empty).unwrap();
        let k2 = gen.compute_cache_key(&config, &hashes_with_file).unwrap();
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_cache_key_changes_with_command() {
        let gen = CommandGenerator;
        let config1: toml::Value = toml::from_str("command = \"python gen.py\"").unwrap();
        let config2: toml::Value = toml::from_str("command = \"python other.py\"").unwrap();
        let hashes = HashMap::new();

        let k1 = gen.compute_cache_key(&config1, &hashes).unwrap();
        let k2 = gen.compute_cache_key(&config2, &hashes).unwrap();
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_command_generator_echo() {
        let gen = CommandGenerator;

        // Build a minimal context
        let tmp = tempfile::TempDir::new().unwrap();
        let project_manifest: crate::manifest::ProjectManifest = toml::from_str(
            r#"
[project]
name = "test"
top_module = "top"
[target]
part = "xc7a35t"
backend = "vivado"
"#,
        )
        .unwrap();

        let resolved = crate::resolve::resolver::ResolvedProject {
            project: project_manifest,
            project_root: tmp.path().to_path_buf(),
            workspace_root: tmp.path().to_path_buf(),
            resolved_components: vec![],
        };

        let context = BuildContext::new(resolved, tmp.path().to_path_buf());

        let config: toml::Value = toml::from_str("command = \"echo hello\"").unwrap();
        let result = gen.execute(&config, &context).unwrap();
        assert!(result.success);
    }

    #[test]
    fn test_command_generator_failure() {
        let tmp = tempfile::TempDir::new().unwrap();
        let gen = CommandGenerator;

        let project_manifest: crate::manifest::ProjectManifest = toml::from_str(
            r#"
[project]
name = "test"
top_module = "top"
[target]
part = "xc7a35t"
backend = "vivado"
"#,
        )
        .unwrap();

        let resolved = crate::resolve::resolver::ResolvedProject {
            project: project_manifest,
            project_root: tmp.path().to_path_buf(),
            workspace_root: tmp.path().to_path_buf(),
            resolved_components: vec![],
        };

        let context = BuildContext::new(resolved, tmp.path().to_path_buf());

        let fail_cmd = if cfg!(target_os = "windows") {
            "exit /b 1"
        } else {
            "exit 1"
        };
        let config: toml::Value = toml::from_str(&format!("command = \"{}\"", fail_cmd)).unwrap();
        let result = gen.execute(&config, &context);
        assert!(result.is_err());
    }
}
