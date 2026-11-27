# Chapter 5 · Porting existing software

> A port preserves behaviour, not bytes. Decide how you will prove the behaviour is the same
> before you write the first byte, or you will end up proving whatever you happened to build.

## 5.1 Principle

Every port has an **original**, an **oracle** and an **equivalence criterion**:

- The original is pinned: version, file hashes, the exact routine or model.
- The oracle runs the original (or a faithful reference of it) on the same inputs.
- The criterion says what "same" means: identical bytes of output, a numeric tolerance
  (SNR, cosine, max error) for floating point, or an observable contract (protocol, UART text).

There are three routes, and real projects combine them:

| Route | From → to | How | Worked example |
|---|---|---|---|
| **A. Behaviour port** | an application, library or model (any language) → NOPRAY | Read the original, re-author its behaviour in literal hex, prove equivalence against the oracle | Qwen3-TTS (PyTorch → bare-metal x86-64); PHY gain routine (vendor object → Xtensa) |
| **B. Binary port** | an existing binary → mapped NOPRAY app | `propose`/`import` a byte-identical baseline (`origin = "imported"`), then re-author routine by routine (`reauthored` + `evidence`) | AgujaESP node (baseline + one slot edit) |
| **C. ISA port** | a NOPRAY app → another ISA | New app, same slot names, purposes and contracts; re-derive every encoding | `jstat` x86-64 → AArch64 in the benchmark |

Porting **to bare metal** is route A or B plus chapter 4: the original's OS services become the
obligation table.

## 5.2 Procedure

1. **Pin the original** and write down where it came from.
2. **Define the oracle and the criterion**, including the inputs (normal, edge, malformed).
3. **Inventory dependencies.** OS services → chapter 4 table. Libraries → only the parts the
   program actually uses (Qwen needed 14 numeric kernels, not a tensor library). Data formats →
   decide what is packed offline (Qwen's weights file has its own spec) and what is parsed at run
   time.
4. **Map it** (chapter 1): one slot per component, contracts from the original's interfaces.
5. **Author bottom-up**, testing each component against the oracle before composing them.
6. **Prove end to end**, then declare deviations instead of hiding them.
7. **Report the origin share** from the audit. A route-B baseline is never reported as a port.

Route-B commands: [WORKFLOW.md](../references/WORKFLOW.md) §3. Origins and evidence fields:
[MANIFEST.md](../references/MANIFEST.md) §1.

## 5.3 Traps

- **Comparing bytes across ISAs or compilers.** Compare behaviour.
- **Happy-path equivalence.** The oracle must see edge and malformed inputs too.
- **Floating point is not associative.** Vectorized sums change the last bits; set a tolerance
  and justify it, or reproduce the original order.
- **Quantization changes decisions.** A greedy argmax can pick a different token; declare it.
- **Calling conventions.** The PHY original uses the windowed ABI (`entry/retw.n`), the port
  CALL0 (`callx0/ret.n`): it cannot replace the original at a stock call site without an adapter.
- **Hidden inputs.** Locale, environment, time, uninitialized memory in the original.
- **Imported is not ported.** Bytes compiled by someone else stay `imported` until re-authored
  and proven.

## 5.4 Worked examples

**Route A at full scale: Qwen3-TTS** ([example](../examples/qwen3-tts-x86_64-baremetal/)).
Original: `Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice` with the official PyTorch package. Oracle: a
NumPy reference of the same algorithm ([`scripts/numpy_ref.py`](../examples/qwen3-tts-x86_64-baremetal/scripts/numpy_ref.py)).
Criterion and result: 89.32 dB SNR, cosine 0.9999999994, 153,359 of 153,600 PCM samples
identical, max error ±1 LSB. Declared deviation: with `Q8_0S` weights the greedy decoder emits
80 frames instead of the 84 of FP32 PyTorch. Data packed offline per
[`docs/WEIGHTS_SPEC.md`](../examples/qwen3-tts-x86_64-baremetal/docs/WEIGHTS_SPEC.md). Provenance:
every slot hand-authored (its `[provenance]`; the manifest predates the `origin` field).

**Route A, one routine: PHY gain-to-index** ([example](../examples/agujaesp-nopray-s3/ports/phy-txbbgain/)).
Original pinned by library, member and section hashes; behaviour derived into a five-row table;
52 bytes re-authored; oracle = seven vectors including high bits set on matching and
non-matching inputs; a deliberately wrong expected value must print `F`. Passed in Espressif
QEMU and on the board. `origin = "reauthored"`, `evidence = "README.md"`.

**Route B baseline: AgujaESP node** ([`node/`](../examples/agujaesp-nopray-s3/node/)). The La
Aguja flash set imported byte-identically, flashed and read back, then a 27-byte slot edit
installed over OTA. Wi-Fi, TLS and WireGuard are imported ESP-IDF bytes: a mapped, editable
baseline, not a hand-written network stack.

**Route C cost: `jstat`** ([BENCHMARKS.md](../references/BENCHMARKS.md) Part 2). Porting the
x86-64 validator to AArch64 took 361 s and 1.53 M tokens, where C, Rust and Go recompiled for
free. That is the honest price of route C today.

## 5.5 Problems

1. ★★ **Port a hash.** FNV-1a 64 (workload `f` of the runtime benchmark) from a C reference to
   x86-64 Linux NOPRAY. Accept: the C binary is the oracle; outputs are identical on 100 random
   inputs plus empty, 1-byte and multi-megabyte input.
2. ★★ **Then remove the OS.** Port problem 1 to bare metal (chapter 4) with the same oracle.
3. ★★★ **Route B end to end.** Build a tiny `-nostdlib -static` C program, `propose` + `import` it,
   then re-author its main loop slot. Accept: rebuild is byte-identical before the edit; after
   it, the audit's origin line shows a non-zero reauthored share, the slot's `evidence` compares
   outputs with the original binary, and nothing outside that slot changed (`diff`).
4. ★★★ **A kernel to another ISA.** Port Qwen's `rmsnorm` kernel to AArch64 (route C). Accept:
   a declared tolerance, and max relative error within it against NumPy on 1,000 random
   vectors run under `qemu-aarch64`.

## 5.6 State of knowledge

- Verified: route A on x86-64 bare metal (Qwen) and Xtensa (PHY); route B baseline and slot
  edit on the ESP32-S3; route C on AArch64 Linux (benchmark).
- Open: a tool command that runs original and port on the same inputs (`nopray equiv`); an
  obligation list generated from the audited syscall sites; an Xtensa decoder, without which
  ported ESP32 code is checked only by execution.
