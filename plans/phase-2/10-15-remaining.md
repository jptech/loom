# Phase 2 / Tasks 10-15: CLI Polish, PyO3, Vivado IP, LSP, Upgrade, Migration

---

## Task 10: CLI Polish

**Dependencies:** colored, indicatif

**Color output:**
- Errors: red
- Warnings: yellow
- Success: green
- Phase names: bold
- Auto-detect TTY; `--no-color` disables

**Progress display (non-TTY: line-by-line; TTY: spinner):**
```rust
use indicatif::{ProgressBar, ProgressStyle};

// Build phase progress
let pb = ProgressBar::new_spinner();
pb.set_style(ProgressStyle::default_spinner()
    .template("{spinner:.cyan} {msg}")
    .unwrap());
pb.set_message("Synthesizing...");
// ... Vivado runs ...
pb.finish_with_message("Synthesis complete (423s)");
```

**`-j N` flag:** Pass through to generator DAG as max parallel count. Implement a simple thread pool or use `rayon` for parallel generator execution.

**`--json` mode:** All commands produce structured JSON output instead of human-readable text. Build result includes full `BuildReport`. Error output is `{"error": "...", "exit_code": N}`.

---

## Task 11: PyO3 Integration

**Spec:** §10.2 (Plugin Loading), §7.7.1 (Python execution model)

**Phase 2 scope:** Load Python plugins from:
1. Installed pip packages (entry points: `loom.plugins`)
2. Workspace-local `plugins/` directory
3. Manifest-declared paths

**Key design decision:** Generator execution uses subprocess isolation, not in-process Python:

```rust
// Rust core spawns a Python subprocess per generator execution
fn execute_python_generator(
    plugin_path: &Path,
    config_json: &str,  // JSON-encoded GeneratorConfig
    context_json: &str, // JSON-encoded BuildContext
) -> Result<GeneratorResult, LoomError> {
    let output = Command::new("python")
        .arg(plugin_path)
        .arg("--config").arg(config_json)
        .arg("--context").arg(context_json)
        .output()?;
    // Parse JSON response from stdout
    serde_json::from_slice(&output.stdout)
        .map_err(|e| LoomError::Internal(e.to_string()))
}
```

**Python plugin SDK** (`loom/plugin.py`):
```python
from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import List, Dict, Optional
import json, sys

class GeneratorPlugin(ABC):
    @property
    @abstractmethod
    def plugin_name(self) -> str: ...

    @abstractmethod
    def validate_config(self, config: dict) -> list[dict]: ...

    @abstractmethod
    def compute_cache_key(self, config: dict, input_hashes: dict) -> str: ...

    @abstractmethod
    def execute(self, config: dict, context: dict) -> dict: ...

    @abstractmethod
    def clean(self, config: dict, context: dict) -> None: ...

def main(plugin: GeneratorPlugin):
    """Entry point for subprocess-executed plugins."""
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--action", required=True)
    parser.add_argument("--config", required=True)
    parser.add_argument("--context", required=True)
    args = parser.parse_args()

    config = json.loads(args.config)
    context = json.loads(args.context)

    if args.action == "validate":
        result = plugin.validate_config(config)
    elif args.action == "cache_key":
        input_hashes = json.loads(args.input_hashes)
        result = {"cache_key": plugin.compute_cache_key(config, input_hashes)}
    elif args.action == "execute":
        result = plugin.execute(config, context)
    elif args.action == "clean":
        plugin.clean(config, context)
        result = {}

    print(json.dumps(result))
```

**Create Python package:** `python/loom_plugin/` containing the SDK. Install as `pip install loom-plugin-sdk`.

---

## Task 12: Vivado IP Generator

**Spec:** §6.5.0, §6.5.1

