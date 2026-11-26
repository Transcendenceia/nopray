# NOPRAY Empirical Benchmarks (`DMCS-1.1` vs C, Rust, Go, Node.js & Python)

This document consolidates the completed **execution** (`72/72` cells) and **multi-model AI authoring / maintenance / cross-ISA porting** (`70/70` cells, `9,990/9,990` hidden tests, 12-run forensic purity audit) benchmarks completed on **2026-11-24 through 2026-11-26**.

No marketing numbers — every advantage **and** every real cost is reported verbatim.

---

## Part 1 — Runtime Execution Benchmark (`72/72` Correct, 0 OOMs, 0 Timeouts)

**Implementations compared** on the exact same specification (`h` hello/exit, `c N` Collatz peak steps, `s N` Sieve of Eratosthenes, `f` FNV-1a 64-bit stdin hash):
- **NOPRAY (`DMCS-L2` W^X):** `1,216 B` ELF64 x86-64, 4 syscalls (`read`, `write`, `mmap`, `exit`), strict audit `0 errors, 0 warnings`.
- **C:** `gcc 14.2 -O2 -static` (`660.4 KiB`).
- **Rust:** `rustc 1.100.0 --release` static (`1.1 MiB`).
- **Go:** `go 1.25.6`, `CGO_ENABLED=0` (`1.4 MiB`).
- **Node.js:** `v24.22.1` (`1.9 KiB` script + `123.6 MiB` runtime).
- **Python:** `3.14.6` (`1.9 KiB` script + `75.7 MiB` runtime).

**Host & harness:** Linux x86-64 (AMD Ryzen 7 5825U, 2.0 GHz cap), measured via a minimal zero-libc `fork` + `execve` + `wait4` spawner plus live `/proc/<pid>/smaps_rollup` inspection.

### 1.1 Footprint, Live Memory (`smaps_rollup`) & Syscall Surface

| Implementation | Binary | Total Deployed | Syscalls `h` (total / distinct) | Syscalls `f` 32 MiB | Live RSS (`f`) | Live PSS (`f`) | Private Anon (`f`) |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **NOPRAY** | **`1.2 KiB`** | **`1.2 KiB`** | **`2` / `2`** | **`515` / `3`** | **`80.0 KiB`** | **`80.0 KiB`** | **`76.0 KiB`** |
| C (`-O2 -static`) | `660.4 KiB` | `660.4 KiB` | `17` / `13` | `530` / `14` | `736.0 KiB` | `728.0 KiB` | `116.0 KiB` |
| Rust (`--release`) | `1.1 MiB` | `1.1 MiB` | `32` / `21` | `545` / `21` | `1.1 MiB` | `1.1 MiB` | `148.0 KiB` |
| Go (`static`) | `1.4 MiB` | `1.4 MiB` | `248` / `22` | `969` / `23` | `2.1 MiB` | `2.1 MiB` | `676.0 KiB` |
| Python `3.14` | `1.9 KiB` | `75.7 MiB` | `730` / `34` | `1,243` / `34` | `10.9 MiB` | `8.9 MiB` | `4.7 MiB` |
| Node.js `v24` | `1.9 KiB` | `123.6 MiB` | `1,319` / `44` | `1,935` / `44` | `64.4 MiB` | `22.7 MiB` | `16.0 MiB` |

### 1.2 Performance Across 3 Load Tiers

| Tier & Workload | NOPRAY (base) | C (`-O2`) | Rust (`--release`) | Go | Node.js | Python |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| **Moderate · Startup `h`** (`n=300`) | **`330 µs`** | `642 µs` | `790 µs` | `2.5 ms` | `100.5 ms` | `43.4 ms` |
| **Moderate · Collatz `10⁶`** | `338.0 ms` | `283.5 ms` | **`224.6 ms`** | `340.3 ms` | `3.11 s` | `16.32 s` |
| **Moderate · Sieve `10⁷`** | **`38.6 ms`** | `39.4 ms` | `40.2 ms` | `55.9 ms` | `176.2 ms` | `2.62 s` |
| **Moderate · FNV-1a `32 MiB`** | `80.7 ms` | **`76.4 ms`** | `76.5 ms` | `78.8 ms` | `190.1 ms` | `7.75 s` |
| **Limited (`50%` CPU, `64M` RAM) · Startup `h`** | **`332 µs`** | `643 µs` | `791 µs` | `2.5 ms` | `288.1 ms` | `97.1 ms` |
| **Heavy · Startup `h`** (`n=1000`) | **`333 µs`** | `652 µs` | `809 µs` | `2.6 ms` | `108.1 ms` | `46.4 ms` |
| **Heavy · Collatz `10⁷`** | `3.69 s` | `3.04 s` | **`2.51 s`** | `3.77 s` | `37.82 s` | `201.08 s` |
| **Heavy · Sieve `3×10⁸`** (`286 MiB`) | **`3.11 s`** | **`3.11 s`** | `3.13 s` | `3.47 s` | `3.90 s` | `109.27 s` |
| **Heavy · FNV-1a `512 MiB`** | `1.22 s` | **`1.20 s`** | **`1.20 s`** | `1.21 s` | `1.33 s` | `115.95 s` |

