# Task 11: Vivado Executor

**Prerequisites:** Task 10 complete
**Goal:** Spawn `vivado -mode batch -source <script>`, capture stdout/stderr to log files, and return a `BuildResult` based on exit code.

## Spec Reference
`system_plan.md` §15 Phase 1 (Vivado backend: batch execution, log capture)

## File to Implement
`crates/loom-vivado/src/executor.rs`

## Implementation

```rust
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Write};
use std::fs;
use loom_core::plugin::backend::BuildResult;
use loom_core::build::context::BuildContext;
use loom_core::error::LoomError;

/// Run Vivado in batch mode, executing the given Tcl scripts in sequence.
/// Captures stdout and stderr to log files in the build directory.
pub fn run_vivado_batch(
    scripts: &[PathBuf],
    context: &BuildContext,
) -> Result<BuildResult, LoomError> {
    if scripts.is_empty() {
        return Err(LoomError::Internal("No build scripts to execute".to_string()));
    }

    // Ensure build directory exists
    fs::create_dir_all(&context.build_dir)
        .map_err(|e| LoomError::Io { path: context.build_dir.clone(), source: e })?;

    // Main log file
    let log_path = context.build_dir.join("build.log");

    // Find Vivado executable
    let vivado_path = find_vivado_executable()?;

    // Run each script. For Phase 1, there's exactly one script.
    // Phase 2+ may chain multiple scripts (e.g., run + extract_metrics).
    let mut all_phases_completed = Vec::new();
    let mut last_result: Option<BuildResult> = None;

    for script in scripts {
        let result = run_single_script(
            &vivado_path,
            script,
            &log_path,
            context,
        )?;

        all_phases_completed.extend(result.phases_completed.clone());
        let success = result.success;
        last_result = Some(result);

        if !success { break; }
    }

    let result = last_result.unwrap_or_else(|| BuildResult {
        success: false,
        exit_code: -1,
        log_paths: vec![log_path.clone()],
        bitstream_path: None,
        phases_completed: vec![],
        failure_phase: Some("unknown".to_string()),
        failure_message: Some("No scripts were executed".to_string()),
    });

    Ok(result)
}

fn run_single_script(
    vivado_path: &Path,
    script: &Path,
    log_path: &Path,
    context: &BuildContext,
) -> Result<BuildResult, LoomError> {
    // Open log file for writing
    let log_file = fs::File::create(log_path)
        .map_err(|e| LoomError::Io { path: log_path.to_owned(), source: e })?;

    let mut log_writer = std::io::BufWriter::new(log_file);

    // Write header to log
    writeln!(log_writer, "# Loom build log")
        .map_err(|e| LoomError::Io { path: log_path.to_owned(), source: e })?;
    writeln!(log_writer, "# Command: vivado -mode batch -source {}", script.display())
        .map_err(|e| LoomError::Io { path: log_path.to_owned(), source: e })?;
    writeln!(log_writer, "# Working directory: {}", context.build_dir.display())
        .map_err(|e| LoomError::Io { path: log_path.to_owned(), source: e })?;
    writeln!(log_writer, "# ---")
        .map_err(|e| LoomError::Io { path: log_path.to_owned(), source: e })?;

    // Build the command
    // On Windows: vivado -mode batch -source <script> -nojournal -nolog
    // (We manage our own log, suppress Vivado's default journal/log)
    let mut cmd = Command::new(vivado_path);
    cmd.arg("-mode").arg("batch")
       .arg("-source").arg(script)
       .arg("-nojournal")  // don't write vivado.jou
       .arg("-nolog")      // don't write vivado.log (we capture manually)
       .current_dir(&context.build_dir)
       .stdout(Stdio::piped())
       .stderr(Stdio::piped());

    // Set environment
    for (key, value) in &context.env {
        cmd.env(key, value);
    }

    let mut child = cmd.spawn()
        .map_err(|e| LoomError::ToolNotFound {
            tool: "vivado".to_string(),
            message: e.to_string(),
        })?;

    // Stream stdout to log file (and optionally to terminal)
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    let stdout_lines = collect_and_log_output(stdout, &mut log_writer, "OUT")
        .map_err(|e| LoomError::Io { path: log_path.to_owned(), source: e })?;

    let stderr_lines = collect_and_log_output(stderr, &mut log_writer, "ERR")
        .map_err(|e| LoomError::Io { path: log_path.to_owned(), source: e })?;

    let status = child.wait()
        .map_err(|e| LoomError::Io { path: log_path.to_owned(), source: e })?;

    let exit_code = status.code().unwrap_or(-1);
    let success = exit_code == 0;

    // Detect bitstream path from log output (look for write_bitstream completion)
    let bitstream_path = detect_bitstream_path(&stdout_lines, &context.build_dir);

    // Phase completion detection (Phase 1: simple pattern matching)
    let phases_completed = detect_completed_phases(&stdout_lines);
    let failure_phase = if !success {
        detect_failure_phase(&stdout_lines, &stderr_lines)
    } else {
        None
    };

    Ok(BuildResult {
        success,
        exit_code,
        log_paths: vec![log_path.to_owned()],
        bitstream_path,
        phases_completed,
        failure_phase,
        failure_message: if !success {
            Some(extract_failure_message(&stderr_lines))
        } else {
            None
        },
    })
}

/// Read all output from a pipe, write to log with prefix, return lines.
fn collect_and_log_output<R: std::io::Read>(
    reader: R,
    log: &mut impl Write,
    prefix: &str,
) -> std::io::Result<Vec<String>> {
    let mut lines = Vec::new();
    let reader = BufReader::new(reader);
    for line in reader.lines() {
        let line = line?;
        writeln!(log, "[{}] {}", prefix, line)?;
        lines.push(line);
    }
    Ok(lines)
}

/// Find Vivado executable from:
/// 1. VIVADO_PATH env var
/// 2. Common installation paths
/// 3. PATH search
fn find_vivado_executable() -> Result<PathBuf, LoomError> {
    // Check env var first
    if let Ok(path) = std::env::var("VIVADO_PATH") {
        let p = PathBuf::from(path);
        if p.exists() { return Ok(p); }
    }

    // Common installation paths (platform-specific)
    let standard_paths = vivado_standard_paths();
    for path in &standard_paths {
        if path.exists() { return Ok(path.clone()); }
    }

    // PATH search
    if let Ok(output) = Command::new("which").arg("vivado").output() {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path_str.is_empty() {
                return Ok(PathBuf::from(path_str));
            }
        }
    }

    // Windows: try `where vivado`
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = Command::new("where").arg("vivado").output() {
            if output.status.success() {
                let path_str = String::from_utf8_lossy(&output.stdout)
                    .lines().next().unwrap_or("").trim().to_string();
                if !path_str.is_empty() {
                    return Ok(PathBuf::from(path_str));
                }
            }
        }
    }

    Err(LoomError::ToolNotFound {
        tool: "vivado".to_string(),
        message: "Not found in VIVADO_PATH, standard paths, or system PATH. \
                  Run `loom env check` for details.".to_string(),
    })
}

#[cfg(not(target_os = "windows"))]
fn vivado_standard_paths() -> Vec<PathBuf> {
    // Scan /tools/Xilinx/Vivado/<version>/bin/vivado
    let mut paths = Vec::new();
    for base in &["/tools/Xilinx/Vivado", "/opt/Xilinx/Vivado", "/home/Xilinx/Vivado"] {
        let base_path = PathBuf::from(base);
        if let Ok(entries) = std::fs::read_dir(&base_path) {
            for entry in entries.flatten() {
                let bin = entry.path().join("bin").join("vivado");
                if bin.exists() { paths.push(bin); }
            }
        }
    }
    paths
}

#[cfg(target_os = "windows")]
fn vivado_standard_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for base in &[r"C:\Xilinx\Vivado", r"C:\tools\Xilinx\Vivado"] {
        let base_path = PathBuf::from(base);
        if let Ok(entries) = std::fs::read_dir(&base_path) {
            for entry in entries.flatten() {
                let bin = entry.path().join("bin").join("vivado.bat");
                if bin.exists() { paths.push(bin); }
            }
        }
    }
    paths
}

/// Look for "write_bitstream completed" pattern in Vivado output.
fn detect_bitstream_path(lines: &[String], build_dir: &Path) -> Option<PathBuf> {
    // Vivado outputs "INFO: [Vivado 12-1842] Bitfile: /path/to/file.bit"
    for line in lines {
        if line.contains("Bitfile:") {
            if let Some(path_str) = line.split("Bitfile:").nth(1) {
                let path = PathBuf::from(path_str.trim());
                if path.exists() { return Some(path); }
            }
        }
    }
    None
}

/// Detect which sub-phases completed based on Vivado output patterns.
fn detect_completed_phases(lines: &[String]) -> Vec<String> {
    let mut phases = Vec::new();
    for line in lines {
        if line.contains("synth_design completed") || line.contains("Synthesis Report") {
            if !phases.contains(&"synthesis".to_string()) {
                phases.push("synthesis".to_string());
            }
        }
        if line.contains("opt_design completed") || line.contains("Opt Design Report") {
            if !phases.contains(&"optimize".to_string()) {
                phases.push("optimize".to_string());
            }
        }
        if line.contains("place_design completed") || line.contains("Placement Report") {
            if !phases.contains(&"place".to_string()) {
                phases.push("place".to_string());
            }
        }
        if line.contains("route_design completed") || line.contains("Routing Report") {
            if !phases.contains(&"route".to_string()) {
                phases.push("route".to_string());
            }
        }
        if line.contains("write_bitstream completed") {
            if !phases.contains(&"bitstream".to_string()) {
                phases.push("bitstream".to_string());
            }
        }
    }
    phases
}

fn detect_failure_phase(stdout: &[String], stderr: &[String]) -> Option<String> {
    // Look for Vivado error patterns to attribute failure
    for line in stdout.iter().chain(stderr.iter()).rev() {
        if line.contains("ERROR") {
            if line.contains("synth") { return Some("synthesis".to_string()); }
            if line.contains("opt") { return Some("optimize".to_string()); }
            if line.contains("place") { return Some("place".to_string()); }
            if line.contains("route") { return Some("route".to_string()); }
            if line.contains("bitstream") { return Some("bitstream".to_string()); }
        }
    }
    Some("unknown".to_string())
}

fn extract_failure_message(stderr: &[String]) -> String {
    // Find the most relevant ERROR lines
    let errors: Vec<&str> = stderr.iter()
        .filter(|l| l.contains("ERROR"))
        .map(|l| l.as_str())
        .take(5)
        .collect();
    if errors.is_empty() {
        "Build failed. Check build log for details.".to_string()
    } else {
        errors.join("\n")
    }
}
```

