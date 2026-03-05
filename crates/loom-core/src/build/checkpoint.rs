use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::LoomError;

/// Persistent build state for resume support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildState {
    pub cache_key: String,
    pub backend: String,
    pub phases_completed: Vec<String>,
    pub phases_failed: Vec<String>,
    pub checkpoints: HashMap<String, PathBuf>,
    pub failure: Option<FailureInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureInfo {
    pub phase: String,
    pub exit_code: i32,
    pub log: PathBuf,
    pub summary: Option<String>,
}

/// The standard build phases in order.
pub const BUILD_PHASES: &[&str] = &[
    "synthesis",
    "optimize",
    "place",
    "route",
    "phys_optimize",
    "bitstream",
];

impl BuildState {
    /// Create a new empty build state.
    pub fn new(cache_key: String, backend: String) -> Self {
        Self {
            cache_key,
            backend,
            phases_completed: vec![],
            phases_failed: vec![],
            checkpoints: HashMap::new(),
            failure: None,
        }
    }

    /// Mark a phase as completed, optionally with a checkpoint path.
    pub fn complete_phase(&mut self, phase: &str, checkpoint: Option<PathBuf>) {
        if !self.phases_completed.contains(&phase.to_string()) {
            self.phases_completed.push(phase.to_string());
        }
        if let Some(cp) = checkpoint {
            self.checkpoints.insert(phase.to_string(), cp);
        }
    }

    /// Mark a phase as failed.
    pub fn fail_phase(
        &mut self,
        phase: &str,
        exit_code: i32,
        log: PathBuf,
        summary: Option<String>,
    ) {
        self.phases_failed.push(phase.to_string());
        self.failure = Some(FailureInfo {
            phase: phase.to_string(),
            exit_code,
            log,
            summary,
        });
    }

    /// Get the last completed phase.
    pub fn last_completed_phase(&self) -> Option<&str> {
        self.phases_completed.last().map(|s| s.as_str())
    }

    /// Get the checkpoint path for the last completed phase (for resume).
    pub fn resume_checkpoint(&self) -> Option<(&str, &PathBuf)> {
        self.last_completed_phase()
            .and_then(|phase| self.checkpoints.get(phase).map(|cp| (phase, cp)))
    }

    /// Determine which phases to run given start_at and stop_after constraints.
    pub fn phases_to_run(&self, start_at: Option<&str>, stop_after: Option<&str>) -> Vec<String> {
        let start_idx = start_at
            .and_then(|s| BUILD_PHASES.iter().position(|&p| p == s))
            .unwrap_or(0);

        let stop_idx = stop_after
            .and_then(|s| BUILD_PHASES.iter().position(|&p| p == s))
            .map(|i| i + 1)
            .unwrap_or(BUILD_PHASES.len());

        BUILD_PHASES[start_idx..stop_idx]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}

/// Path to the build state file.
pub fn build_state_path(build_dir: &Path) -> PathBuf {
    build_dir.join("build_state.json")
}

/// Load build state from disk.
pub fn load_build_state(build_dir: &Path) -> Result<Option<BuildState>, LoomError> {
    let path = build_state_path(build_dir);
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path).map_err(|e| LoomError::Io {
        path: path.clone(),
        source: e,
    })?;

    let state: BuildState = serde_json::from_str(&content)
        .map_err(|e| LoomError::Internal(format!("Failed to parse build state: {}", e)))?;

    Ok(Some(state))
}

/// Save build state to disk.
pub fn save_build_state(state: &BuildState, build_dir: &Path) -> Result<(), LoomError> {
    std::fs::create_dir_all(build_dir).map_err(|e| LoomError::Io {
        path: build_dir.to_owned(),
        source: e,
    })?;

    let path = build_state_path(build_dir);
    let content =
        serde_json::to_string_pretty(state).map_err(|e| LoomError::Internal(e.to_string()))?;

    std::fs::write(&path, content).map_err(|e| LoomError::Io { path, source: e })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_state_lifecycle() {
        let mut state = BuildState::new("abc123".to_string(), "vivado".to_string());
        assert!(state.last_completed_phase().is_none());

        state.complete_phase("synthesis", Some(PathBuf::from("/build/post_synth.dcp")));
        state.complete_phase("optimize", None);
        state.complete_phase("place", Some(PathBuf::from("/build/post_place.dcp")));

        assert_eq!(state.last_completed_phase(), Some("place"));
        assert_eq!(state.phases_completed.len(), 3);

        let (phase, cp) = state.resume_checkpoint().unwrap();
        assert_eq!(phase, "place");
        assert_eq!(cp, &PathBuf::from("/build/post_place.dcp"));
    }

    #[test]
    fn test_phases_to_run_default() {
        let state = BuildState::new("key".to_string(), "vivado".to_string());
        let phases = state.phases_to_run(None, None);
        assert_eq!(phases.len(), BUILD_PHASES.len());
    }

    #[test]
    fn test_phases_to_run_start_at() {
        let state = BuildState::new("key".to_string(), "vivado".to_string());
        let phases = state.phases_to_run(Some("place"), None);
        assert_eq!(phases[0], "place");
        assert!(phases.contains(&"route".to_string()));
    }

    #[test]
    fn test_phases_to_run_stop_after() {
        let state = BuildState::new("key".to_string(), "vivado".to_string());
        let phases = state.phases_to_run(None, Some("place"));
        assert!(phases.contains(&"synthesis".to_string()));
        assert!(phases.contains(&"place".to_string()));
        assert!(!phases.contains(&"route".to_string()));
    }

    #[test]
    fn test_phases_to_run_range() {
        let state = BuildState::new("key".to_string(), "vivado".to_string());
        let phases = state.phases_to_run(Some("optimize"), Some("route"));
        assert_eq!(phases, vec!["optimize", "place", "route"]);
    }

    #[test]
    fn test_save_and_load() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut state = BuildState::new("abc123".to_string(), "vivado".to_string());
        state.complete_phase("synthesis", Some(PathBuf::from("/build/post_synth.dcp")));

        save_build_state(&state, tmp.path()).unwrap();

        let loaded = load_build_state(tmp.path()).unwrap().unwrap();
        assert_eq!(loaded.cache_key, "abc123");
        assert_eq!(loaded.backend, "vivado");
        assert_eq!(loaded.phases_completed, vec!["synthesis"]);
    }

    #[test]
    fn test_fail_phase() {
        let mut state = BuildState::new("key".to_string(), "vivado".to_string());
        state.complete_phase("synthesis", None);
        state.fail_phase(
            "place",
            1,
            PathBuf::from("/build/place.log"),
            Some("Timing violation".to_string()),
        );

        assert!(state.failure.is_some());
        let failure = state.failure.unwrap();
        assert_eq!(failure.phase, "place");
        assert_eq!(failure.exit_code, 1);
    }
}
