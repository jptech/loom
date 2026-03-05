use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LoomError {
    #[error("I/O error at '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to parse manifest '{path}': {message}")]
    ManifestParse { path: PathBuf, message: String },

    #[error("Manifest validation error in '{path}': {message}")]
    ManifestValidation { path: PathBuf, message: String },

    #[error("No workspace.toml found searching from '{start}'")]
    NoWorkspace { start: PathBuf },

    #[error("Project '{name}' not found in workspace")]
    ProjectNotFound { name: String },

    #[error("Dependency '{name}' (constraint: {constraint}) not found in workspace")]
    DependencyNotFound { name: String, constraint: String },

    #[error("Dependency cycle detected involving '{component}'")]
    DependencyCycle { component: String },

    #[error("Version constraint not satisfied for '{dependency}': required {required}, found {found} in {found_in}")]
    VersionNotSatisfied {
        dependency: String,
        required: String,
        found: String,
        found_in: PathBuf,
    },

    #[error("Ambiguous dependency '{name}': multiple candidates found: {candidates:?}")]
    AmbiguousDependency {
        name: String,
        candidates: Vec<String>,
    },

    #[error("Invalid version '{version}' for component '{component}'")]
    InvalidVersion { component: String, version: String },

    #[error("Invalid version requirement '{constraint}' for dependency '{dependency}'")]
    InvalidVersionReq {
        dependency: String,
        constraint: String,
    },

    #[error("Lockfile is stale: {reasons:?}")]
    LockfileStale { reasons: Vec<String> },

    #[error("Failed to write lockfile: {message}")]
    LockfileWrite { message: String },

    #[error("Failed to parse lockfile: {message}")]
    LockfileParse { message: String },

    #[error("Invalid glob pattern '{pattern}': {message}")]
    GlobPattern { pattern: String, message: String },

    #[error("Glob error: {message}")]
    GlobError { message: String },

    #[error("Validation failed with {error_count} error(s)")]
    ValidationFailed { error_count: usize },

    #[error("Build failed during {phase}: see log at {log_path}")]
    BuildFailed { phase: String, log_path: PathBuf },

    #[error("Tool '{tool}' not found: {message}")]
    ToolNotFound { tool: String, message: String },

    #[error("Tool version mismatch: required {required}, found {found}")]
    ToolVersionMismatch { required: String, found: String },

    #[error("Build interrupted by user")]
    Interrupted,

    #[error("Internal error: {0}")]
    Internal(String),
}

impl LoomError {
    pub fn exit_code(&self) -> i32 {
        match self {
            // 1 = build failure
            LoomError::BuildFailed { .. } => 1,
            LoomError::ValidationFailed { .. } => 1,

            // 2 = config error
            LoomError::ManifestParse { .. } => 2,
            LoomError::ManifestValidation { .. } => 2,
            LoomError::DependencyNotFound { .. } => 2,
            LoomError::DependencyCycle { .. } => 2,
            LoomError::VersionNotSatisfied { .. } => 2,
            LoomError::AmbiguousDependency { .. } => 2,
            LoomError::InvalidVersion { .. } => 2,
            LoomError::InvalidVersionReq { .. } => 2,
            LoomError::LockfileStale { .. } => 2,
            LoomError::LockfileParse { .. } => 2,
            LoomError::NoWorkspace { .. } => 2,
            LoomError::ProjectNotFound { .. } => 2,
            LoomError::GlobPattern { .. } => 2,

            // 3 = env error
            LoomError::ToolNotFound { .. } => 3,
            LoomError::ToolVersionMismatch { .. } => 3,

            // 130 = interrupted (128 + SIGINT)
            LoomError::Interrupted => 130,

            // 4 = internal
            LoomError::Io { .. } => 4,
            LoomError::Internal(_) => 4,
            LoomError::LockfileWrite { .. } => 4,
            LoomError::GlobError { .. } => 4,
        }
    }
}
