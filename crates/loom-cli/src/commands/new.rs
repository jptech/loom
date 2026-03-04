use std::path::PathBuf;

use clap::{Args, Subcommand};

use loom_core::error::LoomError;

use crate::GlobalContext;

#[derive(Subcommand)]
pub enum NewCommands {
    /// Create a new component
    Component(NewComponentArgs),

    /// Create a new project
    Project(NewProjectArgs),

    /// Create a new platform
    Platform(NewPlatformArgs),
}

#[derive(Args)]
pub struct NewComponentArgs {
    /// Path to create the component at
    pub path: PathBuf,

    /// Component name (default: last path component with 'myorg/' prefix)
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Args)]
pub struct NewProjectArgs {
    /// Path to create the project at
    pub path: PathBuf,

    /// Project name (default: last path component)
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Args)]
pub struct NewPlatformArgs {
    /// Path to create the platform at
    pub path: PathBuf,

    /// Platform name (default: last path component)
    #[arg(long)]
    pub name: Option<String>,
}

pub fn run(cmd: NewCommands, ctx: &GlobalContext) -> Result<(), LoomError> {
    match cmd {
        NewCommands::Component(args) => run_new_component(args, ctx),
        NewCommands::Project(args) => run_new_project(args, ctx),
        NewCommands::Platform(args) => run_new_platform(args, ctx),
    }
}

fn run_new_component(args: NewComponentArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let path = &args.path;
    let dir_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("my_component");
    let name = args.name.unwrap_or_else(|| format!("myorg/{}", dir_name));

    create_dir(path)?;
    create_dir(&path.join("rtl"))?;
    create_dir(&path.join("tb"))?;
    create_dir(&path.join("constraints"))?;

    let content = format!(
        r#"[component]
name = "{name}"
version = "1.0.0"
description = "TODO: describe this component"

[filesets.synth]
files = ["rtl/{dir_name}.sv"]
constraints = []

[filesets.sim]
files = ["tb/{dir_name}_tb.sv"]
include_synth = true

[synth]
ooc = false
"#
    );

    write_file(&path.join("component.toml"), &content)?;

    // Create stub RTL file
    let rtl_content = format!(
        "module {} (\n    input  logic clk,\n    input  logic rst_n\n);\n\n    // TODO: implement\n\nendmodule\n",
        dir_name
    );
    write_file(
        &path.join("rtl").join(format!("{}.sv", dir_name)),
        &rtl_content,
    )?;

    if !ctx.quiet {
        eprintln!("  Created component '{}' at {}", name, path.display());
    }

    Ok(())
}

fn run_new_project(args: NewProjectArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let path = &args.path;
    let dir_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("my_project");
    let name = args.name.unwrap_or_else(|| dir_name.to_string());

    create_dir(path)?;
    create_dir(&path.join("src"))?;
    create_dir(&path.join("tb"))?;
    create_dir(&path.join("constraints"))?;

    let content = format!(
        r#"[project]
name = "{name}"
top_module = "top"
description = "TODO: describe this project"
# platform = "zcu104"  # uncomment to use a platform

[target]
part = "xc7a35t"
backend = "vivado"
# version = "2023.2"

[filesets.synth]
files = ["src/top.sv"]
constraints = ["constraints/timing.xdc"]

[filesets.sim]
files = ["tb/top_tb.sv"]
include_synth = true

[dependencies]
# Add component dependencies here
# my_component = ">=1.0.0"
"#
    );

    write_file(&path.join("project.toml"), &content)?;

    // Create stub top module
    let rtl_content = "module top (\n    input  logic clk,\n    input  logic rst_n\n);\n\n    // TODO: implement\n\nendmodule\n";
    write_file(&path.join("src").join("top.sv"), rtl_content)?;

    // Create stub constraint file
    let xdc_content =
        "# Timing constraints\n# create_clock -period 10.000 -name sys_clk [get_ports clk]\n";
    write_file(&path.join("constraints").join("timing.xdc"), xdc_content)?;

    if !ctx.quiet {
        eprintln!("  Created project '{}' at {}", name, path.display());
    }

    Ok(())
}

