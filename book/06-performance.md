# Chapter 6 · Making it fast

> Writing machine code by hand does not make a program fast. Knowing what bounds it, and
> choosing the instructions a compiler did not, does.

## 6.1 Principle

The runtime benchmark is the honest starting point ([BENCHMARKS.md](../references/BENCHMARKS.md)
Part 1): a straightforward hand-written Collatz loop (338 ms at 10⁶) **lost** to Rust (225 ms)
and C (284 ms). Speed comes from three questions, asked in order:

1. **What bounds this stage?** Branch prediction and latency, memory bandwidth, or arithmetic.
2. **Which machine-level structure removes that bound?** Branchless selection (`cmov`),
   alignment of hot loops, smaller data (quantization, split planes), SIMD lanes, tiling so the
   working set stays in cache.
3. **Did it work, by how much, and what did it cost?** Measured, with the losses reported too.

Because routines live in fixed slots, an optimization is a **slot edit**: same address, same
contract, a `diff` to review, the previous version as the oracle (chapter 5).

## 6.2 Procedure

1. Time each stage separately (Qwen logs `rdtsc` per stage over COM1, calibrated by the PIT).
2. Name the bound for the slowest stage before changing anything.
3. Copy the app to a new version, change one slot, keep its contract.
4. Prove equivalence against the previous version (bit-exact, or a declared tolerance).
5. Measure interleaved and randomized against the baseline, `n ≥ 10`, report the coefficient
   of variation, fix the CPU governor, and compare against the best compiler flags you can.
6. Record the gain in the slot's evidence and the method in this chapter if it is new.

## 6.3 Traps

- **One run is not a measurement.** Turbo, governor and cache state move results by tens of
  percent.
- **Straw-man baselines.** Compare with `-O2`/`-O3 -march=native` and `--release`, not debug builds.
- **Loop-carried dependencies do not vectorize.** FNV-1a feeds each byte's result into the
  next; SIMD across bytes is impossible, independent streams are not.
- **Alignment matters for hot loops**: the branchless Collatz loop was aligned to 16 bytes.
- **Numerics change with SIMD order** (chapter 5): declare the tolerance.
- **Fast on one CPU.** AVX2/FMA/F16C code needs the CPU check or a declared target; the Qwen
  VM enables and assumes them.

## 6.4 Worked examples

| Case | Bound | Technique | Result |
|---|---|---|---|
| `nopray-opt` Collatz ([BENCHMARKS.md](../references/BENCHMARKS.md) §1.3) | branch misprediction | `0240_collatz.hex`: 6-instruction branchless `shr` + `cmovb` inner loop, 21 bytes, 16-byte aligned | −35.8 % vs the branchy version; 210 ms vs Rust 214 ms and C 279 ms at 10⁶ |
| Qwen Talker + Code Predictor ([example](../examples/qwen3-tts-x86_64-baremetal/), BENCHMARKS Part 3) | memory bandwidth | split-plane `Q8_0S` weights with AVX2+FMA `gemv` (`1000_math_avx2.hex`) | 3.56× less weight traffic than FP32; 15.46 s vs 46.08 s (1-thread FP32 PyTorch) |
| Qwen decoder + BigVGAN vocoder (`5000_decoder_vocoder.hex`) | arithmetic (~385 GFLOPs) | `2oc × 48t` tiled Conv1d with AVX2+FMA+F16C, 3.5 KiB | **Lost** to 8-thread oneDNN FP32 (25.75 s vs 6.63 s) and to 1-thread (18.41 s, a 1.40× gap) |

The last row is in the book on purpose: a single hand-written core does not beat a tuned
multi-core GEMM library on compute-bound work. Knowing that is part of the method.

## 6.5 Problems

1. ★★ **Branchless.** Write Collatz peak steps (`c N` of the runtime benchmark) with a branchy
   inner loop, then a branchless one. Accept: identical outputs for every start value up to 10⁶;
   interleaved timings with CV; the speedup or the absence of one explained by the bound.
2. ★★ **Smaller data.** A sieve of Eratosthenes for 10⁷ with one byte per number, then one bit.
   Accept: same prime count; timings; an explanation in terms of cache footprint.
3. ★★★ **SIMD kernel.** An AVX2+FMA `f32` matrix-vector product. Accept: declared tolerance met
   against NumPy on random inputs; GFLOP/s reported against the CPU's peak.
4. ★ **Why not.** Explain in writing why FNV-1a cannot be vectorized across one input, then
   measure four independent inputs hashed in parallel lanes.

## 6.6 State of knowledge

- Verified: branchless and AVX2/FMA/F16C techniques on x86-64 (Ryzen 7 5825U, Zen 3), single core.
- Open: AArch64 NEON/SVE and RISC-V vector kernels; multi-core bare metal; cache-blocking
  methodology for compute-bound stages that could close the vocoder gap.
