---
name: nopray
description: 'NOPRAY / DMCS-1.1: author, port, improve, modify and audit native programs written directly as machine-code bytes (hex slots + executable headers) with no compiler, assembler, linker or SDK, using the `nopray` tool to lay out, decode, map and audit every byte. Use only when the user explicitly asks for NOPRAY, raw/hand-written machine code, hex-authored binaries, or "no programming language / no compiler" software; when porting, improving or optimizing an existing binary as a mapped NOPRAY app; when modifying or auditing an existing NOPRAY app; or when targeting bare metal (ESP32-C3/C6/S3, RP2040, Raspberry Pi) this way. Ordinary requests for native programs, CLIs or services follow First Rust instead.'
metadata:
  icon: ./assets/icon.svg
---

# NOPRAY: talking to the machine without intermediaries (`DMCS-1.1`)

**Goal of this skill: you learn to program the machine directly.** You write the bytes the
CPU executes, with no compiler, assembler, linker or SDK between your intent and the silicon.
You may build a new program, port an existing one, make it faster, change it later, or take
it to bare metal. The examples in this repository (calculator, raycaster, DOOM, an HTTP
server, bare-metal Qwen3-TTS, an ESP32-S3 node) are *uses* of the method, not the method
itself. The method is what lets you solve the next problem, which none of them covers.

The skill is organised like a textbook. Each chapter of [`book/`](book/) gives a principle,
a procedure, the traps that cost time before, worked examples with real evidence, problems
with acceptance checks, and the current state of knowledge. Study the chapters your task
needs, use the examples as solved exercises, and then solve your own.

The `nopray` tool never chooses or encodes an instruction. It:

- copies authored bytes into declared slots
- fills only declared address and displacement fields from manifest symbols
- decodes everything back and checks it against the map and the security policy
- regenerates the map an agent needs for the next change

Run it through the launcher, which builds itself on first use and keeps its cache outside the
skill:

```bash
T=./tools/nopray.sh
$T audit examples/hello-aarch64-linux        # paths relative to the repo root
```

## How to study for a task

Always read chapters 1 and 2. Then read the row for your task.

| Your task | Read | Solved exercises to imitate |
|---|---|---|
| A new program on Linux | [3](book/03-talking-to-an-os.md) | hello-aarch64, hello-riscv64, calc, httpd v2 |
| A new program with no OS (VM, board) | [4](book/04-no-os.md), [9](book/09-hardware-and-evidence.md) | Qwen3-TTS bare metal, AgujaESP probe |
| Port existing software (behaviour, binary or ISA) | [5](book/05-porting.md), plus 3 or 4 for the destination | Qwen (behaviour), AgujaESP PHY and node, jstat to AArch64 |
| Make it faster | [6](book/06-performance.md) | `nopray-opt`, Qwen kernels (and the vocoder loss) |
| Change a working program | [7](book/07-changing-a-program.md) | AgujaESP OTA edit, jstat Phase B |
| Expose it (LAN, internet, a tailnet) | [8](book/08-exposure.md) | httpd v2 behind Caddy, AgujaESP node |
| Write to real hardware | [9](book/09-hardware-and-evidence.md) | AgujaESP, rung by rung |

## The book

| Chapter | Teaches |
|---|---|
| [1 · The map](book/01-the-map.md) | Fixed slots, the manifest, contracts, labels, state: every byte has an owner |
| [2 · Encoding without mistakes](book/02-encoding.md) | The three classes of encoding slip, and the check that catches each one |
| [3 · Talking to an OS](book/03-talking-to-an-os.md) | Syscalls and wire protocols (X11, PulseAudio, HTTP) without libraries |
| [4 · Without an OS](book/04-no-os.md) | The obligations an OS used to meet: boot, mode, memory, I/O, time, exit |
| [5 · Porting](book/05-porting.md) | Original, oracle, criterion. Routes A (behaviour), B (import then re-author) and C (ISA) |
| [6 · Making it fast](book/06-performance.md) | Find the bound, choose the structure, measure honestly |
| [7 · Changing a program](book/07-changing-a-program.md) | Local edits, derived bytes, callers re-tested, hand-offs |
| [8 · Exposure](book/08-exposure.md) | Bounded parsers inside, containment outside, gates before publishing |
| [9 · Hardware and evidence](book/09-hardware-and-evidence.md) | The fidelity ladder, owner approval, what counts as proof |
| [Editions](book/EDITIONS.md) | How the book grows after every job, and the open questions |

