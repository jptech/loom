"""Vivado IP Generator plugin.

Generates Vivado IP cores from declarative TOML configuration.
Uses ``vivado -mode batch`` to create and configure IP.

Configuration keys:
    vlnv (str): IP VLNV identifier, e.g. "xilinx.com:ip:clk_wiz"
    name (str): Instance name for the generated IP
    properties (dict): IP configuration properties to set

Example manifest entry::

    [[generators]]
    name = "sys_clk"
    plugin = "vivado_ip"
    [generators.config]
    vlnv = "xilinx.com:ip:clk_wiz"
    [generators.config.properties]
    PRIM_IN_FREQ = "200.000"
    CLKOUT1_REQUESTED_OUT_FREQ = "100.000"
"""

import hashlib
import os
import subprocess
import tempfile
from typing import Dict, List

from loom_plugin import GeneratorPlugin, main


class VivadoIpGenerator(GeneratorPlugin):
    @property
    def plugin_name(self) -> str:
        return "vivado_ip"

    def validate_config(self, config: dict) -> List[dict]:
        diagnostics = []
        if "vlnv" not in config:
            diagnostics.append({"message": "Missing required 'vlnv' in vivado_ip config"})
        if "name" not in config:
            diagnostics.append({"message": "Missing required 'name' in vivado_ip config"})
        return diagnostics

    def compute_cache_key(self, config: dict, input_hashes: Dict[str, str]) -> str:
        import json

        content = json.dumps(config, sort_keys=True) + json.dumps(input_hashes, sort_keys=True)
        return hashlib.sha256(content.encode()).hexdigest()

    def execute(self, config: dict, context: dict) -> dict:
        vlnv = config["vlnv"]
        ip_name = config.get("name", "ip_0")
        properties = config.get("properties", {})
        build_dir = context.get("build_dir", ".")
        part = config.get("part", "xc7a35t")

        output_dir = os.path.join(build_dir, "ip", ip_name)
        os.makedirs(output_dir, exist_ok=True)

        # Generate Tcl script
        tcl_lines = [
            f"create_project -in_memory -part {{{part}}}",
            f"set_property target_language Verilog [current_project]",
            f"create_ip -vlnv {{{vlnv}}} -module_name {ip_name} -dir {{{output_dir}}}",
        ]

        if properties:
            prop_list = " \\\n    ".join(
                f"CONFIG.{k} {{{v}}}" for k, v in properties.items()
            )
            tcl_lines.append(f"set_property -dict [list \\\n    {prop_list} \\\n] [get_ips {ip_name}]")

        tcl_lines.append(f"generate_target all [get_ips {ip_name}]")

        tcl_content = "\n".join(tcl_lines) + "\n"

        # Write and execute Tcl
        tcl_path = os.path.join(output_dir, "generate_ip.tcl")
        with open(tcl_path, "w") as f:
            f.write(tcl_content)

        log_lines = [f"Generated Tcl: {tcl_path}"]

        try:
            result = subprocess.run(
                ["vivado", "-mode", "batch", "-source", tcl_path],
                capture_output=True,
                text=True,
                cwd=output_dir,
            )
            log_lines.extend(result.stdout.splitlines())

            if result.returncode != 0:
                log_lines.extend(result.stderr.splitlines())
                return {
                    "success": False,
                    "produced_files": [],
                    "log": log_lines,
                    "error": f"Vivado exited with code {result.returncode}",
                }

            # Collect produced files
            produced = []
            for root, _dirs, files in os.walk(output_dir):
                for fname in files:
                    produced.append(os.path.join(root, fname))

            return {
                "success": True,
                "produced_files": produced,
                "log": log_lines,
            }
        except FileNotFoundError:
            return {
                "success": False,
                "produced_files": [tcl_path],
                "log": log_lines + ["Vivado not found on PATH — Tcl script generated but not executed."],
                "error": "Vivado not found on PATH",
            }

    def clean(self, config: dict, context: dict) -> None:
        import shutil

        ip_name = config.get("name", "ip_0")
        build_dir = context.get("build_dir", ".")
        output_dir = os.path.join(build_dir, "ip", ip_name)
        if os.path.exists(output_dir):
            shutil.rmtree(output_dir)


if __name__ == "__main__":
    main(VivadoIpGenerator())
