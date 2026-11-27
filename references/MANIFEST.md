# `nopray.toml` and finding codes (`DMCS-1.1`)

## Contents

1. Manifest fields
2. Slot files and intents
3. Generated files
4. Finding codes

## 1. Manifest fields

Unknown keys are rejected. Addresses and sizes accept TOML hex (`0x400100`).

```toml
[app]
name = "hello-aarch64"
version = "1.0.0"
standard = "DMCS-1.1"
tier = "L2"                  # L0 | L1 | L2 | L3 (see SECURITY.md)
exposure = "local"           # local | lan | internet
description = "optional"

[target]
isa = "aarch64"              # x86-64 | aarch64 | riscv64 | riscv32 | thumb | xtensa | avr
os = "linux"                 # linux | bare-metal
format = "elf64"             # elf64 | elf32 | raw | data (non-executable: no code slots)
container = "esp-image"      # optional: audit recomputes ESP XOR + appended SHA-256 (E014-E016)
base = 0x400000              # vaddr of file offset 0 when no segment maps it (raw images)
entry = "_start"             # slot or label name
output = "hello-aarch64"     # file written to build/
file_size = 0x180            # optional; defaults to the end of the last slot
code_fill = "1f2003d5"       # optional; default = ISA NOP
data_fill = "00"             # optional
state_register = "r15"       # optional, with state_base
state_base = 0x404000

[[segment]]                  # one per PT_LOAD (ELF) or loaded region (raw)
name = "text"
vaddr = 0x400000
offset = 0x0
file_size = 0x180
mem_size = 0x180
perms = "r-x"                # r-- | r-x | rw- | rwx
align = 0x10000

[syscalls]                   # Linux: complete allow-list (names as in systemd)
allow = ["write", "exit"]

[[mmio]]                     # bare metal: every peripheral window the code may touch
name = "uart0"
base = 0x60000000
size = 0x100
note = "source: esp-idf soc/esp32c3/register/soc/uart_reg.h"

[[state]]                    # fields addressed through state_register
name = "client_fd"
vaddr = 0x40a004
size = 4

[provenance]
authored_by = "Angie (AI agent)"
method = "bytes written by hand from the ISA manual"
date = "2026-11-24"

[[slot]]
name = "_start"
kind = "code"                # header | code | rodata | data
bits = 64                    # optional on x86-64 code slots: 16 | 32 | 64 (e.g. bits = 32 for Multiboot 1 boot32)
vaddr = 0x400100
size = 0x40
file = "slots/0100_start.hex"
purpose = "write(1, msg, 18) then exit(0)"
origin = "authored"          # authored | imported | reauthored (audit totals the hand-written share)
evidence = "tests/start.md"  # required for reauthored slots: differential test vs the replaced routine
indirect = "optional justification for indirect jumps/calls"
dynamic_syscalls = "optional justification for unresolved syscall numbers"
self_modifying = "optional, only below tier L2"
[slot.contract]
inputs = ["rdi: fd"]
outputs = ["rax: result"]
clobbers = ["rax", "rcx", "r11"]
calls = ["write_all"]
noreturn = false
[[slot.label]]
name = "loop_head"
offset = 0x10
entry = true                 # other slots may branch here
note = "why this is an entry"
```

## 2. Slot files, symbolic fields and intents