fn run_new_platform(args: NewPlatformArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let path = &args.path;
    let dir_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("my_platform");
    let name = args.name.unwrap_or_else(|| dir_name.to_string());

    create_dir(path)?;
    create_dir(&path.join("constraints"))?;

    let content = format!(
        r#"[platform]
name = "{name}"
description = "TODO: describe this platform"
part = "xczu7ev-ffvc1156-2-e"
# virtual_platform = false

[platform.tool]
backend = "vivado"
# version = "2023.2"

[platform.clocks.sys_clk]
frequency_mhz = 125.0
period_ns = 8.0
pin = "H9"
standard = "LVDS"
description = "System clock"

[platform.constraints]
files = ["constraints/pins.xdc"]

[platform.params]
# Add platform-specific parameters here
# ddr4_data_width = 64

[platform.variant_defaults]
tags = ["vendor:xilinx"]
"#
    );

    write_file(&path.join("platform.toml"), &content)?;

    // Create stub constraint file
    let xdc_content = "# Pin assignments\n# set_property PACKAGE_PIN H9 [get_ports sys_clk_p]\n# set_property IOSTANDARD LVDS [get_ports sys_clk_p]\n";
    write_file(&path.join("constraints").join("pins.xdc"), xdc_content)?;

    if !ctx.quiet {
        eprintln!("  Created platform '{}' at {}", name, path.display());
    }

    Ok(())
}

fn create_dir(path: &std::path::Path) -> Result<(), LoomError> {
    std::fs::create_dir_all(path).map_err(|e| LoomError::Io {
        path: path.to_owned(),
        source: e,
    })
}

fn write_file(path: &std::path::Path, content: &str) -> Result<(), LoomError> {
    std::fs::write(path, content).map_err(|e| LoomError::Io {
        path: path.to_owned(),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_component_creates_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("my_fifo");
        let ctx = crate::GlobalContext {
            verbose: 0,
            quiet: true,
            json: false,
            no_color: false,
        };

        run_new_component(
            NewComponentArgs {
                path: path.clone(),
                name: None,
            },
            &ctx,
        )
        .unwrap();

        assert!(path.join("component.toml").exists());
        assert!(path.join("rtl").is_dir());
        assert!(path.join("tb").is_dir());
        assert!(path.join("constraints").is_dir());
        assert!(path.join("rtl/my_fifo.sv").exists());

        let content = std::fs::read_to_string(path.join("component.toml")).unwrap();
        assert!(content.contains("myorg/my_fifo"));
    }

    #[test]
    fn test_new_project_creates_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("my_design");
        let ctx = crate::GlobalContext {
            verbose: 0,
            quiet: true,
            json: false,
            no_color: false,
        };

        run_new_project(
            NewProjectArgs {
                path: path.clone(),
                name: None,
            },
            &ctx,
        )
        .unwrap();

        assert!(path.join("project.toml").exists());
        assert!(path.join("src").is_dir());
        assert!(path.join("src/top.sv").exists());
        assert!(path.join("constraints/timing.xdc").exists());
    }

    #[test]
    fn test_new_platform_creates_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("zcu104");
        let ctx = crate::GlobalContext {
            verbose: 0,
            quiet: true,
            json: false,
            no_color: false,
        };

        run_new_platform(
            NewPlatformArgs {
                path: path.clone(),
                name: None,
            },
            &ctx,
        )
        .unwrap();

        assert!(path.join("platform.toml").exists());
        assert!(path.join("constraints").is_dir());
        assert!(path.join("constraints/pins.xdc").exists());

        let content = std::fs::read_to_string(path.join("platform.toml")).unwrap();
        assert!(content.contains("name = \"zcu104\""));
    }
}
