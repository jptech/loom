use std::path::{Path, PathBuf};
use std::process::Command;

/// Add an argument to a tool command, handling Windows `cmd.exe` quoting.
///
/// When commands run through `cmd /C` (via [`tool_command`]), the batch-file
/// processor treats `=`, `;`, and `,` as argument delimiters.  This function
/// wraps arguments containing those characters in double quotes using
/// [`raw_arg`][std::os::windows::process::CommandExt::raw_arg] so they are
/// preserved as single tokens.
///
/// On non-Windows platforms this is equivalent to [`Command::arg`].
///
/// # When to use
///
/// Use `tool_arg` instead of `cmd.arg()` for any user/manifest-supplied value
/// that might contain `=` — primarily **defines** (`SIM=1`) and **plusargs**
/// (`seed=42`).  Fixed tool flags (`--sv`, `-s`, etc.) never contain `=` and
/// can use the regular `cmd.arg()`.
#[cfg(target_os = "windows")]
pub fn tool_arg(cmd: &mut Command, arg: &str) {
    use std::os::windows::process::CommandExt;
    if arg.contains('=') || arg.contains(';') || arg.contains(',') {
        cmd.raw_arg(format!("\"{}\"", arg));
    } else {
        cmd.arg(arg);
    }
}

/// See the Windows-specific doc above — on other platforms this is a plain `arg()`.
#[cfg(not(target_os = "windows"))]
pub fn tool_arg(cmd: &mut Command, arg: &str) {
    cmd.arg(arg);
}

/// Create a [`Command`] for an external tool, handling `.bat`/`.cmd` wrappers on Windows.
///
/// On Windows, [`Command::new("xvlog")`] uses `CreateProcess` which only resolves
/// `.exe` and `.com` extensions — not `.bat` or `.cmd`.  Many EDA tools (Vivado,
/// Quartus, etc.) ship as `.bat` wrappers, so bare names like `"xvlog"` fail even
/// when the tool is on `PATH`.
///
/// This function routes through `cmd.exe /C` on Windows so that the shell's own
/// `PATH` + `PATHEXT` resolution finds `.bat` and `.cmd` files.  On Unix it
/// delegates directly to [`Command::new`].
///
/// # Examples
///
/// ```ignore
/// use loom_core::util::tool_command;
/// let mut cmd = tool_command("xvlog");
/// cmd.arg("--sv").arg("top.sv");
/// let output = cmd.output()?;
/// ```
pub fn tool_command(tool: &str) -> Command {
    if cfg!(target_os = "windows") {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", tool]);
        cmd
    } else {
        Command::new(tool)
    }
}

/// Strip the Windows extended-length path prefix (`\\?\`) from a `PathBuf`.
///
/// On Windows, `std::fs::canonicalize()` produces paths like `\\?\C:\foo\bar`.
/// This prefix causes visual noise in user-facing output and confuses external tools.
/// Call this immediately after `canonicalize()` to keep all downstream paths clean.
///
/// On non-Windows platforms (or paths without the prefix), this is a no-op.
pub fn clean_path(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(stripped) => PathBuf::from(stripped),
        None => path,
    }
}

/// Format a path for user-facing display, stripping the Windows `\\?\` prefix if present.
///
/// Use this when displaying paths that may not have gone through `clean_path()` —
/// e.g., paths constructed by joining a clean root with relative segments that
/// were later re-canonicalized, or paths from external sources.
pub fn display_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(stripped) => stripped.to_string(),
        None => s.into_owned(),
    }
}

/// Convert a path to a tool-safe string: forward slashes, no Windows extended-length prefix.
///
/// On Windows, `std::fs::canonicalize()` produces `\\?\` prefixed paths which tools
/// like Vivado, Quartus, and yosys do not understand. This function strips that prefix
/// and normalizes to forward slashes.
pub fn to_tool_path(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    s.strip_prefix("//?/").unwrap_or(&s).to_string()
}

