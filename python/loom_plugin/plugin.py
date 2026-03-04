"""Base classes and entry point for Loom generator plugins.

Plugins are executed as subprocesses by the Loom build system.
Communication uses JSON over stdin/stdout:

    python my_plugin.py --action execute --config '{"key": "value"}' --context '{"build_dir": "..."}'

The plugin prints a JSON response to stdout.
"""

from abc import ABC, abstractmethod
from typing import Dict, List, Optional
import argparse
import json
import sys


class GeneratorPlugin(ABC):
    """Abstract base class for Loom generator plugins.

    Subclass this and implement all abstract methods, then call
    ``main(MyPlugin())`` to create an executable plugin script.
    """

    @property
    @abstractmethod
    def plugin_name(self) -> str:
        """Unique name identifying this plugin type."""
        ...

    @abstractmethod
    def validate_config(self, config: dict) -> List[dict]:
        """Validate plugin configuration.

        Returns a list of diagnostic dicts, each with at least a "message" key.
        An empty list means the config is valid.
        """
        ...

    @abstractmethod
    def compute_cache_key(self, config: dict, input_hashes: Dict[str, str]) -> str:
        """Compute a deterministic cache key for the given config and input hashes.

        Returns a hex string. Identical inputs must produce identical keys.
        """
        ...

    @abstractmethod
    def execute(self, config: dict, context: dict) -> dict:
        """Execute the generator.

        Args:
            config: Plugin configuration from the manifest.
            context: Build context with keys like ``build_dir``, ``workspace_root``,
                     ``project_root``, ``project_name``.

        Returns:
            A dict with keys:
            - ``success`` (bool): Whether execution succeeded.
            - ``produced_files`` (list[str]): Paths of files produced.
            - ``log`` (list[str]): Log lines.
            - ``error`` (str, optional): Error message if ``success`` is False.
        """
        ...

    @abstractmethod
    def clean(self, config: dict, context: dict) -> None:
        """Remove any generated artifacts.

        Args:
            config: Plugin configuration from the manifest.
            context: Build context (at minimum contains ``build_dir``).
        """
        ...


def main(plugin: GeneratorPlugin) -> None:
    """Entry point for subprocess-executed plugins.

    Call this from your plugin script's ``if __name__ == "__main__"`` block::

        from loom_plugin import GeneratorPlugin, main

        class MyGenerator(GeneratorPlugin):
            ...

        if __name__ == "__main__":
            main(MyGenerator())
    """
    parser = argparse.ArgumentParser(description=f"Loom plugin: {plugin.plugin_name}")
    parser.add_argument("--action", required=True, choices=["validate", "cache_key", "execute", "clean"])
    parser.add_argument("--config", required=True, help="JSON-encoded plugin config")
    parser.add_argument("--context", default="{}", help="JSON-encoded build context")
    parser.add_argument("--input_hashes", default="{}", help="JSON-encoded input file hashes")
    args = parser.parse_args()

    config = json.loads(args.config)
    context = json.loads(args.context)

    try:
        if args.action == "validate":
            result = plugin.validate_config(config)
        elif args.action == "cache_key":
            input_hashes = json.loads(args.input_hashes)
            result = {"cache_key": plugin.compute_cache_key(config, input_hashes)}
        elif args.action == "execute":
            result = plugin.execute(config, context)
        elif args.action == "clean":
            plugin.clean(config, context)
            result = {"success": True}
        else:
            result = {"error": f"Unknown action: {args.action}"}

        print(json.dumps(result))
    except Exception as e:
        error_result = {"success": False, "error": str(e)}
        print(json.dumps(error_result))
        sys.exit(1)
