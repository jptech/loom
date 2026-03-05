use std::path::Path;

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
    use std::path::PathBuf;

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
}
