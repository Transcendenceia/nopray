# Fixed-Slot Virtual Address Architecture (`FIXED-SLOT-ABI`, DMCS-1.1)

> Since DMCS-1.1 the slot table lives in `nopray.toml` and is enforced by `nopray audit`
> (see [MANIFEST.md](MANIFEST.md)); this document explains the reasoning behind it.

## Why Hand-Encoding Machine Code Normally Fails — And How Fixed Slots Solve It

The #1 historical barrier to writing raw machine code without an assembler or linker is **address cascading**: adding or removing a single byte inside function A shifts the virtual addresses of functions B, C, D and every data table behind them, invalidating dozens of `call rel32`, `jmp`, and memory operand addresses simultaneously.

**The DMCS-1.0 Solution: Fixed-Slot Virtual Address Partitioning.**

Instead of packing functions byte-to-byte, partition the executable into **fixed, power-of-two aligned virtual address slots** padded with `NOP` (`0x90` on `x86_64`, `0xD503201F` on `aarch64`) in code slots and `0x00` in data slots.

---

## 1. Canonical Single-Page Layout (`4,096` Bytes — `DMCS-L0/L1`)

Fits compact utilities, calculators, protocol clients, TUI tools, and small GUI apps into **one single 4 KiB OS memory page** on disk (`p_filesz = 0x1000`):

```text
Virtual Address    File Offset   Size     Slot Purpose
────────────────────────────────────────────────────────────────────────────
0x00400000         0x0000        0x0080   ELF64 Header (64B) + PT_LOAD (56B) + 8B pad
0x00400080         0x0080        0x0080   Low-level syscall wrappers (read_exact, write_all, exit)
0x00400100         0x0100        0x0200   _start (init, open sockets/files, setup window/state)
0x00400300         0x0300        0x0120   Main event / IO loop & input dispatch
0x00400420         0x0420        0x0260   Core domain engine (state machine / protocol / math)
0x00400680         0x0680        0x0180   Formatters / parsers / serialization
0x00400800         0x0800        0x0160   Rendering / output primitives (fill_rect, draw_glyph)
0x00400960         0x0960        0x02A0   Scene composer & frame blitter / response writer
0x00400C00         0x0C00        0x0400   Read-only data (strings, headers, font, palettes, tables)
────────────────────────────────────────────────────────────────────────────
0x00402000         (zero-fill)   0x00A0000 .bss (state struct + IO buffers + software framebuffer)
```

### Properties Guaranteed by Fixed Slots
1. **Local Edit Isolation:** Editing a function at `0x400420` only affects internal jumps within `0x400420..0x40067F`. Every external `call 0x400420` and every address in `0x400680..0x400FFF` remains **100% unchanged**.
2. **Instant Branch Arithmetic:**
   - Near call (`E8 rel32`, 5 bytes): `rel32 = (target - (insn_addr + 5)) & 0xFFFFFFFF` (little-endian).
   - Short jump (`EB rel8` / `7x rel8`, 2 bytes): `rel8 = (target - (insn_addr + 2)) & 0xFF` (valid for `-128..+127`).
   - Near conditional jump (`0F 8x rel32`, 6 bytes): `rel32 = (target - (insn_addr + 6)) & 0xFFFFFFFF`.
3. **Zero Relocation Overhead:** Non-PIE fixed load address `0x400000` (classic Linux x86-64 static executable address) allows direct 32-bit absolute addressing for static buffers and tables (`mov esi, 0x400C00`, `lea rdi, [rsi*4 + 0x408000]`).

---

## 2. Hardened Dual-Segment `W^X` Layout (`DMCS-L2`)

For security-sensitive or larger applications (8 KB–64 KB+), split into **two `PT_LOAD` segments** so no page is ever writable and executable at the same time (`W^X` compliance):

| Segment | `p_flags` | File Offset | Virtual Address | Contents |
| :--- | :--- | :--- | :--- | :--- |
| **Segment 0 (RX)** | `PF_R \| PF_X` (`5`) | `0x0000` | `0x00400000` | ELF headers + all machine-code slots + immutable `.rodata` tables (e.g. `p_filesz = p_memsz = 0x2000` or `0x4000`). |
| **Segment 1 (RW)** | `PF_R \| PF_W` (`6`) | `0x2000` (or `0` if pure `.bss`) | `0x00402000` (next page boundary!) | Mutable `.data` (`p_filesz`) + zero-initialized `.bss` (`p_memsz`, e.g. `0x100000` = 1 MiB). |

