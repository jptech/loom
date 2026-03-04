use clap::{Args, Subcommand};

use loom_core::error::LoomError;

use crate::backend_registry::get_backend;
use crate::GlobalContext;

#[derive(Subcommand)]
pub enum EnvCommands {
    /// Check tool environment
    Check,
    /// Open a subshell with the tool environment configured
    Shell(EnvShellArgs),
    /// Generate a Dockerfile for CI builds
    Dockerfile(DockerfileArgs),
}

#[derive(Args)]
pub struct EnvShellArgs {
    /// Backend to configure environment for (default: auto-detect from project.toml)
    #[arg(short, long, default_value = "vivado")]
    pub backend: String,
}

#[derive(Args)]
pub struct DockerfileArgs {
    /// Backend to include in Dockerfile
    #[arg(short, long, default_value = "vivado")]
    pub backend: String,

    /// Tool version to install
    #[arg(long)]
    pub tool_version: Option<String>,

    /// Base image
    #[arg(long, default_value = "ubuntu:22.04")]
    pub base_image: String,

    /// Write to file instead of stdout
    #[arg(short, long)]
    pub output: Option<String>,
}

pub fn run(cmd: EnvCommands, ctx: &GlobalContext) -> Result<(), LoomError> {
    match cmd {
        EnvCommands::Check => run_check(ctx),
        EnvCommands::Shell(args) => run_shell(args, ctx),
        EnvCommands::Dockerfile(args) => run_dockerfile(args, ctx),
    }
}

fn run_check(ctx: &GlobalContext) -> Result<(), LoomError> {
    let backend = get_backend("vivado")?;
    let status = backend.check_environment(None)?;

    if ctx.json {
        let json = serde_json::json!({
            "backend": status.tool_name,
            "path": status.tool_path.display().to_string(),
            "version": status.version,
            "version_ok": status.version_matches,
            "license_ok": status.license_ok,
            "license_detail": status.license_detail,
            "warnings": status.warnings,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&json).unwrap_or_default()
        );
    } else {
        println!("Backend: {}", status.tool_name);
        println!("  Path:    {}", status.tool_path.display());
        println!("  Version: {}", status.version);
        if let Some(req) = &status.required_version {
            let ok_str = if status.version_matches {
                "OK"
            } else {
                "MISMATCH"
            };
            println!("  Required: {} [{}]", req, ok_str);
        }
        let license_str = if status.license_ok { "OK" } else { "FAILED" };
        println!("  License: {}", license_str);
        if let Some(detail) = &status.license_detail {
            println!("    ({})", detail);
        }
        for warning in &status.warnings {
            println!("  warning: {}", warning);
        }
    }

    if status.is_ok() {
        Ok(())
    } else {
        Err(LoomError::ToolVersionMismatch {
            required: status.required_version.unwrap_or_default(),
            found: status.version,
        })
    }
}

fn run_shell(args: EnvShellArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let backend = get_backend(&args.backend)?;
    let status = backend.check_environment(None)?;

    if !ctx.quiet {
        eprintln!(
            "Launching shell with {} {} environment...",
            status.tool_name, status.version
        );
        eprintln!("  Tool path: {}", status.tool_path.display());
        eprintln!("  Type 'exit' to return to normal shell.");
    }

    // Get the tool's bin directory to prepend to PATH
    let tool_bin_dir = status
        .tool_path
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = if tool_bin_dir.is_empty() {
        current_path
    } else {
        format!(
            "{}{}{}",
            tool_bin_dir,
            std::path::MAIN_SEPARATOR,
            current_path
        )
    };

    let prompt_arg = format!("prompt [loom:{}] $P$G", status.tool_name);
    let (shell, args_vec): (&str, Vec<&str>) = if cfg!(target_os = "windows") {
        ("cmd", vec!["/k", &prompt_arg])
    } else {
        ("bash", vec!["--norc", "--noprofile"])
    };

    let mut cmd = std::process::Command::new(shell);
    cmd.args(&args_vec).env("PATH", &new_path);

    // Set PS1 for bash
    if !cfg!(target_os = "windows") {
        cmd.env("PS1", format!("[loom:{}] \\w $ ", status.tool_name));
    }

    // Set tool-specific env vars
    if let Some(parent) = status.tool_path.parent() {
        if let Some(tool_root) = parent.parent() {
            match args.backend.as_str() {
                "vivado" => {
                    cmd.env("XILINX_VIVADO", tool_root.to_string_lossy().to_string());
                }
                "quartus" => {
                    cmd.env("QUARTUS_ROOTDIR", tool_root.to_string_lossy().to_string());
                }
                _ => {}
            }
        }
    }

    let child_status = cmd
        .status()
        .map_err(|e| LoomError::Internal(format!("Failed to launch shell: {}", e)))?;

    if !child_status.success() {
        // Non-zero exit from shell is expected (user just exits)
    }

    Ok(())
}

fn run_dockerfile(args: DockerfileArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let dockerfile = generate_dockerfile(&args);

    if let Some(output_path) = &args.output {
        std::fs::write(output_path, &dockerfile).map_err(|e| LoomError::Io {
            path: output_path.into(),
            source: e,
        })?;
        if !ctx.quiet {
            eprintln!("  Wrote Dockerfile to {}", output_path);
        }
    } else {
        println!("{}", dockerfile);
    }

    Ok(())
}