**Generator plugin (`vivado_ip`):**
```python
class VivadoIpGenerator(GeneratorPlugin):
    """Generates Vivado IP from declarative TOML config."""

    def execute(self, config: dict, context: dict) -> dict:
        vlnv = config["vlnv"]  # e.g., "xilinx.com:ip:clk_wiz"
        properties = config.get("properties", {})
        output_dir = context["build_dir"] + "/ip/" + config["name"]

        # Substitute platform parameter references: ${platform.clocks.sys_clk.frequency_mhz}
        resolved_props = self._resolve_params(properties, context)

        # Generate Tcl to create and configure the IP
        tcl_script = self._generate_ip_tcl(vlnv, resolved_props, output_dir)

        # Run Vivado in batch mode with this Tcl
        result = subprocess.run(
            ["vivado", "-mode", "batch", "-source", tcl_path],
            capture_output=True, text=True
        )

        return {
            "success": result.returncode == 0,
            "produced_files": [output_dir],  # IP output directory
            "log": result.stdout.split("\n"),
        }
```

**Tcl for IP generation:**
```tcl
create_project -in_memory -part {xczu7ev-ffvc1156-2-e}
create_ip -vlnv {xilinx.com:ip:clk_wiz} -module_name sys_clk_0
set_property -dict [list \
    CONFIG.PRIM_IN_FREQ {125.000} \
    CONFIG.CLKOUT1_REQUESTED_OUT_FREQ {100.000} \
] [get_ips sys_clk_0]
generate_target all [get_ips sys_clk_0]
```

**Floating VLNV resolution:** When VLNV has no version (e.g., `xilinx.com:ip:clk_wiz`), Tcl `get_ipdefs -filter "VLNV =~ xilinx.com:ip:clk_wiz:*"` returns all available versions; take the latest. Record in lockfile `[[ip_resolution]]`.

---

## Task 13: loom lsp

**Spec:** §12.3

**Command:** `loom lsp [--format <fmt>]`

Runs Phases 1-3, then exports the assembled file-set as LSP configuration.

**Output formats:**
- Default: `.loom/lsp.json` (Loom schema)
- `--format svls`: `.svls.toml`
- `--format verible`: file list text
- `--format slang`: compile arguments file

**`loom/lsp.json` schema:**
```json
{
    "version": 1,
    "project": "my_design",
    "defines": ["SIM_ON", "WIDTH=32"],
    "include_dirs": ["lib/axi_common/rtl"],
    "files": [
        {"path": "lib/axi_common/rtl/axi_pkg.sv", "language": "systemverilog"}
    ]
}
```

**Implementation:** Run phases 1-3, format `AssembledFilesets` into the output format, write to `.loom/` directory.

---

## Task 14: loom ip upgrade

**Spec:** §6.5.1

**Command:** `loom ip upgrade [--tool-version <v>] [--apply] [--check-properties]`

1. Find all `vivado_ip` generators in all manifests
2. For each: query the (new) Vivado version for the latest matching VLNV
3. Report which IPs have updates available
4. `--apply`: rewrite `component.toml` / `project.toml` TOML files
5. `--check-properties`: for each IP, validate property names exist in new version

**Output (from spec):**
```
sys_clk: xilinx.com:ip:clk_wiz:6.0 → xilinx.com:ip:clk_wiz:6.1 (update available)
pcie_ep: xilinx.com:ip:pcie4_uscale_plus:1.3 → 1.3 (unchanged)

Run "loom ip upgrade --apply" to update component.toml files.
```

**Implementation:** Python script in `loom-vivado-backend` package. Uses `get_ipdefs` Tcl command to query IP versions.

---

## Task 15: loom migrate xci-to-toml

**Spec:** §6.6.1

**Command:** `loom migrate xci-to-toml <file.xci> [--batch <dir>]`

1. Parse `.xci` XML file (XCI is Vivado's IP config format — XML)
2. Extract VLNV from `<Spirit:componentInstantiation>`
3. Query Vivado for the IP's default properties
4. Diff configured vs. default to find non-default properties
5. Generate TOML `[[generators]]` block

**XML parsing:** Use `quick-xml` crate or call Python's `xml.etree.ElementTree`.

**Output (from spec):**
```toml
[[generators]]
name = "clk_wiz_0"
plugin = "vivado_ip"
[generators.config]
vlnv = "xilinx.com:ip:clk_wiz"
properties = {
    PRIM_IN_FREQ = "200.000",
    CLKOUT1_REQUESTED_OUT_FREQ = "100.000",
}
```

**Batch mode:** Scan directory for all `.xci` files, generate TOML for each, output to stdout or a file.