- Bytes: hex digit pairs before the first `#` of each line. Whitespace is free.
- **Symbolic fields** `{kind @symbol}` stand for one address or displacement field computed
  from a manifest symbol (slot, label or `[[state]]` field; `@sym+N` / `@sym-N` add an offset).
  The opcode stays authored: byte kinds replace only the field bytes, word kinds take the whole
  authored word after `:` with the field bits at zero and fail if any field bit is set.
  Range, alignment and ISA are checked at build time; `annotate` keeps the token.

  | Kind | Field | `pc` default |
  |---|---|---|
  | `rel8` `rel16` `rel32` | signed `target - pc`, little-endian bytes | end of the line (next instruction) |
  | `abs8` `abs16` `abs32` `abs64` | unsigned address, little-endian bytes | - |
  | `a64.imm26` | `b`/`bl` word offset, bits 25:0 | the word |
  | `a64.imm19` | `b.cond`, `cbz`/`cbnz`, `ldr` literal, bits 23:5 | the word |
  | `a64.imm14` | `tbz`/`tbnz`, bits 18:5 | the word |
  | `a64.adr` / `a64.adrp` | byte / 4 KiB-page offset in immlo 30:29 + immhi 23:5 | the word |
  | `a64.add_lo12` | low 12 bits of the address in `add` imm12 (21:10) | - |
  | `rv.jal` / `rv.branch` | J-type ±1 MiB / B-type ±4 KiB | the word |
  | `rv.pcrel_hi20` | `auipc` upper part of `target - pc` (rounded for the low part) | the word |
  | `rv.pcrel_lo12_i` / `rv.pcrel_lo12_s` | I-/S-type low 12 bits of `target - pc` | the word; use `pc=-4` to pair with the preceding `auipc` |
  | `rv.hi20` / `rv.lo12_i` / `rv.lo12_s` | absolute `lui` + I/S low part | - |

  `pc=±N` (bytes from the field start) or `pc=@symbol` override the base. Examples:

  ```text
  e8 {rel32 @write_all}                       #= call write_all
  48 8d 35 {rel32 @msg}                       #= lea rsi, [rip+msg]
  {a64.imm26 @loop : 00 00 00 14}             #= b loop
  {a64.adr @msg : 01 00 00 10}                #= adr x1, msg
  {rv.pcrel_hi20 @msg : 97 05 00 00}          #= auipc a1, 0x0
  {rv.pcrel_lo12_i @msg pc=-4 : 93 85 05 00}  #= addi a1, a1, 60
  {abs64 @_start}                             #= e_entry 0x400100
  ```

  A wrong `pc=` or symbol still decodes to a real instruction, so keep the `#=` intent: `I001`
  catches it. Thumb, Xtensa and AVR fields are not supported yet.
- `#=` text after it is the **intent** of the item starting at the first byte of that line.
  `annotate` preserves intents and splits lines so every intent keeps its exact offset.
- Code slot intents are compared with the decoded instruction (numbers, spacing, `ptr`,
  `short`, `near`, `far`, `#`, slot/label/`[[state]]` symbol names, x86 jump opcode synonyms
  `jz`/`jnz`/`jc`/`jnc` ↔ `je`/`jne`/`jb`/`jae`, x86 string/PIO implicit `[rdi]`/`[rsi]`/`[edi]`/`[esi]`/`dx`
  operands such as `stosd`/`rep movsb`/`rep stosq`/`rep outsb`, and x87 implicit `st` in `fld`/`fstp` are
  normalised). Header slot intents must name the ELF field they sit on (`e_entry`, `p_align`, ...).
- For bare-metal x86-64 Multiboot 1 kernels (`isa = "x86-64"`, `os = "bare-metal"`, `format = "elf32"`),
  `e_machine = 3` (`EM_386`) is accepted in `E002`, and `bits = 32` decodes 32-bit boot code while tracing
  `jmp far 0x08:boot64` across the Long Mode transition into 64-bit code slots.
- An intent whose bytes disappeared is kept as `# ORPHAN INTENT ...` so the loss is visible.

## 3. Generated files (`build/`)

| File | Content |
|---|---|
| `<output>` | The binary (written with remove-then-create to avoid `ETXTBSY`) |
| `image.hex` | Plain hex of the whole image, `xxd -r -p image.hex` rebuilds it |
| `manifest.lock.json` | Tool version, image SHA-256, per-slot used size, SHA-256 and symbolic field count |
| `audit.json` | Every finding, per-slot report (origin, fields), syscall sites, origin totals |
| `MAP.md` | Navigation map: segments, slots, call graph, data refs, state ownership, syscall sites, open findings |

## 4. Finding codes

Errors fail `audit`; warnings fail `audit --strict`.