> **Kernel Constraint:** For every `PT_LOAD` segment, `p_offset % 0x1000 == p_vaddr % 0x1000` MUST hold or Linux `execve()` rejects the ELF with `ENOEXEC` / `SIGKILL`. Using page-aligned `0x1000` multiples satisfies this automatically.

---

## 3. Dedicated State-Base Register Convention

Eliminate RIP-relative math and bulky 32-bit addresses for state variables by pinning **one callee-saved register** at `_start` to the start of `.bss`:

- **`x86_64`:** Pin `r15 = 0x402000` (`41 bf 00 20 40 00`).
  - Place all hot scalar state fields in `[r15 + 0x00 .. r15 + 0x7C]` (all offsets `<= 0x7F` fit in a signed `disp8`, encoding loads/stores in just **3–4 bytes**, e.g. `49 8b 47 08` = `mov rax, [r15+8]`).
  - Place large buffers at clean page/hex offsets (`0x402080`, `0x402100`, `0x402200`, `0x408000` framebuffer) and load them with 5-byte `mov ebx, 0x402100`.
- **`aarch64`:** Pin `x28` to the start of the RW segment (on AArch64 put it on a 64 KiB boundary, e.g. `0x410000`, so the image also loads on 16K/64K-page kernels). All 64-bit loads/stores up to `+32760` bytes encode in a single 4-byte `ldr/str` instruction!

---

## 4. Register Clobber Contract

Explicitly document which registers each helper preserves so callers avoid redundant stack spills:

| Routine Category | Preserved Registers | Allowed Clobbers |
| :--- | :--- | :--- |
| **Linux Syscall (`syscall`)** | `rbx, rbp, rsp, rdi, rsi, rdx, r8, r9, r10, r12–r15` | `rax` (return), `rcx`, `r11` |
| **Standard Internal Function** | `rbx, rbp, rsp, r12, r13, r14, r15` | `rax, rcx, rdx, rsi, rdi, r8, r9, r10, r11` |
| **Tight Leaf Primitive** (e.g., `fill_rect`) | Document exact untouched volatile regs (e.g., `r8–r11`, `rdx`) so hot callers like `draw_glyph` run with zero stack traffic inside inner pixel loops. | Only what the leaf explicitly writes. |

---

## 5. Scaling Beyond 4 KB (Multi-Module Applications)

When an application grows to 8 KB–64 KB (e.g., HTTP server + JSON/HTML router, Wayland desktop app, Chip-8 emulator, audio synth):
1. Allocate **256-byte (`0x100`) or 512-byte (`0x200`)** slots per module, leaving 30–50% NOP headroom in each slot for future patches.
2. Put a **Jump/Dispatch Table** or **Function Directory** at a fixed page (`0x400080` or `0x401000`) if modules call each other dynamically. Indirect branches must be justified in the slot's `indirect` field (`C006`); for internet-exposed apps prefer direct calls.
3. For large read-only assets (fonts, icons, lookup tables, HTML templates), allocate dedicated 4 KiB pages at `0x404000+` so code pages never collide with asset growth.
4. Keep `.hex` organized into visually separated blocks matching the slot table so humans and agents can diff and patch a single slot cleanly.

---

## 6. Slots as Enforced by `nopray` (DMCS-1.1)

- Every file byte belongs to a declared slot (`G001`); slot names, purposes and contracts are
  the map other agents navigate (`build/MAP.md`).
- Code slots are padded with the ISA NOP; the tail must be a whole number of NOPs.
- Other slots may only branch to a slot start or to a label declared with `entry = true`
  (`C005`). Ending a slot by falling into the next one is reported (`C015`) because it couples
  two slots invisibly.
- Kernel page-size congruence (`p_vaddr ≡ p_offset mod p_align`) is checked (`E010`); on AArch64
  use `p_align = 0x10000` (`S004`).
- The fixed load address means no ASLR: containment matters more (see [SECURITY.md](SECURITY.md)).
