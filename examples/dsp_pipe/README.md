# dsp_pipe — DSP Signal Processing Pipeline

FIR filter pipeline demonstrating Loom's **generators** for data files,
**simple profiles**, and **build configuration** (reports).

## Features Demonstrated

| Feature | How Used |
|---------|----------|
| Generators | `gen_fir_coeffs.py` and `gen_window_lut.py` produce `.mem` files |
| Simple profiles | `narrow` (12-bit/16-tap), `standard` (16-bit/32-tap), `wide` (24-bit/64-tap) |
| Build config | `[build.reports]` with utilization, timing, and power reports |
| Platform manifests | Arty A7 (Vivado) |

## Architecture

AXI-Stream pipeline with skid buffers between stages:

```
data_in -> [gain_ctrl] -> [skid] -> [window_func] -> [skid] -> [fir_filter] -> data_out
```

| Component | Description |
|-----------|-------------|
| `dsp_common` | AXI-Stream skid buffer for backpressure decoupling |
| `gain_ctrl` | Programmable gain with saturation detection |
| `window_func` | Hann/Hamming window via ROM lookup table |
| `fir_filter` | Transposed-form FIR with coefficient ROM |

## Generators

**FIR coefficients** (requires `scipy`, falls back to sinc without windowing):
```bash
python3 lib/fir_filter/scripts/gen_fir_coeffs.py \
    --order 32 --width 16 --cutoff 0.25 \
    --output lib/fir_filter/generated/fir_coeffs.mem
```

**Window LUT** (pure Python, no dependencies):
```bash
python3 lib/window_func/scripts/gen_window_lut.py \
    --type hann --size 256 --width 16 \
    --output lib/window_func/generated/window_lut.mem
```

## Profiles

```bash
loom build --profile narrow      # 12-bit, 16-tap — minimal resources
loom build --profile standard    # 16-bit, 32-tap — balanced (default)
loom build --profile wide        # 24-bit, 64-tap — maximum precision
```