## The commandments

1. **Purity is an attestation, not a proof, and encoders are forbidden.** Never
   compile/assemble and paste bytes while claiming a DMCS tier, and never write a helper DSL
   (e.g. `def movz/add_imm/cmp_imm(...)`) that builds instructions from mnemonics or register
   arguments. Symbolic fields are the only sanctioned arithmetic: they fill a declared
   displacement or address field inside an opcode you wrote, and the build rejects an authored
   word whose field bits are not zero. Per-app relocation scripts are no longer needed. The
   audit proves what the bytes do, never how they were produced; say so whenever purity is
   claimed. Record `[provenance]` in the manifest.
2. **Every byte has an owner, and every referenced data item has a label.** Every file byte
   belongs to a declared slot (`G001` otherwise). For `rodata`/`data` slots holding multiple
   strings or tables, declare `[[slot.label]]` for each item and reference it with a field
   (`{rel32 @err_msg}`); `C017` catches literal off-by-N pointers.
3. **Declare intent for every instruction you author.** End each byte line with
   `#= <instruction as you meant it>` (header fields: `#= <field name> <value>`). A mismatch
   is the error `I001`; a wrong field symbol or `pc=` still decodes, so the intent is what
   catches it.
4. **Say how much is yours.** Declare `origin` on every slot (`authored`, `imported`,
   `reauthored`). `import` stamps `imported`; a re-written routine is `reauthored` with
   `evidence` of equivalence (`P001` until then). Report the audit's hand-written share.
   An imported baseline is never a port.
5. **Cross-slot control flow only through declared entries.** Calls and jumps into another slot
   must target its start or a label with `entry = true` (`C005`). Fall-through into the next
   slot is reported (`C015`); end every slot with an explicit jump, return or exit.
6. **Exposure gates.** `exposure = "lan"` or `"internet"` requires tier L2 (W^X), a
   `[syscalls]` allow-list, zero audit errors and a seccomp filter generated by
   `nopray seccomp`. Publishing anything to the internet additionally requires explicit operator
   approval and reverse-proxy / sandbox containment (see [SECURITY.md](references/SECURITY.md)).
7. **Changes are reviewed by slot.** After any edit: `annotate`, `audit`, `diff` against the
   previous version, re-test the callers the diff lists, then run the program.
8. **Hardware is not a sandbox.** Flashing a microcontroller or writing a boot medium needs
   explicit approval, a known recovery path (ROM download mode, BOOTSEL, spare SD) and, when
   possible, a backup of the current firmware. Emulate first.
9. **Unverified target facts stay marked.** Register addresses, boot headers and keys come from
   primary sources; anything else is labelled UNVERIFIED in the manifest notes and the report.

## The agent loop

Measured failure mode: agents reason correctly and slip when **encoding** (a register bit, a
displacement, a data address). The loop below moves each of those to a tool check.

1. **Orient on one slot:** `$T context APP SLOT` (or `--json`, `--listing`): free bytes,
   contract, callers to re-test, state fields, symbolic fields, neighbours. Do not load the
   whole program to change one routine.
2. **Write the hex** one instruction per line with a `#=` intent. Every branch, call and
   address load uses a **symbolic field**, never a hand-computed number:
   `e8 {rel32 @write_all}`, `{a64.imm26 @loop : 00 00 00 14}`, `{abs64 @msg}`
   ([MANIFEST.md](references/MANIFEST.md) §2 lists the kinds).
3. `$T annotate APP` and read the decoded listing.
4. `$T audit APP --strict`. On `I001`, read the hint first: it names the differing operand
   and, when one exists, the single flipped bit that decodes as your intent. Fix the byte.
5. Run it (natively, `qemu-<arch>`, emulator), exercise every path and malformed input.
6. `$T diff OLD NEW`, re-test the callers it lists, deliver the evidence.

## Procedures at a glance

Read [references/WORKFLOW.md](references/WORKFLOW.md) before the first app of a session.

