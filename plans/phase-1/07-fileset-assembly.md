# Task 07: File-Set Assembly

**Prerequisites:** Task 06 complete
**Goal:** Collect all source files and constraint files from the resolved project and its transitive dependencies, apply constraint scoping, and produce a flat, ordered `AssembledFilesets`.

## Spec Reference
`system_plan.md` §3.3 (Constraint Scoping), §15 Phase 1 Core Data Types, §15 Phase 1 Build Pipeline

## File to Implement
`crates/loom-core/src/assemble/fileset.rs`
`crates/loom-core/src/assemble/ordering.rs`

## Types

```rust
// assemble/fileset.rs
use std::path::PathBuf;
use crate::resolve::resolver::ResolvedProject;
use crate::error::LoomError;

/// The complete, ordered set of files to pass to the backend.
/// This is the output of Phase 3 (ASSEMBLE).
#[derive(Debug, Clone)]
pub struct AssembledFilesets {
    /// Source files in compilation order: dependencies first, project last.
    pub synth_files: Vec<AssembledFile>,
    /// Constraint files: component-scoped first, then global.
    pub constraint_files: Vec<AssembledConstraint>,
    /// Preprocessor defines (from all components + project).
    pub defines: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AssembledFile {
    pub path: PathBuf,              // absolute path
    pub source_component: String,  // component name or "project"
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
    pub path: PathBuf,              // absolute path
    pub source_component: String,
    pub scope: ConstraintScope,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstraintScope {
    /// Scoped to component hierarchy. Backend applies SCOPED_TO_REF or equivalent.
    Component { ref_name: String },  // ref_name = component's top module name
    Global,
}
```

## Assembly Function

```rust
/// Assemble all filesets from a resolved project.
/// Returns files in the order required by the backend:
///   1. Source files: dependencies in topological order, project last.
///   2. Constraint files: component-scoped first (in dep order), global last.
///   3. Defines: merged from all components + project.
pub fn assemble_filesets(resolved: &ResolvedProject) -> Result<AssembledFilesets, LoomError> {
    let mut synth_files: Vec<AssembledFile> = Vec::new();
    let mut constraint_files: Vec<AssembledConstraint> = Vec::new();
    let mut defines: Vec<String> = Vec::new();

    // 1. Add dependency files (topological order — leaf deps first)
    for component in &resolved.resolved_components {
        let comp_dir = &component.source_path;
        let comp_name = &component.manifest.component.name;

        if let Some(synth_fs) = component.manifest.filesets.get("synth") {
            // Source files
            for rel_path in &synth_fs.files {
                let abs_path = comp_dir.join(rel_path);
                let ext = abs_path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                synth_files.push(AssembledFile {
                    path: abs_path,
                    source_component: comp_name.clone(),
                    language: FileLanguage::from_extension(ext),
                });
            }

            // Constraint files
            for rel_path in &synth_fs.constraints {
                let abs_path = comp_dir.join(rel_path);
                let scope = if synth_fs.constraint_scope == "global" {
                    ConstraintScope::Global
                } else {
                    // Component scope: use the short component name as the ref
                    let short_name = comp_name.rsplit('/').next()
                        .unwrap_or(comp_name.as_str())
                        .to_string();
                    ConstraintScope::Component { ref_name: short_name }
                };
                constraint_files.push(AssembledConstraint {
                    path: abs_path,
                    source_component: comp_name.clone(),
                    scope,
                });
            }

            // Defines
            defines.extend(synth_fs.defines.iter().cloned());
        }
    }

    // 2. Add project's own files
    let proj_dir = &resolved.project_root;
    let proj_name = &resolved.project.project.name;

    if let Some(synth_fs) = resolved.project.filesets.get("synth") {
        for rel_path in &synth_fs.files {
            let abs_path = proj_dir.join(rel_path);
            let ext = abs_path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            synth_files.push(AssembledFile {
                path: abs_path,
                source_component: proj_name.clone(),
                language: FileLanguage::from_extension(ext),
            });
        }

        // Project constraints go last (global by default for project-level constraints)
        for rel_path in &synth_fs.constraints {
            constraint_files.push(AssembledConstraint {
                path: proj_dir.join(rel_path),
                source_component: proj_name.clone(),
                scope: ConstraintScope::Global,  // project constraints are always global
            });
        }

        defines.extend(synth_fs.defines.iter().cloned());
    }

    // Sort constraints: component-scoped first, global last
    // Within each group, maintain topological order
    constraint_files = sort_constraints(constraint_files);

    Ok(AssembledFilesets {
        synth_files,
        constraint_files,
        defines,
    })
}
```

## Constraint Ordering (`ordering.rs`)

```rust
use super::fileset::{AssembledConstraint, ConstraintScope};

/// Sort constraints: component-scoped before global.
/// Within each group, original insertion order is preserved (topological).
pub fn sort_constraints(mut constraints: Vec<AssembledConstraint>) -> Vec<AssembledConstraint> {
    constraints.sort_by_key(|c| match &c.scope {
        ConstraintScope::Component { .. } => 0,
        ConstraintScope::Global => 1,
    });
    constraints
}
```

## Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn resolve_simple_project() -> ResolvedProject {
        // Helper: resolve the simple_project fixture
        // (reuse code from resolver tests)
        todo!()
    }

    #[test]
    fn test_synth_files_ordered_deps_first() {
        let resolved = resolve_simple_project();
        let filesets = assemble_filesets(&resolved).unwrap();

        // axi_common should come before my_design's top.sv
        let names: Vec<&str> = filesets.synth_files.iter()
            .map(|f| f.source_component.as_str())
            .collect();

        // Dependencies before project
        let axi_idx = names.iter().position(|&n| n == "testorg/axi_common").unwrap();
        let proj_idx = names.iter().position(|&n| n == "my_design").unwrap();
        assert!(axi_idx < proj_idx, "Dependencies must come before project files");
    }

    #[test]
    fn test_constraint_scoping_order() {
        // Component-scoped constraints before global
        let resolved = resolve_simple_project();
        let filesets = assemble_filesets(&resolved).unwrap();

        let mut saw_global = false;
        for constraint in &filesets.constraint_files {
            match &constraint.scope {
                ConstraintScope::Global => saw_global = true,
                ConstraintScope::Component { .. } => {
                    assert!(!saw_global, "Component-scoped constraints must come before global");
                }
            }
        }
    }

    #[test]
    fn test_file_language_detection() {
        assert_eq!(FileLanguage::from_extension("sv"), FileLanguage::SystemVerilog);
        assert_eq!(FileLanguage::from_extension("vhd"), FileLanguage::Vhdl);
        assert_eq!(FileLanguage::from_extension("v"), FileLanguage::Verilog);
    }
}
```

## Done When

- `cargo test -p loom-core` passes
- `assemble_filesets()` produces correct ordering for `simple_project`
- Component synth files appear before project synth files
- Component-scoped constraints appear before global constraints
- All paths in `AssembledFile.path` are absolute
