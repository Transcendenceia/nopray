# NOPRAY Bare-Metal Qwen3-TTS-12Hz-0.6B vs Official `qwen-tts` (PyTorch CPU) — Benchmark Report

**Date:** 2026-11-26  
**Silicon (identical physical CPU for every run):** `AMD Ryzen 7 5825U` (8C/16T Zen 3, AVX2 + FMA + F16C, no AVX-512)  
**Model:** `Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice` (Talker 28L + Code Predictor 5L + Speech Tokenizer 12Hz V2 Decoder + 4-stage BigVGAN/SnakeBeta Vocoder)  
**Prompt (identical across all runs):** `"Hola Marlon, esto es código máquina puro con nopray, sin Linux y sin compilador."` (speaker `serena`, language `Spanish`, greedy deterministic `do_sample=False`, `subtalker_dosample=False`, `repetition_penalty=1.05`)

---

## 1. Executive Comparison Table

| Engine & Configuration | Cores | Weights Precision | Talker+CP | ms/frame | Decoder+Vocoder | Total Synth | Cold→WAV Wall | RTF (synth) | RTF (cold) | Peak RSS (`VmHWM`) | Code/Runtime Stack | Weights Disk | Guest Linux Syscalls |
|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| **NOPRAY Bare-Metal VM (KVM, Run 1)** | **1 vCPU** | `Q8_0S` (LM) + `F16` (Dec) + `F32` | **`15.46 s`** | **`193.3 ms`** | `25.75 s` | **`41.22 s`** | **`53.04 s`** | **`6.44×`** | **`8.29×`** | **`2,429 MiB`** (`2.37 GiB`) | **`32 KiB` ELF** (0 OS, 0 libs) | **`1.042 GiB`** | **`0`** (no Linux) |
| **NOPRAY Bare-Metal VM (KVM, Run 2)** | **1 vCPU** | `Q8_0S` (LM) + `F16` (Dec) + `F32` | **`15.76 s`** | **`197.0 ms`** | `27.07 s` | **`42.83 s`** | **`54.27 s`** | **`6.69×`** | **`8.48×`** | **`2,452 MiB`** (`2.39 GiB`) | **`32 KiB` ELF** | **`1.042 GiB`** | **`0`** (no Linux) |
| Official `qwen-tts` PyTorch (`1t FP32`) | 1 thread | FP32 | `46.08 s` | `548.6 ms` | **`18.41 s`** | `64.50 s` | `81.42 s` | `9.60×` | `12.12×` | `5,637 MiB` (`5.50 GiB`) | `1,375 MiB` `.venv` (+OS) | `2.327 GiB` | `134,879+` (import+load alone) |
| Official `qwen-tts` PyTorch (`1t BF16`) | 1 thread | BF16 | `61.12 s` | `754.5 ms` | `400.75 s` | `461.86 s` | `472.14 s` | `71.28×` | `72.86×` | `3,073 MiB` (`3.00 GiB`) | `1,375 MiB` `.venv` (+OS) | `2.327 GiB` | `134,879+` |
| Official `qwen-tts` PyTorch (`8t FP32`) | **8 threads** | FP32 | `47.52 s` | `565.7 ms` | **`6.63 s`** | `54.15 s` | `63.51 s` | `8.06×` | `9.45×` | `5,648 MiB` (`5.52 GiB`) | `1,375 MiB` `.venv` (+OS) | `2.327 GiB` | `134,879+` |
| Official `qwen-tts` PyTorch (`8t BF16`) | **8 threads** | BF16 | `24.68 s` | `304.6 ms` | `386.85 s` | `411.52 s` | `420.35 s` | `63.51×` | `64.87×` | `3,059 MiB` (`2.99 GiB`) | `1,375 MiB` `.venv` (+OS) | `2.327 GiB` | `134,879+` |

> **Key takeaway:** On a **single KVM vCPU**, our 32 KiB hand-authored bare-metal hexadecimal VM (`nopray-qwen-tts.elf`, `2,806` instructions, `0` errors / `0` warnings under `nopray audit --strict`) beat **every single official `qwen-tts` PyTorch CPU configuration** — including **8-thread FP32** (`41.22s` vs `54.15s` synthesis, `53.04s` vs `63.51s` cold-to-WAV, `6.44×` vs `8.06×` RTF) — while cutting peak RAM by **`2.32×`** (`2.37 GiB` vs `5.52 GiB`) and runtime code footprint by **`~44,000×`** (`32 KiB` vs `1.375 GiB` Python/PyTorch `.venv`, excluding Linux itself).

---

## 2. Numerical & Audio Verification

- **WAV format:** `24,000 Hz`, mono, 16-bit signed little-endian PCM (`307,244 B` = `44 B` RIFF/WAVE header + `153,600` samples = `6.400 s` audio, `80` codec frames at `12.5 Hz`).
- **Bit-exactness of bare-metal Talker 28L + Code Predictor 5L:** `1,280 / 1,280` (`100.000%`) token agreement across all 80 frames × 16 RVQ codebooks against `scripts/numpy_ref.py`.
- **Numerical fidelity of bare-metal Decoder + BigVGAN Vocoder (`artifacts/nopray_es.wav` vs `artifacts/numpy_ref_es.wav`):**
  - Cosine similarity: **`0.9999999994`** (9 nines)
  - Waveform SNR: **`89.32 dB`**
  - Max absolute error: **`3.05e-5` (`±1 LSB` out of 65,535)**
  - Exact 16-bit PCM sample matches: **`153,359 / 153,600` (`99.84%`)**