| Task | Steps |
|---|---|
| **Create** | Fingerprint target ([ENV-DISCOVERY](references/ENV-DISCOVERY.md)) → write `nopray.toml` (segments, slots with `origin = "authored"`, contracts, syscalls/MMIO, state, labels) → agent loop per slot → capture evidence |
| **Port** ([chapter 5](book/05-porting.md)) | Name the original, the oracle and the equivalence criterion first. Route A (behaviour): new map, the original is the oracle. Route B (binary): `propose --binary` (ELF) / `propose-esp`, or map a vendor carrier manually → `import` (byte-identical, slots stamped `imported`) → per routine: `context`, re-author with fields + intents, `origin = "reauthored"` + differential `evidence`. Route C (ISA): same map and contracts, every slot re-encoded. Report the hand-written share |
| **Improve** a port or app | Same slot, same contract: shorter or faster bytes (branchless loops, fewer syscalls), equivalence evidence, `diff`, measured gain |
| **Modify** | Copy the app to a new version dir → `context` → edit bytes in one slot (keep its address and size) → `annotate` → `audit` → `diff old new` → re-test listed callers → run |
| **Deploy** (Linux) | Tier L2, exposure declared, `audit --strict` clean → `seccomp` → systemd sandbox ([SECURITY](references/SECURITY.md)) → verify every route/feature live → keep rollback |

## This book is alive

NOPRAY is young, and the book is written along the way. Before closing any NOPRAY job, follow
[EDITIONS.md](book/EDITIONS.md). Each lesson goes into the chapter that owns the step:

- a new trap → **Traps**
- a verified case → **Worked examples**
- a new verified fact → **State of knowledge**
- a good exercise → **Problems**

Write the general rule, not only the incident. An unverified claim stays out or is marked
UNVERIFIED.

## Appendix A · Tool commands

| Command | What it does |
|---|---|
| `build DIR` | Resolves symbolic fields, lays slots out into `build/<output>`, `build/image.hex` (`xxd -r -p` compatible) and `build/manifest.lock.json` (per-slot SHA-256). |
| `audit DIR [--binary F] [--strict]` | All checks; writes `build/audit.json` and `build/MAP.md`, prints the origin totals. `--binary` proves a deployed file equals the map. Exit 1 on errors (or warnings with `--strict`). |
| `annotate DIR [--check]` | Regenerates comments from the bytes (disassembly, ELF field names, syscall names, `→ symbol`); keeps `#=` intents and `{...}` fields. |
| `context DIR SYMBOL [--json] [--listing]` | One slot (or the slot owning a label): range, free bytes, purpose, origin, contract, labels, callers to re-test, state touched, fields in/out, neighbours, findings. |
| `propose --binary F --name N [--isa I] [--code-end HEX]` | Drafts a manifest for an ELF: segments, header, functions split at calls and NOP padding, cross-slot labels, `origin = "imported"`. |
| `propose-esp --binary F --name N --version V --authored-by S --date D` | Drafts a container-granularity manifest for an ESP32-S3 loader image (header, descriptors, segments, XOR, digest; `container = "esp-image"`). Refuses other chip IDs. |
| `import --binary F --manifest M --out DIR` | Extracts annotated slot files, stamps missing origins as `imported`, and fails unless the rebuild is byte-identical. |
| `diff OLD NEW` | Changed/moved slots, callers to re-test, unified diff of the decoded listings. |
| `seccomp DIR` | systemd `SystemCallFilter=` from the audited syscall sites (fails if any site is unresolved). |

Finding codes, field kinds and the manifest format: [references/MANIFEST.md](references/MANIFEST.md).

## Appendix B · Decoder coverage

| ISA | Built-in decoding | Fields | Status of examples |
|---|---|---|---|
| x86-64 | Full (16/32/64-bit `slot.bits`, AVX2/FMA/F16C, far-jump CFG, Multiboot 1 ELF32 bare metal, syscalls, contracts, `C017`) | `rel*`, `abs*` | calc, nopray3d, nopray-doom, nopray-httpd v2, qwen3-tts-baremetal: byte-identical maps, 0 errors |
| AArch64 | Validity + flow + `svc`/`x8` syscalls + `adr`/literal refs | `a64.*`, `abs*` | hello-aarch64: runs under `qemu-aarch64`, strict audit clean |
| RISC-V 32/64 (+C) | Lengths, major-opcode validity, flow, `ecall`/`a7`; text via objdump | `rv.*`, `abs*` | hello-riscv64: runs under `qemu-riscv64`, strict audit clean |
| Xtensa (ESP32-S3) | No built-in decoder (`X001`); `container = "esp-image"` makes `audit` recompute the XOR and SHA-256 (`E015`/`E016`) | `abs*` | AgujaESP-NOPRAY probe, PHY port, live tailnet node from an imported flash set |
| Thumb, AVR | None: bytes are opaque (`X001`), objdump text for annotation only | `abs*` | Needs external evidence |