| Code | Severity | Meaning |
|---|---|---|
| R000 / R001 | info / error | Reference binary identical / different (size, bytes, first offset, owning slot) |
| E001-E004 | error | ELF unreadable, wrong class, wrong `e_machine`, `e_entry` != manifest entry |
| E003 | warning | `e_type` is not `ET_EXEC` |
| E005 | error | `PT_INTERP` or `PT_DYNAMIC` present |
| E006 | error | Entry point is not inside a code slot (not checked for `format = "data"`) |
| E014 | error | `container = "esp-image"` unreadable, truncated, or size differs from the image |
| E015 | error | ESP segment-data XOR byte differs from the XOR of the segment bytes |
| E016 | error | ESP appended SHA-256 differs from the hash of the carrier |
| E007 | warning | Unexpected program header type |
| E008 / E009 | error | `PT_LOAD` count or fields disagree with `[[segment]]` |
| E010 | error | `p_vaddr` and `p_offset` not congruent modulo `p_align` |
| E011 | error / warning | File shorter than `PT_LOAD` needs / trailing unmapped bytes |
| E012 | warning | Section header table present (typical of compiler/linker output) |
| E013 | warning | `PT_GNU_STACK` has non-zero size fields (misplaced field) |
| S001 | warning / error | `rwx` segment (error at tier L2+ or when exposed) |
| S002 | error | Exposed Linux program without `[syscalls]` |
| S003 | warning | No `PT_GNU_STACK` (stack executability left to defaults) |
| S004 | warning | AArch64 segment not 64 KiB aligned |
| S005 | error | Exposed program below tier L2 |
| S006 | error | `PT_GNU_STACK` requests an executable stack |
| S007 | warning | Allow-listed syscall unknown to the tool's table |
| K001 | error | Slot outside segments, code in non-executable or data in read-only segment |
| G001 | warning / error | File bytes owned by no slot (error if they are not fill bytes) |
| C001 | error | Undecodable bytes in a code slot |
| C002 | error | Instruction runs past the authored bytes |
| C003 | error | Branch into padding, data, or outside every slot |
| C004 | error | Branch into the middle of an instruction |
| C005 | error | Cross-slot branch to an undeclared entry |
| C006 | warning / error | Indirect branch without `indirect` justification (error when internet-exposed) |
| C007 | warning / error | Syscall number not statically resolvable (error with an allow-list unless justified) |
| C008 | error / warning | Syscall not in the allow-list, or unknown number |
| C009 | error / warning | Absolute access outside segments and MMIO windows (error on bare metal) |
| C010 | error | Write to read-only memory, or self-modifying write into code |
| C011 | warning | Registers written but not declared in the contract nor saved (x86-64; canonicalizes `eax`→`rax`, `r8d`→`r8`, `xmmN`/`ymmN`→`zmmN`) |
| C012 | warning | `contract.calls` disagrees with the calls in the bytes |
| C013 | info | Trap, breakpoint or privileged instruction |
| C015 | warning | Execution can fall through the slot end into padding and the next slot |
| C016 | info | Local call to an unlabelled address |
| C017 | warning | Address lands inside a labelled `rodata`/`data` slot without matching any declared label (catches off-by-N pointer errors) |
| D001 | info | Code slot with no caller, jump or reference |
| I001 | error | Decoded instruction differs from its `#=` intent. The message names the differing operand (numbers as a delta) and, when one exists, the single-bit edit of the authored bytes that decodes as the intent |
| I002 | error | Intent does not start an instruction / header field (byte boundaries shifted) |
| I003 | error | Header intent names a different ELF field |
| X001 | warning / error | No built-in decoder for the ISA (error when exposed) |
| A001 | warning | Slot comments are stale (`annotate` was not run after a byte change) |
| P001 | warning | `origin = "reauthored"` without `evidence` |
| P002 | warning | `evidence` path does not exist relative to the app directory |
| P003 | info | Some slots declare no `origin`, so the hand-written share is incomplete |

Build errors (not findings) cover malformed slot files: odd hex digits, unknown or unterminated
`{...}` fields, field bits already set in an authored word, out-of-range or misaligned fields,
unknown symbols and fields that do not apply to the target ISA.
