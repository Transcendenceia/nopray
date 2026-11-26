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
format = "elf64"             # elf64 | elf32 | raw
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

## 2. Slot files and intents

- Bytes: hex digit pairs before the first `#` of each line. Whitespace is free.
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
| `manifest.lock.json` | Tool version, image SHA-256, per-slot used size and SHA-256 |
| `audit.json` | Every finding, per-slot report, syscall sites |
| `MAP.md` | Navigation map: segments, slots, call graph, data refs, state ownership, syscall sites, open findings |

## 4. Finding codes

Errors fail `audit`; warnings fail `audit --strict`.

| Code | Severity | Meaning |
|---|---|---|
| R000 / R001 | info / error | Reference binary identical / different (size, bytes, first offset, owning slot) |
| E001-E004 | error | ELF unreadable, wrong class, wrong `e_machine`, `e_entry` != manifest entry |
| E003 | warning | `e_type` is not `ET_EXEC` |
| E005 | error | `PT_INTERP` or `PT_DYNAMIC` present |
| E006 | error | Entry point is not inside a code slot |
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
| I001 | warning | Decoded instruction differs from its `#=` intent |
| I002 | error | Intent does not start an instruction / header field (byte boundaries shifted) |
| I003 | error | Header intent names a different ELF field |
| X001 | warning / error | No built-in decoder for the ISA (error when exposed) |
| A001 | warning | Slot comments are stale (`annotate` was not run after a byte change) |
