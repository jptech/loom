# regmap — GPIO+Timer SoC Peripheral

Register-mapped GPIO and Timer peripheral demonstrating Loom's advanced features:
**generators**, **component variants**, **dimensional profiles**, and **platform manifests**.

## Features Demonstrated

| Feature | How Used |
|---------|----------|
| Generators | `gen_regmap.py` produces SystemVerilog register file from JSON spec |
| Component variants | `axilite` (AXI-Lite wrapper) vs `simple` (direct bus) |
| Dimensional profiles | `board` (arty_a7/icebreaker/sim) x `tier` (full/lite) |
| Platform manifests | Arty A7 (Vivado), iCEBreaker (yosys), sim_generic (virtual) |

## Architecture

```
gpio_timer_top
  +-- simple_regmap / axilite_regmap   (variant-selected wrapper)
        +-- gpio_timer_core            (hand-written GPIO + timer + IRQ)
              +-- gpio_timer_regs      (GENERATED register file)
```

## Generator

The register file is generated from `lib/regmap_core/spec/gpio_timer_regs.json`:

```bash
python3 lib/regmap_core/scripts/gen_regmap.py \
    lib/regmap_core/spec/gpio_timer_regs.json \
    lib/regmap_core/generated/
```

Produces:
- `gpio_timer_regs_pkg.sv` — address constants and field definitions
- `gpio_timer_regs.sv` — register file with write decode, read mux, W1C logic

## Variant Selection

Platform tags drive automatic variant selection:

- **Arty A7** (`tags = ["bus:axilite"]`) selects the `axilite` variant with AXI-Lite protocol wrapper
- **iCEBreaker** (`tags = ["bus:simple"]`) selects the `simple` variant with direct bus passthrough

## Dimensional Profiles

Build any combination of board and feature tier:

```bash
loom build --profile board=arty_a7,tier=full    # 16 GPIO, 32-bit timer, AXI-Lite
loom build --profile board=icebreaker,tier=lite  # 8 GPIO, 16-bit timer, simple bus
loom build --profile board=sim,tier=full         # simulation, no physical target
```

## Register Map

| Offset | Name | Access | Description |
|--------|------|--------|-------------|
| 0x00 | GPIO_DIR | RW | GPIO direction (1=output) |
| 0x04 | GPIO_OUT | RW | GPIO output data |
| 0x08 | GPIO_IN | RO | GPIO input data |
| 0x0C | IRQ_ENABLE | RW | Interrupt enable mask |
| 0x10 | IRQ_STATUS | RO | Interrupt status |
| 0x14 | IRQ_CLEAR | W1C | Write-1-to-clear IRQ |
| 0x18 | TIMER_CTRL | RW | Timer control (enable, auto-reload) |
| 0x1C | TIMER_COUNT | RO | Timer current count |
| 0x20 | TIMER_COMPARE | RW | Timer compare value |
| 0x24 | ID | RO | Peripheral ID (0xCA5B1A01) |
