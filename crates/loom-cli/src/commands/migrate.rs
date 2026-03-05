use std::path::PathBuf;

use clap::{Args, Subcommand};

use loom_core::error::LoomError;

use crate::ui::{self, Icon};
use crate::GlobalContext;

#[derive(Subcommand)]
pub enum MigrateCommands {
    /// Convert Vivado .xci files to TOML generator declarations
    XciToToml(XciToTomlArgs),
    /// Convert Quartus .qsf files to project.toml
    QsfToToml(QsfToTomlArgs),
    /// Audit a Tcl build script for loom compatibility
    TclAudit(TclAuditArgs),
    /// Wrap a Tcl script as a loom generator
    TclWrap(TclWrapArgs),
}

#[derive(Args)]
pub struct XciToTomlArgs {
    /// Path to .xci file
    pub file: Option<PathBuf>,

    /// Batch convert all .xci files in a directory
    #[arg(long, value_name = "DIR")]
    pub batch: Option<PathBuf>,
}

#[derive(Args)]
pub struct QsfToTomlArgs {
    /// Path to .qsf file
    pub file: PathBuf,
}

#[derive(Args)]
pub struct TclAuditArgs {
    /// Path to Tcl script to audit
    pub file: PathBuf,
}

#[derive(Args)]
pub struct TclWrapArgs {
    /// Path to Tcl script to wrap
    pub file: PathBuf,

    /// Name for the generator
    #[arg(long)]
    pub name: Option<String>,
}

pub fn run(cmd: MigrateCommands, ctx: &GlobalContext) -> Result<(), LoomError> {
    match cmd {
        MigrateCommands::XciToToml(args) => run_xci_to_toml(args, ctx),
        MigrateCommands::QsfToToml(args) => run_qsf_to_toml(args, ctx),
        MigrateCommands::TclAudit(args) => run_tcl_audit(args, ctx),
        MigrateCommands::TclWrap(args) => run_tcl_wrap(args, ctx),
    }
}

fn run_xci_to_toml(args: XciToTomlArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let files = collect_xci_files(&args)?;

    if files.is_empty() {
        return Err(LoomError::Internal(
            "No .xci files found. Provide a file path or --batch <dir>.".to_string(),
        ));
    }

    for file in &files {
        if !ctx.quiet {
            ui::status(Icon::Dot, "Processing", &file.display().to_string());
        }
        match parse_xci(file) {
            Ok(toml_output) => {
                println!("{}", toml_output);
            }
            Err(e) => {
                eprintln!("  Error processing {}: {}", file.display(), e);
            }
        }
    }

    Ok(())
}

fn collect_xci_files(args: &XciToTomlArgs) -> Result<Vec<PathBuf>, LoomError> {
    let mut files = Vec::new();

    if let Some(ref path) = args.file {
        if !path.exists() {
            return Err(LoomError::Io {
                path: path.clone(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "File not found"),
            });
        }
        files.push(path.clone());
    }

    if let Some(ref dir) = args.batch {
        if !dir.is_dir() {
            return Err(LoomError::Io {
                path: dir.clone(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "Directory not found"),
            });
        }
        for entry in walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("xci") {
                files.push(path.to_owned());
            }
        }
    }

    Ok(files)
}

