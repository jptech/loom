use clap::Args;

use loom_core::assemble::assemble_filesets;
use loom_core::assemble::fileset::FileLanguage;
use loom_core::error::LoomError;
use loom_core::resolve::{
    discover_members, find_project, find_workspace_root, load_all_components, resolve_project,
    WorkspaceDependencySource,
};

use crate::GlobalContext;

#[derive(Args)]
pub struct LspArgs {
    /// Output format: loom (default), svls, verible, slang
    #[arg(long, default_value = "loom")]
    pub format: String,

    /// Project name (default: auto-detect)
    #[arg(short = 'p', long)]
    pub project: Option<String>,
}

pub fn run(args: LspArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let cwd = std::env::current_dir().map_err(|e| LoomError::Io {
        path: ".".into(),
        source: e,
    })?;

    let (workspace_root, ws_manifest) = find_workspace_root(&cwd)?;
    let members = discover_members(&workspace_root, &ws_manifest)?;
    let all_components = load_all_components(&members)?;

    let (project_root, project_manifest) = match &args.project {
        Some(name) => find_project(&members, Some(name))?,
        None => find_project(&members, None)?,
    };

    let source = WorkspaceDependencySource::new(all_components);
    let resolved = resolve_project(
        project_manifest,
        project_root,
        workspace_root.clone(),
        &source,
    )?;

    let filesets = assemble_filesets(&resolved)?;

    match args.format.as_str() {
        "loom" => {
            let lsp_dir = workspace_root.join(".loom");
            std::fs::create_dir_all(&lsp_dir).map_err(|e| LoomError::Io {
                path: lsp_dir.clone(),
                source: e,
            })?;

            let files: Vec<serde_json::Value> = filesets
                .synth_files
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "path": f.path.display().to_string(),
                        "language": match f.language {
                            FileLanguage::SystemVerilog => "systemverilog",
                            FileLanguage::Verilog => "verilog",
                            FileLanguage::Vhdl => "vhdl",
                            FileLanguage::Unknown => "unknown",
                        }
                    })
                })
                .collect();

            let lsp_config = serde_json::json!({
                "version": 1,
                "project": resolved.project.project.name,
                "defines": filesets.defines,
                "include_dirs": [],
                "files": files,
            });

            let output_path = lsp_dir.join("lsp.json");
            let content = serde_json::to_string_pretty(&lsp_config).unwrap_or_default();
            std::fs::write(&output_path, &content).map_err(|e| LoomError::Io {
                path: output_path.clone(),
                source: e,
            })?;

            if !ctx.quiet {
                eprintln!("  Wrote {}", output_path.display());
            }
        }
        "svls" => {
            let mut toml_content = String::from("[verilog]\n");
            toml_content.push_str("include_dirs = []\n");
            toml_content.push_str("defines = [");
            for (i, d) in filesets.defines.iter().enumerate() {
                if i > 0 {
                    toml_content.push_str(", ");
                }
                toml_content.push_str(&format!("\"{}\"", d));
            }
            toml_content.push_str("]\n");

            let output_path = workspace_root.join(".svls.toml");
            std::fs::write(&output_path, &toml_content).map_err(|e| LoomError::Io {
                path: output_path.clone(),
                source: e,
            })?;

            if !ctx.quiet {
                eprintln!("  Wrote {}", output_path.display());
            }
        }
        "verible" => {
            let mut file_list = String::new();
            for f in &filesets.synth_files {
                file_list.push_str(&f.path.display().to_string());
                file_list.push('\n');
            }

            let output_path = workspace_root.join("verible.filelist");
            std::fs::write(&output_path, &file_list).map_err(|e| LoomError::Io {
                path: output_path.clone(),
                source: e,
            })?;

            if !ctx.quiet {
                eprintln!("  Wrote {}", output_path.display());
            }
        }
        "slang" => {
            let mut args_content = String::new();
            for d in &filesets.defines {
                args_content.push_str(&format!("--define {}\n", d));
            }
            for f in &filesets.synth_files {
                args_content.push_str(&f.path.display().to_string());
                args_content.push('\n');
            }

            let output_path = workspace_root.join("slang.args");
            std::fs::write(&output_path, &args_content).map_err(|e| LoomError::Io {
                path: output_path.clone(),
                source: e,
            })?;

            if !ctx.quiet {
                eprintln!("  Wrote {}", output_path.display());
            }
        }
        other => {
            return Err(LoomError::Internal(format!(
                "Unknown LSP format '{}'. Supported: loom, svls, verible, slang",
                other
            )));
        }
    }

    Ok(())
}