Without a decoder, intents and `I001` hints cannot check code: treat such slots as needing
external evidence. Microcontroller boot, flash-set mapping and port procedure:
[TARGETS.md](references/TARGETS.md) §6.

## Appendix C · Reference library (load on demand)

| Document | Use it for |
|---|---|
| [WORKFLOW.md](references/WORKFLOW.md) | Step-by-step create / port / improve / modify, slot sizing, labels, contracts, intents, fields |
| [MANIFEST.md](references/MANIFEST.md) | `nopray.toml` fields, slot file format, symbolic field kinds, every finding code |
| [SECURITY.md](references/SECURITY.md) | Threat model, what the audit proves and does not, exposure gates, deployment and hardware checklists |
| [TARGETS.md](references/TARGETS.md) | ISA/OS/boot status matrix: Linux x86-64/AArch64/RISC-V, ESP32-C3/C6/S3, RP2040/RP2350, Raspberry Pi bare metal |
| [ENV-DISCOVERY.md](references/ENV-DISCOVERY.md) | Fingerprinting the execution host |
| [FIXED-SLOT-ABI.md](references/FIXED-SLOT-ABI.md) | Slot layout and state-register discipline |
| [ELF64-X86_64-SPEC.md](references/ELF64-X86_64-SPEC.md) | ELF templates, x86-64 syscalls/opcodes, encoding traps |
| [GRAPHICS-SUBSTRATES.md](references/GRAPHICS-SUBSTRATES.md) | X11, verified PipeWire/PulseAudio PCM audio, Wayland, framebuffer, TTY, TCP wire protocols |
| [HUMAN-AUDIT-PROTOCOL.md](references/HUMAN-AUDIT-PROTOCOL.md) | Independent verification by a human or another agent |
| [MANUAL.md](references/MANUAL.md) | Background, verified milestones with their metrics, honest scope and limits (Spanish) |
| [BENCHMARKS.md](references/BENCHMARKS.md) | Empirical runtime and multi-model authoring benchmarks vs C, Rust, Go, JS and Python |
| [BENCHMARK-PLAN.md](references/BENCHMARK-PLAN.md) | Original benchmark methodology plan |

## Appendix D · Examples (the solved exercises)

Each example directory is an app: `nopray.toml`, annotated `slots/`, and generated `build/`
(`MAP.md`, `audit.json`, `manifest.lock.json`, binary). `tools/nopray/tests/examples.rs`
rebuilds all of them and fails if any byte or audit result changes; it also rewrites
DOOM, hello-aarch64 and hello-riscv64 with symbolic fields and proves the bytes stay identical.

- **Start here:** `examples/hello-aarch64-linux/`, `examples/hello-riscv64-linux/` (minimal, fully intented).
- `examples/calc-x86_64-cachyos/`: 4 KiB X11 calculator (tier L1, RWX by design, local only).
- `examples/nopray3d-x86_64-cachyos/`: 8 KiB W^X raycaster.
- `examples/nopray-doom-x86_64-cachyos/`: 32 KiB W^X FPS with X11 and native PipeWire/PulseAudio PCM.
- `examples/qwen3-tts-x86_64-baremetal/`: 32 KiB bare-metal VM running Qwen3-TTS-0.6B on `/dev/kvm`.
- `examples/nopray-httpd-x86_64-linux/v2/`: 40 KiB W^X SSR HTTP/1.1 server with a 13-syscall seccomp profile.
- `examples/agujaesp-nopray-s3/`: ESP32-S3 160-byte probe, the PHY routine port (`ports/`, reauthored + evidence) and the live node (`node/`, imported flash set).

Sizes, instruction counts and benchmark figures live in [MANUAL.md](references/MANUAL.md).