/// Parse a Vivado .xci file and produce TOML generator declaration.
fn parse_xci(path: &std::path::Path) -> Result<String, LoomError> {
    let content = std::fs::read_to_string(path).map_err(|e| LoomError::Io {
        path: path.to_owned(),
        source: e,
    })?;

    let mut reader = quick_xml::Reader::from_str(&content);
    reader.config_mut().trim_text(true);

    let mut module_name = String::new();
    let mut vlnv = String::new();
    let mut properties: Vec<(String, String)> = Vec::new();

    // Track current element context
    let mut in_param = false;
    let mut current_param_name = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e))
            | Ok(quick_xml::events::Event::Empty(ref e)) => {
                let local_name = e.local_name();
                let name_str = std::str::from_utf8(local_name.as_ref()).unwrap_or("");

                match name_str {
                    "spirit:componentInstance" | "componentInstance" => {
                        // Look for instance name in attributes or children
                    }
                    "spirit:instanceName" | "instanceName" => {
                        // Will read text in next event
                    }
                    "spirit:configurableElementValue" | "configurableElementValue" => {
                        // Extract parameter name from spirit:referenceId attribute
                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                            if key.contains("referenceId") || key == "spirit:referenceId" {
                                current_param_name =
                                    String::from_utf8_lossy(&attr.value).to_string();
                                // Strip common prefixes
                                if let Some(stripped) =
                                    current_param_name.strip_prefix("MODELPARAM_VALUE.")
                                {
                                    current_param_name = stripped.to_string();
                                } else if let Some(stripped) =
                                    current_param_name.strip_prefix("PARAM_VALUE.")
                                {
                                    current_param_name = stripped.to_string();
                                }
                                in_param = true;
                            }
                        }
                    }
                    "spirit:vlnv" | "vlnv" => {
                        // Extract VLNV from attributes
                        let mut vendor = String::new();
                        let mut library = String::new();
                        let mut ip_name = String::new();
                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                            let val = String::from_utf8_lossy(&attr.value).to_string();
                            match key {
                                "spirit:vendor" | "vendor" => vendor = val,
                                "spirit:library" | "library" => library = val,
                                "spirit:name" | "name" => ip_name = val,
                                _ => {}
                            }
                        }
                        if !vendor.is_empty() && !ip_name.is_empty() {
                            vlnv = format!("{}:{}:{}", vendor, library, ip_name);
                        }
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                if in_param && !current_param_name.is_empty() {
                    if !text.trim().is_empty() {
                        properties.push((current_param_name.clone(), text.trim().to_string()));
                    }
                    in_param = false;
                    current_param_name.clear();
                }
                // Check if we're reading instanceName
                if module_name.is_empty() && !text.trim().is_empty() {
                    // Heuristic: first text we see might be module name
                    // but we need more context — skip for now
                }
            }
            Ok(quick_xml::events::Event::End(_)) => {
                in_param = false;
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => {
                return Err(LoomError::Internal(format!(
                    "XML parse error in {}: {}",
                    path.display(),
                    e
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    // Derive module name from filename if not found in XML
    if module_name.is_empty() {
        module_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
    }

    // Generate TOML output
    let mut output = String::new();
    output.push_str("[[generators]]\n");
    output.push_str(&format!("name = \"{}\"\n", module_name));
    output.push_str("plugin = \"vivado_ip\"\n");
    output.push_str("[generators.config]\n");
    if !vlnv.is_empty() {
        output.push_str(&format!("vlnv = \"{}\"\n", vlnv));
    }
    if !properties.is_empty() {
        output.push_str("properties = {\n");
        for (key, value) in &properties {
            output.push_str(&format!("    {} = \"{}\",\n", key, value));
        }
        output.push_str("}\n");
    }

    Ok(output)
}

fn run_qsf_to_toml(args: QsfToTomlArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    if !args.file.exists() {
        return Err(LoomError::Io {
            path: args.file.clone(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "QSF file not found"),
        });
    }

    if !ctx.quiet {
        ui::status(Icon::Dot, "Parsing", &args.file.display().to_string());
    }

    let content = std::fs::read_to_string(&args.file).map_err(|e| LoomError::Io {
        path: args.file.clone(),
        source: e,
    })?;

    let mut device = String::new();
    let mut top_module = String::new();
    let mut sv_files = Vec::new();
    let mut v_files = Vec::new();
    let mut vhdl_files = Vec::new();
    let mut sdc_files = Vec::new();
    let mut ip_files = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 || parts[0] != "set_global_assignment" || parts[1] != "-name" {
            continue;
        }

        match parts[2] {
            "DEVICE" => device = parts[3].to_string(),
            "TOP_LEVEL_ENTITY" => top_module = parts[3].to_string(),
            "SYSTEMVERILOG_FILE" => sv_files.push(parts[3].to_string()),
            "VERILOG_FILE" => v_files.push(parts[3].to_string()),
            "VHDL_FILE" => vhdl_files.push(parts[3].to_string()),
            "SDC_FILE" => sdc_files.push(parts[3].to_string()),
            "IP_FILE" | "QIP_FILE" => ip_files.push(parts[3].to_string()),
            _ => {}
        }
    }

    // Generate project.toml
    let project_name = args
        .file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("my_project");

    let mut output = String::new();
    output.push_str("[project]\n");
    output.push_str(&format!("name = \"{}\"\n", project_name));
    if !top_module.is_empty() {
        output.push_str(&format!("top_module = \"{}\"\n", top_module));
    }
    output.push_str("\n[target]\n");
    if !device.is_empty() {
        output.push_str(&format!("part = \"{}\"\n", device));
    }
    output.push_str("backend = \"quartus\"\n");

    // Source files
    let all_sources: Vec<&str> = sv_files
        .iter()
        .chain(v_files.iter())
        .chain(vhdl_files.iter())
        .map(|s| s.as_str())
        .collect();

    if !all_sources.is_empty() {
        output.push_str("\n[filesets.synth]\n");
        output.push_str("files = [\n");
        for f in &all_sources {
            output.push_str(&format!("    \"{}\",\n", f));
        }
        output.push_str("]\n");
    }

    if !sdc_files.is_empty() {
        output.push_str("\n[filesets.constraints]\n");
        output.push_str("files = [\n");
        for f in &sdc_files {
            output.push_str(&format!("    \"{}\",\n", f));
        }
        output.push_str("]\n");
    }

    if !ip_files.is_empty() {
        output.push_str("\n# IP files found (convert to generators):\n");
        for f in &ip_files {
            output.push_str(&format!("# {}\n", f));
        }
    }

    println!("{}", output);
    Ok(())
}

fn run_tcl_audit(args: TclAuditArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    if !args.file.exists() {
        return Err(LoomError::Io {
            path: args.file.clone(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "Tcl file not found"),
        });
    }

    if !ctx.quiet {
        ui::status(Icon::Dot, "Auditing", &args.file.display().to_string());
    }

    let content = std::fs::read_to_string(&args.file).map_err(|e| LoomError::Io {
        path: args.file.clone(),
        source: e,
    })?;

    let mut warnings = Vec::new();
    let mut info = Vec::new();
    let mut has_file_ops = false;
    let mut has_source = false;

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let line_num = i + 1;

        // Check for file I/O operations
        if trimmed.starts_with("open ") || trimmed.contains("[open ") {
            has_file_ops = true;
            warnings.push(format!("  L{}: File I/O detected: {}", line_num, trimmed));
        }

        // Check for source commands
        if trimmed.starts_with("source ") {
            has_source = true;
            info.push(format!("  L{}: Source command: {}", line_num, trimmed));
        }

        // Check for exec commands
        if trimmed.starts_with("exec ") || trimmed.contains("[exec ") {
            warnings.push(format!(
                "  L{}: External command execution: {}",
                line_num, trimmed
            ));
        }

        // Check for Vivado-specific commands
        for cmd in &[
            "read_verilog",
            "read_vhdl",
            "synth_design",
            "opt_design",
            "place_design",
            "route_design",
            "write_bitstream",
            "read_xdc",
            "create_project",
            "add_files",
        ] {
            if trimmed.starts_with(cmd) {
                info.push(format!("  L{}: Vivado command: {}", line_num, trimmed));
            }
        }
    }

    println!("Tcl Audit Report: {}", args.file.display());
    println!();

    if !info.is_empty() {
        println!("  Detected commands:");
        for line in &info {
            println!("{}", line);
        }
        println!();
    }

    if !warnings.is_empty() {
        println!("  Warnings:");
        for line in &warnings {
            println!("{}", line);
        }
        println!();
    }

    println!("  Summary:");
    println!(
        "    File I/O:     {}",
        if has_file_ops {
            "YES (review needed)"
        } else {
            "none"
        }
    );
    println!(
        "    Source calls:  {}",
        if has_source { "YES" } else { "none" }
    );
    println!(
        "    Migratable:    {}",
        if warnings.is_empty() {
            "likely"
        } else {
            "needs review"
        }
    );

    Ok(())
}

fn run_tcl_wrap(args: TclWrapArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    if !args.file.exists() {
        return Err(LoomError::Io {
            path: args.file.clone(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "Tcl file not found"),
        });
    }

    if !ctx.quiet {
        ui::status(
            Icon::Dot,
            "Wrapping",
            &format!("{} as generator", args.file.display()),
        );
    }

    let gen_name = args.name.unwrap_or_else(|| {
        args.file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("custom_tcl")
            .to_string()
    });

    let file_path = args.file.display().to_string().replace('\\', "/");

    let mut output = String::new();
    output.push_str("[[generators]]\n");
    output.push_str(&format!("name = \"{}\"\n", gen_name));
    output.push_str("plugin = \"tcl_script\"\n");
    output.push_str("[generators.config]\n");
    output.push_str(&format!("script = \"{}\"\n", file_path));
    output.push_str("# Add any parameters your Tcl script expects:\n");
    output.push_str("# [generators.config.params]\n");
    output.push_str("# param1 = \"value1\"\n");

    println!("{}", output);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_xci() {
        let tmp = tempfile::TempDir::new().unwrap();
        let xci_path = tmp.path().join("clk_wiz_0.xci");
        std::fs::write(
            &xci_path,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<spirit:design xmlns:spirit="http://www.spiritconsortium.org/XMLSchema/SPIRIT/1685-2009">
  <spirit:vendor>xilinx.com</spirit:vendor>
  <spirit:library>xci</spirit:library>
  <spirit:name>unknown</spirit:name>
  <spirit:version>1.0</spirit:version>
  <spirit:componentInstances>
    <spirit:componentInstance>
      <spirit:instanceName>clk_wiz_0</spirit:instanceName>
      <spirit:componentRef spirit:vendor="xilinx.com" spirit:library="ip" spirit:name="clk_wiz" spirit:version="6.0"/>
      <spirit:configurableElementValues>
        <spirit:configurableElementValue spirit:referenceId="PARAM_VALUE.PRIM_IN_FREQ">200.000</spirit:configurableElementValue>
        <spirit:configurableElementValue spirit:referenceId="PARAM_VALUE.CLKOUT1_REQUESTED_OUT_FREQ">100.000</spirit:configurableElementValue>
      </spirit:configurableElementValues>
    </spirit:componentInstance>
  </spirit:componentInstances>
</spirit:design>"#,
        )
        .unwrap();

        let result = parse_xci(&xci_path).unwrap();
        assert!(result.contains("name = \"clk_wiz_0\""));
        assert!(result.contains("plugin = \"vivado_ip\""));
        assert!(result.contains("PRIM_IN_FREQ"));
        assert!(result.contains("100.000"));
    }
}
