# uCaspian Neuromorphic Processor — Loom Example

A multi-target FPGA project demonstrating Loom's cross-backend support with a
real-world neuromorphic processor design.

**Original project:** [uCaspian](https://github.com/ornl/ucaspian) by Oak Ridge
National Laboratory (MIT License). See [LICENSES.md](LICENSES.md) for full
attribution.

## What is uCaspian?

uCaspian is a neuromorphic "microcontroller" implementing a spiking neural
network (SNN) processor: 256 neurons, 4096 synapses, activity-driven sparse
evaluation, and a variable-length packet protocol for host communication over
UART.

## Structure

```
lib/
  ucaspian_core/   — 11-module neuromorphic processor core (vendor-agnostic)
  io/              — UART, FIFO, and utility IP (vendor-agnostic)
projects/
  ice40_ucaspian/  — iCE40 UP5K target (yosys + nextpnr, UPduino v3 board)
  artix7_ucaspian/ — Artix-7 target (Vivado, Numato Mimas A7 board)
```

## Dependency graph

```
ice40_ucaspian / artix7_ucaspian
  ├── ucaspian/core   — neuromorphic processor (neurons, synapses, axons, ...)
  └── ucaspian/io     — UART TX/RX with FIFOs, clock dividers
```

The core RTL is identical across both targets. Only the project-level top module
differs: the ice40 target uses `SB_HFOSC` (Lattice internal oscillator), while
the Artix-7 target uses `MMCME2_BASE` + `BUFG` to derive 24 MHz from the
board's 100 MHz oscillator.

## Building

### iCE40 (yosys + nextpnr)

```bash
# Requires: yosys, nextpnr-ice40, icepack
loom build -p ice40_ucaspian
```

### Artix-7 (Vivado)

```bash
# Requires: Vivado (WebPACK edition supports xc7a50t)
loom build -p artix7_ucaspian
```

## What this example demonstrates

- **Multi-backend targeting:** same core RTL builds for iCE40 (open-source) and
  Artix-7 (Vivado)
- **Component reuse:** UART IP shared across both projects
- **Vendor-primitive isolation:** clock generation is project-specific; the core
  is fully portable
- **Real-world complexity:** 11 tightly-coupled RTL modules with pipelines,
  dual-port RAMs, and activity-driven sparse computation
