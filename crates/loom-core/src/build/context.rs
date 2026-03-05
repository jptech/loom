use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::resolve::resolver::ResolvedProject;

/// Build context passed to backend plugins.
#[derive(Debug, Clone)]
pub struct BuildContext {
    pub project: ResolvedProject,
    pub build_dir: PathBuf,
    pub workspace_root: PathBuf,
    pub env: HashMap<String, String>,
    pub strategy: String,
    /// Cancellation flag set by Ctrl+C handler.
    pub cancelled: Arc<AtomicBool>,
}

impl BuildContext {
    pub fn new(project: ResolvedProject, workspace_root: PathBuf) -> Self {
        let build_dir = workspace_root
            .join(".build")
            .join(&project.project.project.name)
            .join("default");

        Self {
            build_dir,
            workspace_root,
            env: std::env::vars().collect(),
            strategy: "default".to_string(),
            project,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}
