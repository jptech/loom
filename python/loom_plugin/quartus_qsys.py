"""
Quartus Platform Designer (Qsys) generator plugin for Loom.

Manages .qsys files — regenerates from canonical XML export,
avoiding Quartus GUI state pollution.

Usage in component.toml:
    [[generators]]
    name = "pcie_subsystem"
    plugin = "quartus_qsys"
    [generators.config]
    qsys_file = "ip/pcie_subsystem.qsys"
"""

import hashlib
import os
import subprocess
import sys
from pathlib import Path


class QuartusQsysGenerator:
    """Generator plugin that manages Quartus Platform Designer .qsys files."""

    name = "quartus_qsys"

    def __init__(self, config: dict):
        self.qsys_file = config.get("qsys_file", "")
        self.part = config.get("part", "")
        self.tool_version = config.get("tool_version", "")
        self.extra_args = config.get("extra_args", [])

    def compute_hash(self) -> str:
        """Compute a deterministic hash of inputs for caching."""
        h = hashlib.sha256()
        if os.path.exists(self.qsys_file):
            with open(self.qsys_file, "rb") as f:
                h.update(f.read())
        h.update(self.part.encode())
        h.update(self.tool_version.encode())
        return h.hexdigest()[:16]

    def needs_regeneration(self, output_dir: str) -> bool:
        """Check if the IP needs to be regenerated."""
        hash_file = os.path.join(output_dir, ".qsys_hash")
        current_hash = self.compute_hash()

        if not os.path.exists(hash_file):
            return True

        with open(hash_file, "r") as f:
            cached_hash = f.read().strip()

        return cached_hash != current_hash

    def generate(self, output_dir: str) -> dict:
        """
        Generate/regenerate the Qsys subsystem.

        Returns dict with:
            - success: bool
            - outputs: list of generated file paths
            - log: str
        """
        os.makedirs(output_dir, exist_ok=True)

        if not os.path.exists(self.qsys_file):
            return {
                "success": False,
                "outputs": [],
                "log": f"Qsys file not found: {self.qsys_file}",
            }

        if not self.needs_regeneration(output_dir):
            return {
                "success": True,
                "outputs": self._find_outputs(output_dir),
                "log": "Qsys IP is up to date (cached)",
            }

        # Run qsys-generate
        qsys_generate = self._find_qsys_generate()
        if not qsys_generate:
            return {
                "success": False,
                "outputs": [],
                "log": "qsys-generate not found. Ensure Quartus is installed and on PATH.",
            }

        cmd = [
            qsys_generate,
            self.qsys_file,
            "--output-directory=" + output_dir,
            "--synthesis=VERILOG",
            "--family-migration=ON",
        ]

        if self.part:
            cmd.append(f"--part={self.part}")

        cmd.extend(self.extra_args)

        log_lines = [f"Running: {' '.join(cmd)}"]

        try:
            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=600,
            )
            log_lines.append(result.stdout)
            if result.stderr:
                log_lines.append(result.stderr)

            if result.returncode != 0:
                return {
                    "success": False,
                    "outputs": [],
                    "log": "\n".join(log_lines),
                }

        except FileNotFoundError:
            return {
                "success": False,
                "outputs": [],
                "log": f"qsys-generate not found at {qsys_generate}",
            }
        except subprocess.TimeoutExpired:
            return {
                "success": False,
                "outputs": [],
                "log": "qsys-generate timed out after 600 seconds",
            }

        # Save hash for caching
        hash_file = os.path.join(output_dir, ".qsys_hash")
        with open(hash_file, "w") as f:
            f.write(self.compute_hash())

        outputs = self._find_outputs(output_dir)
        log_lines.append(f"Generated {len(outputs)} output files")

        return {
            "success": True,
            "outputs": outputs,
            "log": "\n".join(log_lines),
        }

    def _find_qsys_generate(self) -> str | None:
        """Find qsys-generate executable."""
        # Check QUARTUS_ROOTDIR
        quartus_root = os.environ.get("QUARTUS_ROOTDIR", "")
        if quartus_root:
            candidate = os.path.join(
                quartus_root, "..", "sopc_builder", "bin", "qsys-generate"
            )
            if os.path.exists(candidate):
                return candidate

        # Try PATH
        import shutil

        path = shutil.which("qsys-generate")
        if path:
            return path

        return None

    def _find_outputs(self, output_dir: str) -> list[str]:
        """Find all generated output files."""
        outputs = []
        for root, _dirs, files in os.walk(output_dir):
            for f in files:
                if f.endswith((".v", ".sv", ".vhd", ".qip", ".sdc")):
                    outputs.append(os.path.join(root, f))
        return outputs


def main():
    """CLI entry point for standalone testing."""
    if len(sys.argv) < 2:
        print("Usage: quartus_qsys.py <qsys_file> [output_dir]")
        sys.exit(1)

    qsys_file = sys.argv[1]
    output_dir = sys.argv[2] if len(sys.argv) > 2 else "./qsys_output"

    gen = QuartusQsysGenerator({"qsys_file": qsys_file})
    result = gen.generate(output_dir)

    print(f"Success: {result['success']}")
    print(f"Outputs: {result['outputs']}")
    print(f"Log:\n{result['log']}")

    sys.exit(0 if result["success"] else 1)


if __name__ == "__main__":
    main()
