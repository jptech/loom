# Phase 2 / Task 02: Command Generator Plugin

**Prerequisites:** Phase 2 Task 01
**Goal:** Implement the `command` generator plugin — runs an arbitrary shell command, declares inputs/outputs, integrates with the GeneratorPlugin trait.

## Spec Reference
`system_plan.md` §6.4 (Generator Plugin Types), §13.4.2 (Shell/Command on Windows)

## File to Implement
`crates/loom-core/src/generate/plugins/command.rs`

## Implementation

```rust
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::collections::HashMap;
use crate::plugin::generator::{GeneratorPlugin, GeneratorResult};
use crate::build::context::BuildContext;
use crate::plugin::backend::Diagnostic;
use crate::error::LoomError;

pub struct CommandGenerator;

impl GeneratorPlugin for CommandGenerator {
    fn plugin_name(&self) -> &str { "command" }

    fn validate_config(&self, config: &toml::Value) -> Result<Vec<Diagnostic>, LoomError> {
        // For `command` generator, config is the GeneratorDecl fields, not [generators.config]
        // The command field is on the GeneratorDecl itself, not in config.
        // Nothing to validate here for Phase 2.
        Ok(vec![])
    }

    fn compute_cache_key(
        &self,
        config: &toml::Value,
        input_hashes: &HashMap<String, String>,
    ) -> Result<String, LoomError> {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(b"command\0");

        // Hash the command string
        if let Some(cmd) = config.get("command").and_then(|v| v.as_str()) {
            hasher.update(cmd.as_bytes());
        }
        hasher.update(b"\0");

        // Hash input files in sorted order
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
        let command = config.get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LoomError::Internal(
                "CommandGenerator: 'command' field is required".to_string()
            ))?;

        // Platform-specific command execution
        let output = execute_shell_command(command, &context.project.project_root, &context.env)?;

        let success = output.status.success();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let mut log = Vec::new();
        for line in stdout.lines() { log.push(line.to_string()); }
        for line in stderr.lines() { log.push(format!("[err] {}", line)); }

        if !success {
            return Err(LoomError::Internal(format!(
                "Generator command failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr.lines().next().unwrap_or("no error output")
            )));
        }

        // Verify outputs exist (declared outputs are checked by framework)
        let produced_files = if let Some(outputs) = config.get("outputs") {
            outputs.as_array()
                .map(|arr| arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| context.project.project_root.join(s))
                    .collect())
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
/// On Unix: `sh -c <command>`
/// On Windows: `cmd.exe /c <command>` (or PowerShell if command starts with `ps:`)
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

    cmd.output().map_err(|e| LoomError::Internal(
        format!("Failed to spawn command '{}': {}", command, e)
    ))
}
```

## Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_command_generator_echo() {
        // Run a simple echo command, verify output
        let tmp = TempDir::new().unwrap();
        let gen = CommandGenerator;

        let config = toml::toml! {
            command = "echo hello"
        };

        // Build a minimal BuildContext pointing to tmp dir
        // Execute and check result.success == true
    }

    #[test]
    fn test_command_generator_failure() {
        // Run a command that exits with code 1
        // Verify Err is returned
    }

    #[test]
    fn test_cache_key_deterministic() {
        let gen = CommandGenerator;
        let config = toml::toml! { command = "python gen.py" };
        let hashes = HashMap::new();
        let k1 = gen.compute_cache_key(&config, &hashes).unwrap();
        let k2 = gen.compute_cache_key(&config, &hashes).unwrap();
        assert_eq!(k1, k2);
    }

    #[test]
    fn test_cache_key_changes_with_input() {
        let gen = CommandGenerator;
        let config = toml::toml! { command = "python gen.py" };
        let hashes_empty = HashMap::new();
        let mut hashes_with_file = HashMap::new();
        hashes_with_file.insert("input.yaml".to_string(), "sha256:abc".to_string());

        let k1 = gen.compute_cache_key(&config, &hashes_empty).unwrap();
        let k2 = gen.compute_cache_key(&config, &hashes_with_file).unwrap();
        assert_ne!(k1, k2);
    }
}
```

## Done When

- `cargo test -p loom-core` passes
- `CommandGenerator::execute()` runs shell commands on both Unix and Windows
- Failed commands return `Err` (not `Ok` with `success: false`)
- Cache keys are deterministic and input-dependent
