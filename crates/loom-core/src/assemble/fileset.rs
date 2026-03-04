use std::path::PathBuf;

use crate::error::LoomError;
use crate::resolve::resolver::ResolvedProject;

use super::ordering::sort_constraints;

/// The complete, ordered set of files to pass to the backend.
#[derive(Debug, Clone)]
pub struct AssembledFilesets {
    pub synth_files: Vec<AssembledFile>,
    pub constraint_files: Vec<AssembledConstraint>,
    pub defines: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AssembledFile {
    pub path: PathBuf,
    pub source_component: String,
    pub language: FileLanguage,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileLanguage {
    SystemVerilog,
    Verilog,
    Vhdl,
    Unknown,
}

impl FileLanguage {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "sv" | "svh" => FileLanguage::SystemVerilog,
            "v" | "vh" => FileLanguage::Verilog,
            "vhd" | "vhdl" => FileLanguage::Vhdl,
            _ => FileLanguage::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AssembledConstraint {
    pub path: PathBuf,
    pub source_component: String,
    pub scope: ConstraintScope,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstraintScope {
    Component { ref_name: String },
    Global,
}

/// Assemble all filesets from a resolved project.
pub fn assemble_filesets(resolved: &ResolvedProject) -> Result<AssembledFilesets, LoomError> {
    let mut synth_files: Vec<AssembledFile> = Vec::new();
    let mut constraint_files: Vec<AssembledConstraint> = Vec::new();
    let mut defines: Vec<String> = Vec::new();

    // 1. Add dependency files (topological order -- leaf deps first)
    for component in &resolved.resolved_components {
        let comp_dir = &component.source_path;
        let comp_name = &component.manifest.component.name;

        if let Some(synth_fs) = component.manifest.filesets.get("synth") {
            for rel_path in &synth_fs.files {
                let abs_path = comp_dir.join(rel_path);
                let language = FileLanguage::from_extension(
                    abs_path.extension().and_then(|e| e.to_str()).unwrap_or(""),
                );
                synth_files.push(AssembledFile {
                    path: abs_path,
                    source_component: comp_name.clone(),
                    language,
                });
            }

            for rel_path in &synth_fs.constraints {
                let abs_path = comp_dir.join(rel_path);
                let scope = if synth_fs.constraint_scope == "global" {
                    ConstraintScope::Global
                } else {
                    let short_name = comp_name
                        .rsplit('/')
                        .next()
                        .unwrap_or(comp_name.as_str())
                        .to_string();
                    ConstraintScope::Component {
                        ref_name: short_name,
                    }
                };
                constraint_files.push(AssembledConstraint {
                    path: abs_path,
                    source_component: comp_name.clone(),
                    scope,
                });
            }

            defines.extend(synth_fs.defines.iter().cloned());
        }
    }

    // 2. Add project's own files
    let proj_dir = &resolved.project_root;
    let proj_name = &resolved.project.project.name;

    if let Some(synth_fs) = resolved.project.filesets.get("synth") {
        for rel_path in &synth_fs.files {
            let abs_path = proj_dir.join(rel_path);
            let language = FileLanguage::from_extension(
                abs_path.extension().and_then(|e| e.to_str()).unwrap_or(""),
            );
            synth_files.push(AssembledFile {
                path: abs_path,
                source_component: proj_name.clone(),
                language,
            });
        }

        for rel_path in &synth_fs.constraints {
            constraint_files.push(AssembledConstraint {
                path: proj_dir.join(rel_path),
                source_component: proj_name.clone(),
                scope: ConstraintScope::Global,
            });
        }

        defines.extend(synth_fs.defines.iter().cloned());
    }

    constraint_files = sort_constraints(constraint_files);

    Ok(AssembledFilesets {
        synth_files,
        constraint_files,
        defines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::resolver::{resolve_project, WorkspaceDependencySource};
    use crate::resolve::workspace::{
        discover_members, find_project, find_workspace_root, load_all_components,
    };

    fn resolve_simple_project() -> ResolvedProject {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/simple_project");
        let (root, ws_manifest) = find_workspace_root(&fixture).unwrap();
        let members = discover_members(&root, &ws_manifest).unwrap();
        let all_components = load_all_components(&members).unwrap();
        let (project_root, project_manifest) = find_project(&members, None).unwrap();
        let source = WorkspaceDependencySource::new(all_components);
        resolve_project(project_manifest, project_root, root, &source).unwrap()
    }

    #[test]
    fn test_synth_files_ordered_deps_first() {
        let resolved = resolve_simple_project();
        let filesets = assemble_filesets(&resolved).unwrap();

        let names: Vec<&str> = filesets
            .synth_files
            .iter()
            .map(|f| f.source_component.as_str())
            .collect();

        let axi_idx = names
            .iter()
            .position(|&n| n == "testorg/axi_common")
            .unwrap();
        let proj_idx = names.iter().position(|&n| n == "my_design").unwrap();
        assert!(
            axi_idx < proj_idx,
            "Dependencies must come before project files"
        );
    }

    #[test]
    fn test_constraint_scoping_order() {
        let resolved = resolve_simple_project();
        let filesets = assemble_filesets(&resolved).unwrap();

        let mut saw_global = false;
        for constraint in &filesets.constraint_files {
            match &constraint.scope {
                ConstraintScope::Global => saw_global = true,
                ConstraintScope::Component { .. } => {
                    assert!(
                        !saw_global,
                        "Component-scoped constraints must come before global"
                    );
                }
            }
        }
    }

    #[test]
    fn test_file_language_detection() {
        assert_eq!(
            FileLanguage::from_extension("sv"),
            FileLanguage::SystemVerilog
        );
        assert_eq!(FileLanguage::from_extension("vhd"), FileLanguage::Vhdl);
        assert_eq!(FileLanguage::from_extension("v"), FileLanguage::Verilog);
    }

    #[test]
    fn test_absolute_paths() {
        let resolved = resolve_simple_project();
        let filesets = assemble_filesets(&resolved).unwrap();

        for file in &filesets.synth_files {
            assert!(
                file.path.is_absolute() || file.path.starts_with("\\\\"),
                "Path should be absolute: {}",
                file.path.display()
            );
        }
    }
}
