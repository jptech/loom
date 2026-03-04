# Task 13: Error Types and Diagnostic Formatting

**Prerequisites:** Task 12 complete (note: a minimal `LoomError` with `Io` and `Internal` variants should be stubbed during Task 01 and extended incrementally as each task introduces new error cases)
**Goal:** Implement the `LoomError` enum covering all error cases, exit code mapping, and human-readable formatting with source location.

## Spec Reference
`system_plan.md` §12.4 (Exit Codes), §12.1 (Clear error messages)

## File to Implement
`crates/loom-core/src/error.rs`

## Exit Codes (from spec §12.4)

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Build failed (timing not met, synthesis error) |
| 2 | Configuration error (bad manifest, missing dependency) |
| 3 | Environment error (tool not found, wrong version, license) |
| 4 | Internal error |

## `LoomError` Enum

```rust
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LoomError {
    // ─── I/O errors ───────────────────────────────────────────────────────────
    #[error("I/O error at '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    // ─── Manifest parsing errors (exit code 2) ────────────────────────────────
    #[error("Failed to parse manifest '{path}': {message}")]
    ManifestParse { path: PathBuf, message: String },

    #[error("Invalid manifest at '{path}': {message}")]
    ManifestValidation { path: PathBuf, message: String },

    // ─── Workspace errors (exit code 2) ───────────────────────────────────────
    #[error(
        "No workspace.toml found starting from '{start}'.\n\
         Run `loom new workspace` to initialize a workspace, or cd into a workspace directory."
    )]
    NoWorkspace { start: PathBuf },

    #[error("Project '{name}' not found in workspace. Run `loom deps tree` to see available projects.")]
    ProjectNotFound { name: String },

    // ─── Dependency resolution errors (exit code 2) ───────────────────────────
    #[error(
        "Dependency '{name}' (constraint: {constraint}) was not found in the workspace.\n\
         Run `loom deps tree` to inspect the dependency graph."
    )]
    DependencyNotFound { name: String, constraint: String },

    #[error(
        "Circular dependency detected involving component '{component}'.\n\
         Run `loom deps tree` to view the dependency graph and identify the cycle."
    )]
    DependencyCycle { component: String },

    #[error(
        "Version conflict for dependency '{dependency}': required {required}, \
         but found {found} at '{found_in}'."
    )]
    VersionNotSatisfied {
        dependency: String,
        required: String,
        found: String,
        found_in: PathBuf,
    },

    #[error(
        "Ambiguous dependency '{name}': multiple workspace components match. \
         Use the full 'org/name' form. Candidates: {candidates:?}"
    )]
    AmbiguousDependency { name: String, candidates: Vec<String> },

    #[error("Invalid semver version '{version}' for component '{component}'.")]
    InvalidVersion { component: String, version: String },

    #[error("Invalid version requirement '{constraint}' for dependency '{dependency}'.")]
    InvalidVersionReq { dependency: String, constraint: String },

    // ─── Lockfile errors (exit code 2) ────────────────────────────────────────
    #[error(
        "Lockfile is stale. Run `loom deps update` to regenerate it.\nReasons:\n{reasons}"
    )]
    LockfileStale { reasons: String },

    #[error("Failed to write lockfile: {message}")]
    LockfileWrite { message: String },

    #[error("Failed to parse lockfile: {message}")]
    LockfileParse { message: String },

    // ─── File-set errors (exit code 2) ────────────────────────────────────────
    #[error("Glob pattern error in '{pattern}': {message}")]
    GlobPattern { pattern: String, message: String },

    #[error("Glob traversal error: {message}")]
    GlobError { message: String },

    // ─── Build errors (exit code 1) ───────────────────────────────────────────
    #[error("Build failed in phase '{phase}'. See log: '{log_path}'")]
    BuildFailed { phase: String, log_path: PathBuf },

    #[error("Pre-build validation failed with {error_count} error(s).")]
    ValidationFailed { error_count: usize },

    // ─── Environment errors (exit code 3) ─────────────────────────────────────
    #[error(
        "Tool '{tool}' not found. {message}"
    )]
    ToolNotFound { tool: String, message: String },

    #[error(
        "Tool version mismatch: project requires '{required}', but '{found}' is installed.\n\
         Update the platform/project configuration or install the correct version."
    )]
    ToolVersionMismatch { required: String, found: String },

    // ─── Internal errors (exit code 4) ────────────────────────────────────────
    #[error("Internal error: {0}")]
    Internal(String),
}

impl LoomError {
    /// Map this error to a CLI exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            // Build failures → 1
            LoomError::BuildFailed { .. } | LoomError::ValidationFailed { .. } => 1,

            // Configuration errors → 2
            LoomError::ManifestParse { .. }
            | LoomError::ManifestValidation { .. }
            | LoomError::NoWorkspace { .. }
            | LoomError::ProjectNotFound { .. }
            | LoomError::DependencyNotFound { .. }
            | LoomError::DependencyCycle { .. }
            | LoomError::VersionNotSatisfied { .. }
            | LoomError::AmbiguousDependency { .. }
            | LoomError::InvalidVersion { .. }
            | LoomError::InvalidVersionReq { .. }
            | LoomError::LockfileStale { .. }
            | LoomError::LockfileWrite { .. }
            | LoomError::LockfileParse { .. }
            | LoomError::GlobPattern { .. }
            | LoomError::GlobError { .. } => 2,

            // Environment errors → 3
            LoomError::ToolNotFound { .. } | LoomError::ToolVersionMismatch { .. } => 3,

            // Internal errors and I/O → 4
            LoomError::Internal(_) | LoomError::Io { .. } => 4,
        }
    }
}
```

## Error Display Utilities (for CLI)

In `crates/loom-cli/src/main.rs`, implement error display:

```rust
/// Display a LoomError to stderr with formatting appropriate for the exit code.
/// Phase 1: plain text. Phase 2: colored output.
pub fn display_error(err: &loom_core::error::LoomError) {
    let prefix = match err.exit_code() {
        1 => "Build error",
        2 => "Configuration error",
        3 => "Environment error",
        _ => "Error",
    };
    eprintln!("{}: {}", prefix, err);
}
```

## Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_codes() {
        assert_eq!(LoomError::BuildFailed {
            phase: "synthesis".to_string(),
            log_path: PathBuf::from("/tmp/build.log"),
        }.exit_code(), 1);

        assert_eq!(LoomError::DependencyNotFound {
            name: "axi_common".to_string(),
            constraint: ">=1.0.0".to_string(),
        }.exit_code(), 2);

        assert_eq!(LoomError::ToolNotFound {
            tool: "vivado".to_string(),
            message: "not found".to_string(),
        }.exit_code(), 3);

        assert_eq!(LoomError::Internal("oops".to_string()).exit_code(), 4);
    }

    #[test]
    fn test_error_messages_are_actionable() {
        // Verify that error messages include "what to do" guidance
        let err = LoomError::DependencyNotFound {
            name: "missing_lib".to_string(),
            constraint: ">=1.0.0".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("missing_lib"));
        assert!(msg.contains("loom deps tree"), "Error should suggest running `loom deps tree`");
    }

    #[test]
    fn test_dependency_cycle_message() {
        let err = LoomError::DependencyCycle {
            component: "org/my_component".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("org/my_component"));
        assert!(msg.contains("cycle") || msg.contains("Circular"));
    }
}
```

## Done When

- `cargo test -p loom-core` passes
- All error variants defined, covering all cases referenced in tasks 01-12
- `exit_code()` returns correct code for each error category
- Error messages include actionable guidance (what to run next)
- `thiserror::Error` derive works correctly for all variants
