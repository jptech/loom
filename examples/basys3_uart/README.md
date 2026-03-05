# Basys 3 UART Echo — Loom Example

A four-component FPGA project demonstrating multi-level dependencies and mixed
constraint scoping, targeting the Digilent Basys 3 board (Artix-7 xc7a35tcpg236-1).

The design echoes bytes received over UART back to the sender and displays the
last received hex value on the 4-digit 7-segment display.

## Structure

```
examples/basys3_uart/
├── workspace.toml
├── lib/
│   ├── clk_gen/        # demo/clk_gen  — MMCME2_BASE: 100 MHz → 50 MHz
│   ├── uart/           # demo/uart     — UART TX/RX with CDC synchronizer
│   ├── seg7_driver/    # demo/seg7_driver — 4-digit 7-segment display controller
│   └── counter/        # demo/counter  — parameterizable N-bit counter
└── projects/
    └── basys3_uart/    # Project targeting Basys 3
        ├── project.toml
        ├── src/top.sv
        └── constraints/basys3.xdc
```

## Dependency graph

```
basys3_uart (project)
  ├── demo/clk_gen
  ├── demo/uart
  └── demo/seg7_driver
        └── demo/counter
```

## Testing without Vivado

```bash
# From examples/basys3_uart/:
loom lint            # Validate all manifests
loom deps tree       # Show dependency tree
loom status          # Show resolved file list and dependency summary
```

## Building with Vivado

Requires Vivado installed and on PATH (free WebPACK license supports xc7a35t).

```bash
# From examples/basys3_uart/:
loom env check       # Verify Vivado is found
loom build           # Full synthesis → implementation → bitstream
```

## What this example demonstrates

- **Four-level dependency graph**: project → seg7_driver → counter (transitive dep)
- **Mixed constraint scoping**: `uart_cdc.xdc` is global-scoped; `clk_gen.xdc` is
  component-scoped; `basys3.xdc` is the board-level global constraint
- **Vendor primitives in a dedicated component**: `MMCME2_BASE` is isolated in
  `clk_gen` so the UART and display logic remain vendor-agnostic
