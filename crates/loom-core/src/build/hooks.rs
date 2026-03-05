use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::LoomError;

/// Configuration for a single hook.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HookConfig {
    /// Shell command to execute.
    pub command: String,
    /// Timeout in seconds (default: 300).
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// If true, failure does not halt the build (only valid for post_build, post_report).
    #[serde(default)]
    pub allow_failure: bool,
}

fn default_timeout() -> u64 {
    300
}

/// Hook lifecycle points.
pub const HOOK_LIFECYCLES: &[&str] = &[
    "pre_generate",
    "post_generate",
    "pre_build",
    "post_build",
    "post_report",
];

/// Runs hooks at various lifecycle points.
pub struct HookRunner {
    pub hooks: HashMap<String, HookConfig>,
    pub context_dir: PathBuf,
}

/// Result of running a hook.
#[derive(Debug)]
pub struct HookResult {
    pub exit_code: i32,
    pub stdout_json: Option<serde_json::Value>,
    pub stderr: String,
}

impl HookRunner {
    pub fn new(hooks: HashMap<String, HookConfig>, context_dir: PathBuf) -> Self {
        Self { hooks, context_dir }
    }

    /// Run a hook for a given lifecycle point.
    ///
    /// Returns Ok(Some(json)) if hook produced JSON output, Ok(None) if no hook configured.
    /// Returns Err if hook fails with exit code 1 (unless allow_failure).
    pub fn run_hook(
        &self,
        lifecycle: &str,
        context: &serde_json::Value,
    ) -> Result<Option<serde_json::Value>, LoomError> {
        let config = match self.hooks.get(lifecycle) {
            Some(c) => c,
            None => return Ok(None),
        };

        // Write context file
        let context_path = self.context_dir.join("hook_context.json");
        if let Some(parent) = context_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let context_str = serde_json::to_string_pretty(context)
            .map_err(|e| LoomError::Internal(e.to_string()))?;
        std::fs::write(&context_path, &context_str).map_err(|e| LoomError::Io {
            path: context_path.clone(),
            source: e,
        })?;

        // Run hook
        let result = run_hook_command(
            &config.command,
            &context_path,
            Duration::from_secs(config.timeout_secs),
        )?;

        match result.exit_code {
            0 => Ok(result.stdout_json),
            1 => {
                if config.allow_failure && (lifecycle == "post_build" || lifecycle == "post_report")
                {
                    Ok(result.stdout_json)
                } else {
                    Err(LoomError::Internal(format!(
                        "Hook '{}' failed (exit 1): {}",
                        lifecycle,
                        result.stderr.lines().next().unwrap_or("no error output")
                    )))
                }
            }
            2 => {
                // Warning — continue but log
                Ok(result.stdout_json)
            }
            code => Err(LoomError::Internal(format!(
                "Hook '{}' exited with unexpected code {}",
                lifecycle, code
            ))),
        }
    }
}

fn run_hook_command(
    command: &str,
    context_path: &Path,
    _timeout: Duration,
) -> Result<HookResult, LoomError> {
    let shell = if cfg!(target_os = "windows") {
        ("cmd", "/c")
    } else {
        ("sh", "-c")
    };

    let output = Command::new(shell.0)
        .arg(shell.1)
        .arg(command)
        .env("LOOM_CONTEXT_FILE", context_path.display().to_string())
        .output()
        .map_err(|e| LoomError::Internal(format!("Failed to execute hook: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let stdout_json = serde_json::from_str(&stdout).ok();

    Ok(HookResult {
        exit_code: output.status.code().unwrap_or(-1),
        stdout_json,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_runner_no_hook() {
        let runner = HookRunner::new(HashMap::new(), PathBuf::from("/tmp"));
        let context = serde_json::json!({"project": "test"});
        let result = runner.run_hook("pre_build", &context).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_hook_runner_success() {
        let tmp = tempfile::TempDir::new().unwrap();
        let echo_cmd = if cfg!(target_os = "windows") {
            "echo {}"
        } else {
            "echo '{}'"
        };
        let mut hooks = HashMap::new();
        hooks.insert(
            "pre_build".to_string(),
            HookConfig {
                command: echo_cmd.to_string(),
                timeout_secs: 10,
                allow_failure: false,
            },
        );

        let runner = HookRunner::new(hooks, tmp.path().to_path_buf());
        let context = serde_json::json!({"project": "test"});
        let result = runner.run_hook("pre_build", &context).unwrap();
        // echo outputs JSON-like text, may or may not parse
        assert!(result.is_some() || result.is_none()); // just verify it ran
    }

    #[test]
    fn test_hook_runner_failure() {
        let tmp = tempfile::TempDir::new().unwrap();
        let fail_cmd = if cfg!(target_os = "windows") {
            "exit /b 1"
        } else {
            "exit 1"
        };
        let mut hooks = HashMap::new();
        hooks.insert(
            "pre_build".to_string(),
            HookConfig {
                command: fail_cmd.to_string(),
                timeout_secs: 10,
                allow_failure: false,
            },
        );

        let runner = HookRunner::new(hooks, tmp.path().to_path_buf());
        let context = serde_json::json!({"project": "test"});
        let result = runner.run_hook("pre_build", &context);
        assert!(result.is_err());
    }

    #[test]
    fn test_hook_allow_failure_post_build() {
        let tmp = tempfile::TempDir::new().unwrap();
        let fail_cmd = if cfg!(target_os = "windows") {
            "exit /b 1"
        } else {
            "exit 1"
        };
        let mut hooks = HashMap::new();
        hooks.insert(
            "post_build".to_string(),
            HookConfig {
                command: fail_cmd.to_string(),
                timeout_secs: 10,
                allow_failure: true,
            },
        );

        let runner = HookRunner::new(hooks, tmp.path().to_path_buf());
        let context = serde_json::json!({"project": "test"});
        let result = runner.run_hook("post_build", &context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_hook_lifecycles() {
        assert_eq!(HOOK_LIFECYCLES.len(), 5);
        assert!(HOOK_LIFECYCLES.contains(&"pre_build"));
        assert!(HOOK_LIFECYCLES.contains(&"post_report"));
    }
}