/// Result of scanning simulation output for self-checking testbench patterns.
#[derive(Debug, Default)]
pub struct SimOutputScan {
    /// Found "PASS:" or "PASS " in output
    pub has_pass: bool,
    /// Found "FAIL:" or "FAIL " in output
    pub has_fail: bool,
    /// Found "$finish" or "PASSED" (simulation completed normally)
    pub has_finish: bool,
    /// Lines containing FAIL patterns
    pub fail_lines: Vec<String>,
    /// Lines containing "Error", "ERROR", or "FATAL"
    pub error_lines: Vec<String>,
    /// Count of warning lines
    pub warning_count: usize,
    /// True if output was completely empty (no lines at all)
    pub empty_output: bool,
}

/// Write combined simulation stdout+stderr to a log file.
pub fn write_sim_log(log_path: &Path, stdout: &str, stderr: &str) {
    let content = if stderr.is_empty() {
        stdout.to_string()
    } else if stdout.is_empty() {
        stderr.to_string()
    } else {
        format!("{}\n{}", stdout, stderr)
    };
    let _ = std::fs::write(log_path, &content);
}

/// Scan simulation output for self-checking testbench patterns.
///
/// Pass/fail logic used by backends:
/// - `has_fail` → always fail (regardless of exit code)
/// - `has_pass` or `has_finish` → evidence the testbench ran to completion
/// - `empty_output` → no output at all, likely a silent failure
///
/// Backends should use [`SimOutputScan::is_pass`] for the final verdict.
pub fn scan_sim_output(output: &str) -> SimOutputScan {
    let trimmed = output.trim();
    let mut scan = SimOutputScan {
        empty_output: trimmed.is_empty(),
        ..Default::default()
    };
    for line in output.lines() {
        if line.contains("PASS:") || line.contains("PASS ") {
            scan.has_pass = true;
        }
        if line.contains("FAIL:") || line.contains("FAIL ") {
            scan.has_fail = true;
            scan.fail_lines.push(line.to_string());
        }
        if line.contains("Error") || line.contains("ERROR") || line.contains("FATAL") {
            scan.error_lines.push(line.to_string());
        }
        if line.contains("WARNING") || line.contains("Warning") {
            scan.warning_count += 1;
        }
        if line.contains("$finish") || line.contains("PASSED") {
            scan.has_finish = true;
        }
    }
    scan
}

