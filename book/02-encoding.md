# Chapter 2 · Encoding without mistakes

> Measured across 70 benchmark cells: agents reason correctly and slip when they turn an
> instruction into bits. The method is built so that every slip is caught by a check, not by
> a crash.

## 2.1 Principle

Three kinds of encoding error account for almost everything observed so far:

1. **A wrong bit inside a field**: a register number (`w9` written as `w1`), a REX bit.
2. **A wrong number**: a branch displacement or a data address computed by hand.
3. **A wrong boundary**: the bytes of one instruction run into the next.

The defences are layered, and each covers what the previous one cannot see:

| Defence | Catches | Blind to |
|---|---|---|
| `#=` intent on every line, compared with the decoder (`I001`, `I002`) | wrong bits, wrong boundaries | a wrong number written the same way in bytes and intent |
| Symbolic fields `{rel32 @f}`, `{a64.adr @msg : …}` | hand-computed displacements and addresses (they are no longer typed) | the wrong symbol or `pc=` (the intent still catches it) |
| Labels on every data item (`C017`) | literal pointers into the middle of a data slot | pointers to the wrong item that happen to hit a label |
| Contracts (`C011`, `C012`) | clobbered registers, undeclared calls | semantics |
| Running it (chapter 3/4) | everything that executes | paths not exercised |

The tool never encodes: it decodes your bytes, fills only the fields you declared, and
compares. When `I001` fires it names the operand that differs and, when one exists, the
single flipped bit of your bytes that decodes as your intent. That bit is a lead, not an
answer: confirm it against the ISA manual, then edit the byte.

## 2.2 Procedure

1. One instruction per line, intent in the decoder's spelling (run `annotate` once and copy its
   style). Symbols are accepted: `#= call write_all`.
2. Every branch, call and address load is a field. Kinds: [MANIFEST.md](../references/MANIFEST.md) §2.
   Compute by hand only where no kind exists (Thumb, Xtensa, AVR today) and say so in the
   provenance notes.
3. `annotate`, read the listing, `audit --strict`. Fix `I001` before anything else.
4. Never write a helper that builds instructions from mnemonics or register arguments
   (`def add_imm(...)`). That is an assembler; the tier claim becomes false.

## 2.3 Traps by ISA

- **x86-64**: signed `imm8`/`disp8` (≥ 0x80 is negative); `rsp`/`r12` as base need a SIB byte;
  `mod=00 rm=101` means RIP+disp32, so `[rbp]`/`[r13]` need `mod=01` with a zero `disp8`; **REX.R/X/B** select the high registers in
  ModRM/SIB (`45 8d 1c 2e` is `[r14+rbp]`, `[r14+r13]` needs `47`); `div` raises `#DE` on
  overflow; RIP-relative displacements count from the end of the whole instruction, which
  is why `rel32` defaults to the end of the line. Checklist:
  [ELF64-X86_64-SPEC.md](../references/ELF64-X86_64-SPEC.md) §4.
- **AArch64**: branch offsets count words from the branch itself; `adr` reaches ±1 MiB, beyond
  that `adrp` + `add_lo12`; `movz/movk` build constants 16 bits at a time; **bit 8 of
  `subs`/`cmp` immediate** is the top bit of `Rn` and crosses a byte boundary (the most
  frequent real error).
- **RISC-V**: `auipc`+`addi` with a sign-extended low part (`rv.pcrel_hi20` +
  `rv.pcrel_lo12_i pc=-4` handle the rounding); declare whether compressed instructions are
  used; there are no field kinds for C forms.
- **Any ISA**: an instruction text from an external disassembler (`objdump`) is annotation,
  not audit evidence. Where the tool has no decoder (`X001`), intents are not checked at all.

| ISA | Decoder in the tool | Field kinds |
|---|---|---|
| x86-64 | full (16/32/64-bit, AVX2/FMA/F16C, syscalls, contracts) | `rel*`, `abs*` |
| AArch64 | validity, flow, `svc`, `adr`/literal refs | `a64.*`, `abs*` |
| RISC-V 32/64 (+C) | lengths, validity, flow, `ecall`; text via objdump | `rv.*`, `abs*` |
| Xtensa, Thumb, AVR | none (`X001`) | `abs*` |

## 2.4 Worked examples

| Case | What happened | What it teaches |
|---|---|---|
| DOOM, x86-64 ([BENCHMARKS.md](../references/BENCHMARKS.md) Part 2.2) | Intents caught **5 real REX.R/X/B SIB typos** before the first run | Write the intent even for "obvious" lines; REX errors decode to valid, wrong instructions |
| `jstat` port to AArch64 (same) | Repeated `cmp w9` → `w1` slips (bit 8), each caught by `I001` | The hint `… = instruction bit 8` points straight at it now |
| `nopray-jb-r3` (same) | `mov esi, 0x400e4a` instead of `0x400e4c`, in bytes **and** intent: the first defect that reached the tests | Why fields and data labels exist: a wrong number typed twice is still wrong |
| [`tools/nopray/tests/examples.rs`](../tools/nopray/tests/examples.rs) `symbolic_fields_rebuild_byte_identical` | Every `call` of DOOM, `adr`/`e_entry` of hello-aarch64 and `auipc`/`addi` of hello-riscv64 rewritten as fields rebuild to the same SHA-256 | Fields are exactly the arithmetic you would have done by hand |

## 2.5 Problems

1. ★ **Read the hint.** In a copy of `hello-aarch64-linux`, change the intent of the `adr`
   line to `adr x9, msg`. Accept: you can state, from the `I001` message alone, which byte and
   bit to change and why that field is `Rd`; then restore it.
2. ★★ **Remove every typed number.** In a copy of `calc-x86_64-cachyos`, turn every direct
   `call`/`jmp`/`jcc` and every data address into a field. Accept: `build` prints the locked
   SHA-256 of the original, `audit --strict` has no new findings.
3. ★★ **Replay jb-r3.** Write a two-string program where the second string's address is
   typed by hand two bytes off, in bytes and intent. Accept: show that `I001` stays silent,
   that `C017` reports it once labels exist, and that a field makes the error impossible.
4. ★★★ **New ISA rule.** Pick one instruction family not covered above (e.g. AArch64
   `ldr` scaled offsets, RISC-V stores) and write its trap for §2.3 from the manual, with one
   test that demonstrates it. Accept: the trap states the general rule, not the incident.

## 2.6 State of knowledge

- Verified: the error classes and defences above, from the 70-cell benchmark and the examples.
- Open: field kinds and decoders for Thumb (RP2040) and Xtensa (ESP32/S3); `ldr`/`str`
  scaled `lo12` kinds on AArch64; compressed RISC-V branch kinds.
