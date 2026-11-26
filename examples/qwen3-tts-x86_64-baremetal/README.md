# `qwen3-tts-x86_64-baremetal` — Pure NOPRAY 32 KiB Bare-Metal Qwen3-TTS-12Hz-0.6B VM

> **Zero Linux · Zero Compiler · Zero Assembler · Zero Linker · Zero Libc · Zero Python**  
> `DMCS-1.1` · Tier **`L3` W^X** · `32,768 B` (`32 KiB`) ELF32 Multiboot 1 → 64-bit Long Mode + AVX2/FMA/F16C · **`2,806` decoded instructions** · **`0` errors / `0` warnings** (`nopray audit --strict`) · **`0` Linux syscalls**

## Real Audio Demos

Synthesized end-to-end for `"Hola Marlon, esto es código máquina puro con nopray, sin Linux y sin compilador."` (speaker `serena`, language `Spanish`, greedy deterministic `repetition_penalty=1.05`):

- 🔊 **Bare-metal NOPRAY KVM output (`6.40 s`, `307,244 B`, 24 kHz mono S16LE):** [`../../assets/audio/nopray_qwen3_tts_baremetal_es.wav`](../../assets/audio/nopray_qwen3_tts_baremetal_es.wav)  
  — Matches FP32 NumPy to **`89.32 dB` SNR / `0.9999999994` cosine** (`153,359 / 153,600` bit-identical 16-bit PCM samples, max error `±1 LSB`).
- 🔊 **Official `qwen-tts` PyTorch greedy reference (`6.72 s`, `322,604 B`, 24 kHz mono S16LE):** [`../../assets/audio/official_qwen3_tts_pytorch_es.wav`](../../assets/audio/official_qwen3_tts_pytorch_es.wav)

---

## Architecture (12 Hand-Authored Hex Slots, `0x100000`–`0x10CFFF`)

| Slot | VAddr | Size | Used | Purpose |
| :--- | :---: | ---: | ---: | :--- |
| `0000_elf32_hdr.hex` | `0x100000` | `0x100` | `140 B` | ELF32 header (`EM_386`), 2 W^X `PT_LOAD` segments, 12-byte Multiboot 1 header at `+0x80` |
| `0100_boot32.hex` (`bits = 32`) | `0x100100` | `0x100` | `169 B` | 32-bit Multiboot 1 entry: zeroes BSS, builds 4-level 2 MiB huge-page identity map `0..4 GiB`, PAE/LME/PG, `lgdt`, `jmp far 0x08:boot64` |
| `0200_boot64.hex` | `0x100200` | `0x200` | `203 B` | 64-bit Long Mode setup: `rsp=0x124000`, `r15=0x10C000`, enables SSE/AVX2/FMA/F16C (`XCR0=7`, `MXCSR=0x1F80`), UART init, PIT TSC calibration, `-initrd` discovery |
| `0400_io_drivers.hex` | `0x100400` | `0x400` | `550 B` | Bare-metal hardware drivers: COM1 (`0x3F8`) serial telemetry, `isa-debugcon` (`0xE9` `rep outsb`) binary WAV stream, PIT Ch2 TSC calibration, `isa-debug-exit` (`0xF4`) / ACPI S5 |
| `0800_tts_main.hex` | `0x100800` | `0x400` | `308 B` | Orchestrates Talker+CP + Decoder+Vocoder, logs PIT-calibrated `rdtsc` ms timings to COM1, builds 44-byte RIFF/WAVE header, streams PCM over `0xE9` |
| `0c00_rodata.hex` | `0x100C00` | `0x400` | `580 B` | `gdt64`, GDTR, MXCSR, telemetry strings, IEEE-754 polynomial constants (`expf`, `sin²`, `erf`/GELU, `32767.0`), 44-byte WAV header template, default prompt IDs |
| `1000_math_avx2.hex` | `0x101000` | `0x1000` | `1,683 B` | 14 AVX2+FMA+F16C kernels (`gemv_q8_0s`, `gemv_f16`, `rmsnorm`, `layernorm`, `expf_ymm0`, `sin2_ymm0`, `silu_mul`, `gelu_inplace`, `softmax_inplace`, `rope_apply`, `dequant_q8_row`, `dequant_f16_row`, `vec_add_inplace`, `snakebeta_channel`) |
| `2000_qwen3_layer.hex` | `0x102000` | `0x1000` | `1,394 B` | Unified Qwen3 Transformer layer (`dir_lookup`, `qwen3_gemv_tid`, QK-Norm, RoPE, KV-cache, GQA 16Q/8KV attention, SwiGLU MLP) serving both Talker 28L & Code Predictor 5L |
| `3000_talker_cp.hex` | `0x103000` | `0x1000` | `1,008 B` | Autoregressive Talker 28L + 15-group Code Predictor 5L loop with suppression mask (`[2048..3071]\{2150}`), `min_new_tokens=2`, and `repetition_penalty=1.05` |
| `4000_decoder_xfmr.hex` | `0x104000` | `0x1000` | `1,790 B` | Speech Tokenizer 12Hz V2 Part 1: 16-codebook RVQ lookup, causal `pre_conv`, `input_proj`, 8-layer sliding-window=72 Pre-Transformer, final norm & `output_proj` |
| `5000_decoder_vocoder.hex` | `0x105000` | `0x2000` | `3,582 B` | Speech Tokenizer Part 2: `2oc × 48t` AVX2+FMA+F16C tiled `conv1d_causal_tid` & `transconv1d_causal_tid`, causal `dwconv`, 2× ConvNeXt upsample, 4-stage BigVGAN (`rates=[8,5,4,3]`) + SnakeBeta + FP32→S16LE |
| `c000_data.hex` | `0x10C000` | `0x1000` | `0 B` (+`0x43000` BSS) | Writable state page (`r15 = 0x10C000`) + page tables + BSP stack |

