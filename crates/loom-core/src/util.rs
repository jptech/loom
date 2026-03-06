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
}