- **Decoder parity (`numpy_ref.py` vs official PyTorch on identical golden codes):** `rvq_cos = 0.99999999999996`, `pre_xfmr_cos = 0.99999996`, `upsample_cos = 0.99999989`, `wav_cos = 0.99999991`, **`wav_snr_db = 67.42 dB`**.
- **Audible intelligibility:** Verified directly by playback/listening — natural, clean Spanish speech pronouncing the entire sentence clearly.

---

## 3. Why Each Stage Behaves the Way It Does (Honest Systems Analysis)

1. **Talker 28L + Code Predictor 5L (memory-bandwidth bound):**
   - NOPRAY 1-vCPU (`15.46s`, `193.3 ms/frame`) beats official `1t FP32` (`46.08s`) by **`2.98×`**, `8t FP32` (`47.52s`) by **`3.07×`**, `1t BF16` (`61.12s`) by **`3.95×`**, and `8t BF16` (`24.68s`) by **`1.60×`**.
   - Reason: Batch=1 autoregressive decoding streams `~660 MiB` of linear weights per frame. `Q8_0S` split-plane AVX2+FMA cuts DRAM bandwidth by `3.56×` vs FP32 with zero Python/HuggingFace/dispatcher overhead and zero OpenMP barrier stalls (`8t FP32` is slightly slower than `1t FP32` on Talker!).
2. **Speech Tokenizer 12Hz V2 Decoder + BigVGAN Vocoder (compute-bound, ~385 GFLOPs):**
   - Official PyTorch `8t FP32` (`6.63s`) and `1t FP32` (`18.41s`) win this stage by dispatching Conv1d to Intel oneDNN / OpenBLAS multi-core AVX2 SGEMM.
   - Official PyTorch `BF16` (`386.85s` 8t / `400.75s` 1t) collapses on Zen 3 because Zen 3 lacks AVX-512 BF16 hardware, making NOPRAY **`15.02×–15.56×` faster** than official BF16!
   - Our single-core, 3.5 KiB hand-authored `2oc × 48t` AVX2+FMA+F16C tiled `conv1d`/`transconv1d` (`25.75s`) lands within **`1.40×`** of single-thread oneDNN FP32 SGEMM!
3. **Cold Startup & Virtualization Overhead:**
   - Inside the guest VM, boot from `0x100100` (32-bit Multiboot 1 → 4-level 2 MiB identity map → 64-bit Long Mode → AVX2/FMA/F16C `XCR0=7` → PIT TSC calibration → `-initrd` discovery) takes **< 20 ms**.
   - The `~11.8s` difference between `total_ms` (`41.22s`) and QEMU `WALL_MS` (`53.04s`) comes from QEMU's Multiboot option ROM copying the `1.042 GiB` `-initrd` image into guest RAM + host page faults. Replacing byte-polled COM2 (`0x2F8`, `614k` PIO exits = `~6.8s`) with batched `isa-debugcon` (`0xE9` `rep outsb`, 15 bytes of machine code) dropped total wall time from `59.81s` down to `53.04s`.
   - Meanwhile, official `qwen-tts` spends `8.8s–16.9s` just importing Python/PyTorch/Transformers and deserializing safetensors (`134,879` Linux syscalls measured via `strace -f -c` before generating a single token).
4. **Honest Caveats & Scope:**
   - `weights.nqts` (`1.042 GiB`, `DMCS-L3`) is pre-packed offline once from the official safetensors (`Q8_0S` Talker/CP, `F16` Decoder, pre-projected RVQ & text embeddings, folded layer-scale & ConvNeXt gamma).
   - Greedy argmax under `Q8_0S` produces `80` frames (`6.40s`) vs `84` (`6.72s`, FP32) and `81` (`6.48s`, BF16); all three speak the complete sentence naturally with slightly different pause timing.
   - Purity (`DMCS-1.1` tier `L3`) is an author attestation backed by strict static audit (`0 error(s), 0 warning(s)` across all 12 slots / 2,806 instructions; zero compiler, zero assembler, zero custom opcode-encoder DSL — only arithmetic displacement patching in `vm/patch_rel.py`).

---

## 4. Artifact Inventory

- Bare-metal ELF (`32,768 B`, `sha256 014256c393b5d6d8103bd665cd4a4c82c1d3c103284640070b8e5afc6fecf319`): `vm/build/nopray-qwen-tts.elf`
- Audited binary map & JSON report: `vm/build/MAP.md`, `vm/build/audit.json`
- Bare-metal synthesized Spanish WAV (`307,244 B`, `6.40s`, 24 kHz S16LE): `artifacts/nopray_es.wav`
- Official PyTorch greedy reference WAV (`322,604 B`, `6.72s`, 24 kHz S16LE): `artifacts/official_es_greedy.wav`
- Raw per-config benchmark JSONs: `benchmarks/official_{1t,8t}_{float32,bfloat16}.json`
