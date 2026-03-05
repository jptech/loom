# Blinky Demo — Loom FPGA Build System

A minimal multi-component FPGA project targeting the Digilent Basys 3 board (Artix-7 xc7a35tcpg236-1).

## Structure

```
examples/blinky/
├── workspace.toml              # Workspace root
├── lib/
│   ├── counter/                # demo/counter — parameterizable N-bit counter
│   │   ├── component.toml
│   │   └── rtl/counter.sv
│   └── blinky/                 # demo/blinky — LED blinker (depends on counter)
│       ├── component.toml
│       ├── rtl/blinky.sv
│       └── constraints/blinky.xdc   # Component-scoped timing constraint
└── projects/
    └── basys3_blinky/          # Project targeting Basys 3
        ├── project.toml
        ├── src/top.sv
        └── constraints/basys3.xdc   # Board pin constraints
```

## Dependency chain

```
basys3_blinky (project)
  └── demo/blinky (component)
        └── demo/counter (component)
```

## Testing without Vivado

```bash
# From examples/blinky/:
loom lint                # Validate all manifests
loom deps tree           # Show dependency tree
```

## Building with Vivado

Requires Vivado installed and on PATH (free WebPACK license supports xc7a35t).

```bash
# From examples/blinky/:
loom env check           # Verify Vivado is found
loom build               # Full synthesis → implementation → bitstream (~2 min)
```

## What this demo shows

- **Multi-component dependencies**: project → blinky → counter
- **Constraint scoping**: `blinky.xdc` is component-scoped (`-ref`), `basys3.xdc` is global
- **Real synthesizable SystemVerilog**: no vendor primitives, runs on any Artix-7 board