impl SimOutputScan {
    /// Determine if the simulation passed, given the process exit code.
    ///
    /// A test passes only when ALL of these hold:
    /// 1. The process exited successfully (exit code 0)
    /// 2. No `FAIL:` patterns were found in the output
    /// 3. The output is not empty (guards against silent failures)
    /// 4. There is evidence the testbench completed (`PASS:`, `$finish`, or `PASSED`)
    ///
    /// If the output has content but no pass/fail/finish markers, the test
    /// is considered indeterminate and fails with an explanatory error.
    pub fn is_pass(&self, exit_success: bool) -> bool {
        exit_success && !self.has_fail && !self.empty_output && (self.has_pass || self.has_finish)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forward_slashes() {
        let path = PathBuf::from(r"C:\tools\Xilinx\test.xdc");
        let result = to_tool_path(&path);
        assert_eq!(result, "C:/tools/Xilinx/test.xdc");
    }

    #[test]
    fn test_strips_extended_length_prefix() {
        let path = PathBuf::from(r"\\?\Z:\loom\examples\blinky\rtl\counter.sv");
        let result = to_tool_path(&path);
        assert_eq!(result, "Z:/loom/examples/blinky/rtl/counter.sv");
    }

    #[test]
    fn test_unix_path_unchanged() {
        let path = PathBuf::from("/tools/Xilinx/Vivado/2023.2/bin/vivado");
        let result = to_tool_path(&path);
        assert_eq!(result, "/tools/Xilinx/Vivado/2023.2/bin/vivado");
    }

    #[test]
    fn test_clean_path_strips_prefix() {
        let path = PathBuf::from(r"\\?\Z:\loom\examples\blinky");
        let cleaned = clean_path(path);
        assert_eq!(cleaned, PathBuf::from(r"Z:\loom\examples\blinky"));
    }

    #[test]
    fn test_clean_path_no_prefix() {
        let path = PathBuf::from(r"Z:\loom\examples\blinky");
        let cleaned = clean_path(path.clone());
        assert_eq!(cleaned, path);
    }

    #[test]
    fn test_clean_path_unix() {
        let path = PathBuf::from("/home/user/loom");
        let cleaned = clean_path(path.clone());
        assert_eq!(cleaned, path);
    }

    #[test]
    fn test_display_path_strips_prefix() {
        let path = PathBuf::from(r"\\?\Z:\loom\examples\blinky");
        assert_eq!(display_path(&path), r"Z:\loom\examples\blinky");
    }

    #[test]
    fn test_display_path_no_prefix() {
        let path = PathBuf::from(r"Z:\loom\examples\blinky");
        assert_eq!(display_path(&path), r"Z:\loom\examples\blinky");
    }

    #[test]
    fn test_display_path_unix() {
        let path = PathBuf::from("/home/user/loom");
        assert_eq!(display_path(&path), "/home/user/loom");
    }

    #[test]
    fn test_scan_pass_only() {
        let scan = scan_sim_output("PASS: all checks completed\nDone.");
        assert!(scan.has_pass);
        assert!(!scan.has_fail);
        assert!(scan.fail_lines.is_empty());
    }

    #[test]
    fn test_scan_fail_only() {
        let scan = scan_sim_output("FAIL: mismatch at cycle 42\nFAIL: timeout");
        assert!(!scan.has_pass);
        assert!(scan.has_fail);
        assert_eq!(scan.fail_lines.len(), 2);
    }

    #[test]
    fn test_scan_both_pass_and_fail() {
        let scan = scan_sim_output("PASS: test A\nFAIL: test B");
        assert!(scan.has_pass);
        assert!(scan.has_fail);
        assert_eq!(scan.fail_lines.len(), 1);
    }

    #[test]
    fn test_scan_neither() {
        let scan = scan_sim_output("simulation complete\nall done");
        assert!(!scan.has_pass);
        assert!(!scan.has_fail);
        assert!(scan.fail_lines.is_empty());
        assert!(scan.error_lines.is_empty());
    }

    #[test]
    fn test_scan_fatal_and_error_lines() {
        let scan = scan_sim_output("FATAL: assertion failed\nERROR: bad state\nError at line 5");
        assert_eq!(scan.error_lines.len(), 3);
    }

    #[test]
    fn test_scan_case_sensitive() {
        let scan = scan_sim_output("pass: lowercase\nfail: lowercase");
        assert!(!scan.has_pass);
        assert!(!scan.has_fail);
    }

    #[test]
    fn test_scan_warnings() {
        let scan = scan_sim_output("WARNING: clk glitch\nWarning: width mismatch\nall ok");
        assert_eq!(scan.warning_count, 2);
    }

    #[test]
    fn test_scan_finish_detected() {
        let scan = scan_sim_output("some output\n$finish called at 100ns\n");
        assert!(scan.has_finish);
        assert!(scan.is_pass(true));
    }

    #[test]
    fn test_scan_empty_output_fails() {
        let scan = scan_sim_output("");
        assert!(scan.empty_output);
        assert!(!scan.is_pass(true)); // exit 0 + empty = fail
    }

    #[test]
    fn test_scan_no_markers_fails() {
        // Output exists but no PASS/FAIL/$finish — indeterminate
        let scan = scan_sim_output("some random simulator output\nno markers here");
        assert!(!scan.empty_output);
        assert!(!scan.has_pass);
        assert!(!scan.has_finish);
        assert!(!scan.is_pass(true)); // no evidence of completion
    }

    #[test]
    fn test_scan_pass_with_exit_0() {
        let scan = scan_sim_output("PASS: all tests passed");
        assert!(scan.is_pass(true));
        assert!(!scan.is_pass(false)); // exit non-zero still fails
    }

    #[test]
    fn test_scan_fail_overrides_pass() {
        let scan = scan_sim_output("PASS: test A\nFAIL: test B\n$finish");
        assert!(!scan.is_pass(true)); // FAIL takes priority
    }

    #[test]
    fn test_write_sim_log() {
        let dir = std::env::temp_dir().join("loom_test_sim_log");
        let _ = std::fs::create_dir_all(&dir);
        let log = dir.join("test.log");

        write_sim_log(&log, "stdout", "stderr");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "stdout\nstderr");

        write_sim_log(&log, "stdout only", "");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "stdout only");

        write_sim_log(&log, "", "stderr only");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "stderr only");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