---

## Head-to-Head Benchmark (Same Physical AMD Ryzen 7 5825U Silicon)

Full breakdown and raw JSONs: [`benchmarks/REPORT.md`](./benchmarks/REPORT.md) and [`benchmarks/summary.json`](./benchmarks/summary.json).

| Engine & Config | Cores | Talker+CP | Decoder+Vocoder | Total Synth | Cold→WAV | Peak RSS | Stack / Code | Guest Linux Syscalls |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **NOPRAY Bare-Metal KVM (Run 1)** | **1 vCPU** | **`15.46 s`** | `25.75 s` | **`41.22 s`** (`6.44×` RTF) | **`53.04 s`** | **`2,429 MiB`** | **`32 KiB` ELF** | **`0`** |
| Official `qwen-tts` PyTorch (`1t FP32`) | 1 thread | `46.08 s` | **`18.41 s`** | `64.50 s` (`9.60×` RTF) | `81.42 s` | `5,637 MiB` | `1,375 MiB` `.venv` | `134,879+` |
| Official `qwen-tts` PyTorch (`1t BF16`) | 1 thread | `61.12 s` | `400.75 s` | `461.86 s` (`71.28×` RTF) | `472.14 s` | `3,073 MiB` | `1,375 MiB` `.venv` | `134,879+` |
| Official `qwen-tts` PyTorch (`8t FP32`) | **8 threads** | `47.52 s` | **`6.63 s`** | `54.15 s` (`8.06×` RTF) | `63.51 s` | `5,648 MiB` | `1,375 MiB` `.venv` | `134,879+` |
| Official `qwen-tts` PyTorch (`8t BF16`) | **8 threads** | `24.68 s` | `386.85 s` | `411.52 s` (`63.51×` RTF) | `420.35 s` | `3,059 MiB` | `1,375 MiB` `.venv` | `134,879+` |

---

## Reproducing From Scratch

### 1. Rebuild & strictly audit the 32 KiB bare-metal binary (zero weights needed)

```bash
./tools/nopray.sh build examples/qwen3-tts-x86_64-baremetal
./tools/nopray.sh audit --strict examples/qwen3-tts-x86_64-baremetal
```

### 2. Pack `weights.nqts` (`DMCS-L3`, `1.042 GiB`) and boot on QEMU/KVM

See [`docs/WEIGHTS_SPEC.md`](./docs/WEIGHTS_SPEC.md) and [`scripts/pack_weights.py`](./scripts/pack_weights.py) to pack `Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice` into `weights.nqts` (and verify against pure NumPy via `scripts/numpy_ref.py`), then boot directly with zero Linux:

```bash
qemu-system-x86_64 -enable-kvm -cpu host -m 3G -smp 1 \
  -display none -monitor none -no-reboot \
  -kernel examples/qwen3-tts-x86_64-baremetal/build/nopray-qwen-tts.elf \
  -initrd weights.nqts \
  -chardev stdio,id=c1 -serial chardev:c1 \
  -chardev file,id=c2,path=nopray_es.wav -device isa-debugcon,iobase=0xe9,chardev=c2 \
  -device isa-debug-exit,iobase=0xf4,iosize=0x04
```