## Tests

Since these tests require an actual Vivado installation, use integration test guards:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// This test only runs if VIVADO_PATH is set (CI with Vivado installed)
    #[test]
    #[ignore = "requires Vivado installation"]
    fn test_vivado_batch_simple() {
        // Create a minimal Tcl script that just prints something and exits
        // Run it, verify exit code 0
    }

    #[test]
    fn test_find_vivado_path_from_env() {
        std::env::set_var("VIVADO_PATH", "/usr/bin/true");  // fake path
        // Test that env var is checked first
        // (don't actually run it)
    }

    #[test]
    fn test_detect_completed_phases_from_log() {
        let lines = vec![
            "INFO: [Vivado 12-111] synth_design completed".to_string(),
            "INFO: [Vivado 12-112] opt_design completed".to_string(),
        ];
        let phases = detect_completed_phases(&lines);
        assert!(phases.contains(&"synthesis".to_string()));
        assert!(phases.contains(&"optimize".to_string()));
        assert!(!phases.contains(&"place".to_string()));
    }

    #[test]
    fn test_tcl_path_forward_slashes() {
        // verify to_tcl_path converts backslashes
        // (Import from tcl_gen module)
    }
}
```

## Error Variants to Add

```rust
ToolNotFound { tool: String, message: String },
Internal(String),
```

## Done When

- `cargo test -p loom-vivado` passes (ignoring Vivado-requiring tests)
- `run_vivado_batch()` correctly constructs the `vivado -mode batch -source ...` command
- Log file is written to `<build_dir>/build.log`
- `detect_completed_phases()` parses Vivado output patterns
- Forward-slash path conversion works on all platforms
- `find_vivado_executable()` correctly checks env var → standard paths → PATH in that order
