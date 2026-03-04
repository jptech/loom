# Phase 2: Generators, Caching, CLI Polish, Windows

**Prerequisites:** Phase 1 complete and passing
**Goal:** Code generation works. Incremental builds work. The CLI is polished. Windows is a first-class target. LSP integration works.

## Spec Reference
`system_plan.md` §6 (Code Generation), §7.3 (Caching), §7.5 (Checkpoints/Resume), §7.6 (Dry Run), §12.3 (LSP), §13.4 (Windows)

## What's New in Phase 2

- **GENERATE phase active** (was skipped in Phase 1)
- `command` generator plugin — arbitrary shell commands
- Generator DAG with input/output dependency detection
- Cache key computation per generator
- `vivado_ip` generator — declarative IP from TOML
- Constraint templating (`.xdc.tpl` preprocessing)
- Build checkpoint tracking (`build_state.json`) + `--resume`
- `--stop-after`, `--start-at` build sub-phase control
- `--dry-run` mode
- JSON build report with hierarchical metrics
- `loom lsp` command (LSP config export)
- `loom ip upgrade` (report IP version updates)
- `loom migrate xci-to-toml` (convert Vivado .xci to TOML)
- CLI: color output, progress display, `--json`, `-j N` (actual parallelism)
- PyO3 integration: Python plugin loading
- Windows CI: tests passing on Windows

## Task List

| # | Task | Key Deliverable |
|---|---|---|
| 01 | [Generator Types](./01-generator-types.md) | Generator manifest types, DAG data structures |
| 02 | [Command Generator](./02-command-generator.md) | `command` plugin executes shell commands |
| 03 | [Cache Service](./03-cache-service.md) | Cache key computation, cache store in `.build/cache/` |
| 04 | [Generator DAG](./04-generator-dag.md) | Topological ordering, input/output overlap detection |
| 05 | [Generate Phase](./05-09-remaining.md) | Phase 2 execution, skip if cached |
| 06 | [Constraint Templating](./05-09-remaining.md) | `.xdc.tpl` preprocessing |
| 07 | [Build Checkpoints](./05-09-remaining.md) | `build_state.json`, `--resume`, `--stop-after`, `--start-at` |
| 08 | [Dry Run Mode](./05-09-remaining.md) | `--dry-run` shows plan without building |
| 09 | [JSON Build Report](./05-09-remaining.md) | Metrics extraction, JSON output |
| 10 | [CLI Polish](./10-15-remaining.md) | Color, progress bars, `--json`, `-j N` |
| 11 | [PyO3 Integration](./10-15-remaining.md) | Python plugin loading, subprocess execution |
| 12 | [Vivado IP Generator](./10-15-remaining.md) | `vivado_ip` plugin, floating VLNV |
| 13 | [loom lsp](./10-15-remaining.md) | LSP config export for HDL editors |
| 14 | [loom ip upgrade](./10-15-remaining.md) | IP version update reporting |
| 15 | [loom migrate xci-to-toml](./10-15-remaining.md) | Convert .xci files to TOML generators |

## Key Data Structures for Phase 2

### Generator Manifest Types

Add to `component.toml` and `project.toml`:
```toml
[[generators]]
name = "regmap"
plugin = "command"
command = "python scripts/gen_regs.py"
inputs = ["regs/radar_ctrl_regs.yaml"]
outputs = ["generated/radar_ctrl_regs.sv", "generated/radar_ctrl_regs.h"]
fileset = "synth"

[[generators]]
name = "sys_clk"
plugin = "vivado_ip"
[generators.config]
vlnv = "xilinx.com:ip:clk_wiz"
properties = { PRIM_IN_FREQ = "125.000", CLKOUT1_REQUESTED_OUT_FREQ = "100.000" }
```

### Cache Key Formula

```
generator_cache_key = hash(
    plugin_name + plugin_version +
    config_toml_canonical +
    for each input: file_content_hash +
    tool_version (for vendor IP generators) +
    target_part (for vendor IP generators)
)
```

### Build State JSON (for --resume)

```json
{
    "cache_key": "sha256:abc123...",
    "backend": "vivado",
    "phases_completed": ["synthesis", "optimize", "place"],
    "phases_failed": ["route"],
    "checkpoints": {
        "synthesis": ".build/project/default/post_synth.dcp",
        "place": ".build/project/default/post_place.dcp"
    },
    "failure": {
        "phase": "route",
        "exit_code": 1,
        "log": ".build/project/default/route.log"
    }
}
```

## Windows-Specific Requirements (Phase 2)

- All path manipulation uses `PathBuf` (no raw string paths)
- Shell command execution on Windows uses `cmd /c` or PowerShell
- `command_windows` field in generator config for Windows overrides
- CI matrix: GitHub Actions `windows-latest` runner
- Forward-slash paths in all generated scripts (already handled in Tcl gen)

## Vivado Backend Changes (Phase 2)

The Vivado backend transitions from a pure Rust implementation to:
1. Rust core (`loom-vivado` crate) handles script generation and tool invocation
2. Python plugin (`loom-vivado-backend` pip package) handles:
   - IP generation via `vivado_ip` generator
   - Metrics extraction (Tcl query phase)
   - Migration tools

The `BackendPlugin` trait gains new methods in Phase 2:
- `resume_build()` — resume from checkpoint
- `extract_metrics()` — structured metrics from completed build

Note: `execute_ooc_synthesis()` is added in Phase 3 (OOC synthesis support).