### 1.3 Branchless Slot Optimization (`nopray-opt` on Collatz)

Replacing the branchy parity check in `0240_collatz.hex` with a 16-byte-aligned 6-instruction branchless `shr + cmovb` inner loop (`21 B`) yielded an interleaved, randomized-order comparison:

| Load Tier | `nopray-opt` | Rust (`--release`) | C (`-O2`) | `nopray` (branchy) | Gain vs Base |
| :--- | ---: | ---: | ---: | ---: | ---: |
| **Moderate** (`c 10⁶`, `n=10`) | **`209.93 ms`** (`CV 0.15%`) | `213.76 ms` | `278.54 ms` | `327.10 ms` | **`−35.8%`** (`0.754× C`) |
| **Limited** (`c 10⁶`, `50%` CPU) | **`460.62 ms`** | `483.36 ms` | `530.89 ms` | `687.07 ms` | **`−33.0%`** (`0.868× C`) |
| **Heavy** (`c 10⁷`, `n=5`) | **`2.456 s`** (`CV 0.09%`) | `2.535 s` | `3.039 s` | `3.688 s` | **`−33.4%`** (`0.808× C`) |

*Honest caveat:* compilers apply optimizations like `cmov`, unrolling, and SIMD automatically across hundreds of functions. In NOPRAY, every micro-optimization is hand-crafted and tied to one ISA.

---

## Part 2 — AI Authoring, Maintenance & Cross-ISA Porting (`jstat` Strict JSON Validator)

Task: build a strict RFC 8259 JSON validator with exact byte-offset error reporting (`Phase A`), hand it to a fresh agent with zero context to extend it to v2 (`Phase B`), and port the x86-64 binary to Linux AArch64 (`Phase C`).

- **Coverage:** **70 graded cells** across two model families (`n = 3` primary track = 48 cells + multi-model replication = 22 cells), **`9,990 / 9,990` hidden tests passed (100%, 0 escaped defects)** against an oracle cross-checked on `10,313` cases catching `18/18` planted mutants.

### 2.1 Primary Track (`n = 3` mean and min–max range)

| Language | Phase A Wall / First Green | Phase B Wall / First Green | Total A+B Wall | Total A+B Tokens | A+B Cost Ratio vs C | Port to AArch64 |
| :--- | :---: | :---: | ---: | ---: | :---: | :--- |
| **Go** | `128 s` / `71 s` | `138 s` / `53 s` | `266 s` | `1.23M` | `0.98×` / `0.85×` | `0 s` (recompile) |
| **C** | `124 s` / `71 s` | `148 s` / `52 s` | `272 s` | `1.45M` | `1.00×` / `1.00×` | `0 s` (recompile) |
| **JavaScript** | `155 s` / `84 s` | `145 s` / `59 s` | `301 s` | `1.32M` | `1.11×` / `0.91×` | `0 s` (portable) |
| **Rust** | `182 s` / `112 s` | `118 s` / `86 s` | `301 s` | `2.24M` | `1.11×` / `1.54×` | `0 s` (recompile) |
| **Python** | `272 s` / `83 s` | `213 s` / `64 s` | `485 s` | `2.17M` | `1.78×` / `1.50×` | `0 s` (portable) |
| **NOPRAY** | `348 s` / `251 s` | `208 s` / `120 s` | **`555 s`** | **`2.90M`** | **`2.05×` / `2.02×`** | **`361 s` + `1.53M` tok** |

### 2.2 Key Forensic & Tooling Discoveries Folded Into `DMCS-1.1`

1. **Agents fail at bit-field encoding, not logic:** on AArch64 porting, agents repeatedly slipped on bit 8 (`Rn[9:5]` crossing the byte boundary in `subs`/`cmp imm12`), caught immediately by `#=` intents (`I001`). On x86-64 (`nopray-doom`), `#=` intents caught **5 real `REX.R/X/B` SIB bit typos** before execution.
2. **Closing the `I001` blind spot (`WARN C017`):** in `nopray-jb-r3`, an agent miscalculated a `rodata` string address by 2 bytes in both bytes and `#=` intent (`mov esi, 0x400e4a` instead of `0x400e4c`). `I001` could not see it because both agreed on the wrong address. We added **`WARN C017`** to `tools/nopray/src/audit.rs`: whenever a `rodata`/`data` slot declares `[[slot.label]]` entries, any code address landing inside it without matching a declared label is flagged with the exact byte delta to the nearest label.
3. **Banning custom bit-field encoder DSLs:** forensic audit of 12 NOPRAY runs identified agents occasionally writing mini-assemblers in Python (`def movz/add_imm(...)`). `SKILL.md` and `WORKFLOW.md` now explicitly forbid instruction-synthesizing DSLs: helper scripts may resolve `@label` displacements and declared addresses only.
4. **Live PipeWire/PulseAudio PCM substrate verification:** building `nopray-doom-x86_64-cachyos` proved direct 24 kHz S16LE real-time PCM synthesis over `/run/user/<uid>/pulse/native` using the exact same 6 syscalls as X11 and zero libraries — promoting the audio substrate from `UNVERIFIED` to **VERIFIED** in [`GRAPHICS-SUBSTRATES.md`](./GRAPHICS-SUBSTRATES.md).
