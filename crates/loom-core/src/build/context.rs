use std::collections::HashMap;
use std::path::PathBuf;

use crate::resolve::resolver::ResolvedProject;

/// Build context passed to backend plugins.
#[derive(Debug, Clone)]
pub struct BuildContext {
    pub project: ResolvedProject,
    pub build_dir: PathBuf,
    pub workspace_root: PathBuf,
    pub env: HashMap<String, String>,
    pub strategy: String,
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
        }
    }
}
