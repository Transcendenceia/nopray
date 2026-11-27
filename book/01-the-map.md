# Chapter 1 · The map: every byte has an owner

> A program you can change months later is a program whose bytes are explained. The map is
> the explanation; the bytes are only the conclusion.

## 1.1 Principle

A compiler keeps a symbol table, a layout and a calling convention in its head and throws them
away. NOPRAY keeps them **outside the bytes, in `nopray.toml`**, so the next agent inherits
them:

- **Segments** say what the loader maps and with which permissions.
- **Slots** split the file into named, fixed-address regions: header, one per routine, data.
  Every byte belongs to exactly one slot (`G001` otherwise).
- **Contracts** say what a code slot takes, returns, destroys and calls.
- **Labels** name interior addresses: entry points other slots may reach, and every string or
  table inside a data slot.
- **State fields** name the memory a permanent base register points at, so the map can say
  which slot reads and writes each field.

Fixed slots are the core idea. A routine lives at a fixed address with spare room (NOP
padding). Changing it never moves another routine, so every `call` and pointer elsewhere stays
valid. It is a linker done by hand, declared, and checked.

## 1.2 Procedure

1. Decide the target and its exposure first (chapter 8); they constrain the layout.
2. Write `nopray.toml` **before any opcode**: segments, then slots with `purpose` and
   `origin`, contracts, labels, `[syscalls]` or `[[mmio]]`, state fields.
3. Size each code slot about 1.5x its expected size, rounded to 0x10 or 0x40. Put small,
   reused helpers (write-all, read-exact, exit) in their own slots at the front.
4. One permanent register holds the state base (`r15` on x86-64, `x28` on AArch64). Declare it
   as `state_register`/`state_base` and name the fields.
5. Author the header field by field, one field per line, each with `#= <field> <value>`.

Field reference: [MANIFEST.md](../references/MANIFEST.md). Layout patterns:
[FIXED-SLOT-ABI.md](../references/FIXED-SLOT-ABI.md). ELF templates:
[ELF64-X86_64-SPEC.md](../references/ELF64-X86_64-SPEC.md).

## 1.3 Traps

- **Slots sized exactly to their content.** The first edit then moves everything after it.
- **A data slot with several strings and no labels.** A pointer that is off by two bytes
  still lands inside the slot and nobody notices (`C017` exists for this; chapter 2).
- **Contracts written after the code.** They end up describing what the code happens to do,
  not what callers may rely on. Write them first; `C011`/`C012` then check the bytes.
- **AArch64 segments aligned to 4 KiB.** They fail to load on 16K/64K-page kernels; align to
  0x10000 (`S004`).

## 1.4 Worked examples

| Example | What it teaches | Look at |
|---|---|---|
| [`hello-aarch64-linux`](../examples/hello-aarch64-linux/) | The whole method in 384 bytes: header slot by ELF field, one code slot, one labelled data slot, every line intented | `nopray.toml`, `slots/0000_elf_header.hex` (one field per line), `slots/0100_start.hex` |
| [`hello-riscv64-linux`](../examples/hello-riscv64-linux/) | The same map on another ISA: only the bytes change | `slots/0100_start.hex` (`auipc`/`addi` pair) |
| [`calc-x86_64-cachyos`](../examples/calc-x86_64-cachyos/) | Helper slots at the front (`read_exact`, `write_all`, `exit_clean`) reused by everything after; one slot per routine in a 4 KiB page | `slots/0080_read_exact.hex` … `0960_render_and_blit.hex` |
| [`nopray3d-x86_64-cachyos`](../examples/nopray3d-x86_64-cachyos/) | Two segments: `r-x` code and `rw-` state/framebuffer (W^X) | `[[segment]]` entries in `nopray.toml` |

Run `$T context examples/nopray-doom-x86_64-cachyos fill_rect` to see what the map tells an
agent about one routine of a 32 KiB program without reading the rest.

## 1.5 Problems

Each problem is solved when its acceptance check passes. Do not look at an example's slots
until you have a first version of your own.

1. ★ **Two lines.** Extend `hello-riscv64-linux` (in a copy) to print two different
   strings from one data slot. Accept: both strings are labels, every reference uses a field
   (chapter 2), `audit --strict` is clean, `qemu-riscv64` prints both lines.
2. ★★ **Grow past the slot.** Make `_start` of your copy longer than its slot. Move the data
   slot instead of shrinking the code. Accept: `diff` reports the moved slot, every reference
   to it was a field so nothing else needed editing, the program still runs.
3. ★★ **Map from a description.** Given only "a Linux x86-64 program that reads stdin and
   writes it reversed", write the complete `nopray.toml` (segments, slots, contracts, state,
   syscalls) before any byte. Accept: a reviewer can name every routine and its callers from
   the manifest alone; then implement it and confirm the map did not need structural changes.

## 1.6 State of knowledge

- Verified: the layout rules above on x86-64, AArch64 and RISC-V Linux, and on bare-metal
  x86-64 and the ESP32-S3 (chapters 4 and 9).
- Open: a convention for apps larger than a few hundred slots (directory-level grouping)
  has not been needed yet.