fn generate_dockerfile(args: &DockerfileArgs) -> String {
    let tool_version = args.tool_version.as_deref().unwrap_or("latest");

    let mut dockerfile = String::new();

    dockerfile.push_str(&format!(
        "# Loom FPGA Build Environment — {} {}\n",
        args.backend, tool_version
    ));
    dockerfile.push_str("# Auto-generated by `loom env dockerfile`\n\n");
    dockerfile.push_str(&format!("FROM {}\n\n", args.base_image));

    // Common setup
    dockerfile.push_str("ENV DEBIAN_FRONTEND=noninteractive\n");
    dockerfile.push_str("RUN apt-get update && apt-get install -y \\\n");
    dockerfile.push_str("    curl \\\n");
    dockerfile.push_str("    git \\\n");
    dockerfile.push_str("    make \\\n");
    dockerfile.push_str("    python3 \\\n");
    dockerfile.push_str("    python3-pip \\\n");
    dockerfile.push_str("    libncurses5 \\\n");
    dockerfile.push_str("    libxtst6 \\\n");
    dockerfile.push_str("    locales \\\n");
    dockerfile.push_str("    && rm -rf /var/lib/apt/lists/*\n\n");
    dockerfile.push_str("RUN locale-gen en_US.UTF-8\n");
    dockerfile.push_str("ENV LANG=en_US.UTF-8\n\n");

    // Backend-specific instructions
    match args.backend.as_str() {
        "vivado" => {
            dockerfile.push_str(&format!("# Vivado {} installation\n", tool_version));
            dockerfile.push_str("# Option 1: Mount from host volume\n");
            dockerfile.push_str(&format!(
                "# ENV XILINX_VIVADO=/tools/Xilinx/Vivado/{}\n",
                tool_version
            ));
            dockerfile.push_str("# ENV PATH=$XILINX_VIVADO/bin:$PATH\n\n");
            dockerfile.push_str("# Option 2: Install via Xilinx Unified Installer (batch mode)\n");
            dockerfile.push_str("# COPY Xilinx_Unified_*_Lin64.bin /tmp/installer.bin\n");
            dockerfile.push_str("# COPY install_config.txt /tmp/install_config.txt\n");
            dockerfile.push_str("# RUN chmod +x /tmp/installer.bin && \\\n");
            dockerfile.push_str(
                "#     /tmp/installer.bin --agree XilinxEULA,3rdPartyEULA --batch Install --config /tmp/install_config.txt && \\\n",
            );
            dockerfile.push_str("#     rm /tmp/installer.bin\n\n");
            dockerfile.push_str(&format!(
                "ENV XILINX_VIVADO=/tools/Xilinx/Vivado/{}\n",
                tool_version
            ));
            dockerfile.push_str("ENV PATH=$XILINX_VIVADO/bin:$PATH\n\n");
        }
        "quartus" => {
            dockerfile.push_str(&format!("# Quartus {} installation\n", tool_version));
            dockerfile.push_str("# Install Intel Quartus Prime (batch mode)\n");
            dockerfile.push_str(&format!(
                "ENV QUARTUS_ROOTDIR=/opt/intelFPGA_lite/{}/quartus\n",
                tool_version
            ));
            dockerfile.push_str("ENV PATH=$QUARTUS_ROOTDIR/bin:$PATH\n\n");
        }
        "yosys" => {
            dockerfile.push_str("# Open-source FPGA toolchain\n");
            dockerfile.push_str("RUN apt-get update && apt-get install -y \\\n");
            dockerfile.push_str("    yosys \\\n");
            dockerfile.push_str("    nextpnr-ice40 \\\n");
            dockerfile.push_str("    nextpnr-ecp5 \\\n");
            dockerfile.push_str("    fpga-icestorm \\\n");
            dockerfile.push_str("    && rm -rf /var/lib/apt/lists/*\n\n");
        }
        "radiant" => {
            dockerfile.push_str(&format!(
                "# Lattice Radiant {} installation\n",
                tool_version
            ));
            dockerfile.push_str(&format!(
                "ENV FOUNDRY=/usr/local/lscc/radiant/{}/ispfpga\n",
                tool_version
            ));
            dockerfile.push_str(&format!(
                "ENV PATH=/usr/local/lscc/radiant/{}/bin:$PATH\n\n",
                tool_version
            ));
        }
        _ => {
            dockerfile.push_str(&format!(
                "# TODO: Add {} installation steps\n\n",
                args.backend
            ));
        }
    }

    // Install Loom
    dockerfile.push_str("# Install Loom\n");
    dockerfile.push_str("RUN curl -fsSL https://github.com/loom-fpga/loom/releases/latest/download/loom-linux-x64.tar.gz | \\\n");
    dockerfile.push_str("    tar -xzf - -C /usr/local/bin/\n\n");

    // Working directory and entrypoint
    dockerfile.push_str("WORKDIR /workspace\n\n");
    dockerfile.push_str("# CI-ready entrypoint\n");
    dockerfile.push_str("ENTRYPOINT [\"loom\"]\n");
    dockerfile.push_str("CMD [\"build\"]\n");

    dockerfile
}
