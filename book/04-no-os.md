# Chapter 4 · Without an operating system

> Without an OS you are the OS of one program. Everything a syscall used to do, you now owe
> the program yourself, and nothing more than that.

## 4.1 Principle

Bare metal does not add a new kind of programming; it moves the door. Instead of a syscall
instruction, the program talks to a **loader contract** at the start and to **devices**
afterwards. A program, or a port, needs exactly these obligations covered:

| What the OS did | What you provide on bare metal | Proven in |
|---|---|---|
| Load the program | Satisfy a boot protocol: Multiboot 1 (`qemu -kernel`), the ESP mask-ROM image loader, the Raspberry Pi firmware (`kernel8.img`), RP2040 `boot2` | Qwen (Multiboot), AgujaESP (ESP ROM) |
| Put the CPU in a usable mode | Mode switches yourself: 32-bit → long mode, paging, GDT | Qwen `0100_boot32.hex` |
| Enable FP/SIMD | CR0/CR4/XCR0 and `MXCSR` on x86-64; nothing is on by default | Qwen `0200_boot64.hex` |
| Stack, heap, `mmap`, `brk` | Fixed regions declared in the manifest; zero `.bss` yourself; identity page tables | Qwen `0100_boot32.hex` |
| Files and arguments | Data handed over by the loader (`-initrd`, Multiboot modules) or a flash partition | Qwen (`-initrd weights.nqts`) |
| `write` to a terminal | A UART driver (COM1 at `0x3F8`; ESP32-S3 UART0 FIFO at `0x60000000`) | Qwen `0400_io_drivers.hex`, AgujaESP probe |
| Write a file | A debug port or a second stream (`isa-debugcon` at `0xE9` carries the WAV) | Qwen `0400_io_drivers.hex` |
| Clocks | A timer you calibrate (PIT channel 2 → TSC) | Qwen `0200_boot64.hex` |
| `exit` with a status | Something the harness can observe: `isa-debug-exit` (`0xF4`), ACPI S5, or a UART marker and a spin | Qwen, AgujaESP |
| Watchdogs, interrupts | Feed or disable the watchdog; poll instead of interrupts unless you need them | documented for ESP32-C3 in [TARGETS.md](../references/TARGETS.md) |

The list for a port comes from chapter 3: the program's syscall set **is** its bare-metal
obligation list. Each syscall site becomes a driver call or disappears.

## 4.2 Procedure

1. **Name the loader boundary** and cite its contract from a primary source (protocol spec,
   SoC technical reference, vendor ROM documentation). Record unverified facts as UNVERIFIED.
2. **Write the obligation table** for this program, as in 4.1: one row per OS service it uses.
3. Declare every device window as `[[mmio]]`; the audit flags accesses outside them (`C009`).
4. **Boot first, alone.** Author the boot slots and prove them with a fixed marker on the
   serial line before writing any application code.
5. Drivers next, each in its own slot with a contract; then the application slots.
6. An exit path the harness can see, so tests can be automated.
7. Emulate first (QEMU, Espressif QEMU); hardware only per chapter 9.

## 4.3 Traps

- **Features off at reset.** SSE/AVX instructions raise `#UD` until CR4/XCR0 enable them.
- **`.bss` is not zero** unless you zero it.
- **The stack lands on your data.** Place it explicitly and leave a gap.
- **The loader checks the image.** ESP images carry a checksum and SHA-256 (`E015`/`E016`);
  a single edited byte without refreshing them is rejected by the ROM.
- **Watchdogs reset the chip** while you debug; it looks like a crash loop.
- **An emulator UART pass proves the code path, not the board** (timing, Wi-Fi, real flash).
- **Register addresses from memory or forums.** Cite the reference manual or mark UNVERIFIED.

## 4.4 Worked examples

**Qwen3-TTS on bare-metal x86-64** ([example](../examples/qwen3-tts-x86_64-baremetal/)): a
PyTorch application turned into a 32 KiB Multiboot 1 ELF that boots in QEMU/KVM with no Linux.
Read its slots in this order to see 4.1 applied:

1. `0000_elf32_hdr.hex`: ELF32 with the 12-byte Multiboot header the loader looks for.
2. `0100_boot32.hex` (`bits = 32`): zero `.bss`, build 2 MiB identity pages for 0–4 GiB,
   PAE/LME/PG, `lgdt`, far jump to 64-bit code.
3. `0200_boot64.hex`: stack, state register `r15`, SSE/AVX2/FMA/F16C enabled
   (`XCR0=7`, `MXCSR=0x1F80`), UART init, PIT→TSC calibration, find the `-initrd` weights.
4. `0400_io_drivers.hex`: COM1 telemetry, `isa-debugcon` binary output, timer, exit.
5. Only then the model: `1000_math_avx2.hex` … `5000_decoder_vocoder.hex` (chapter 6).

**AgujaESP probe on the ESP32-S3** ([example](../examples/agujaesp-nopray-s3/)): 160 bytes. The
mask ROM is the loader; the image header, one segment, checksum and digest satisfy it; the code
writes a 13-byte marker to the UART0 FIFO and spins. No stack, no calls, no inputs: the
smallest proof that the loader boundary is understood.

## 4.5 Problems

1. ★ **Multiboot hello.** A Multiboot 1 ELF that prints a marker on COM1 and exits through
   `isa-debug-exit`. Accept: QEMU's exit status is `(code << 1) | 1` for the code you wrote, the
   marker appears on the serial chardev, `audit --strict` is clean.
2. ★★ **Port `wc` to bare metal.** Take problem 3.1 and remove Linux: input from `-initrd`,
   counts on COM1, exit code 0. Accept: the obligation table is in the README; outputs equal
   the Linux version on the same five inputs.
3. ★★ **Another machine.** Bare-metal hello on `qemu-system-riscv64 -machine virt`. Accept:
   the load address and UART base come from primary sources (QEMU documentation or the dumped
   device tree) and are cited in the manifest notes.
4. ★★★ **A board.** Bare-metal AArch64 hello for a Raspberry Pi under QEMU `raspi3b`, then on
   hardware with a spare SD card (approval, chapter 9). Accept: UART marker on both, and the
   gaps listed in [TARGETS.md](../references/TARGETS.md) §5 are closed or kept as UNVERIFIED.

## 4.6 State of knowledge

- Verified: x86-64 Multiboot 1 → long mode with AVX2 on KVM (Qwen); ESP32-S3 ROM boot in
  Espressif QEMU and by ROM RAM load on an N16R8 board (AgujaESP).
- Documented with gaps: ESP32-C3/C6, RP2040/RP2350, Raspberry Pi bare metal
  ([TARGETS.md](../references/TARGETS.md), [MCU-RESEARCH](../references/MCU-RESEARCH-2026-11-24.md)).
- Not yet demonstrated: interrupt-driven drivers, multi-core bare metal, a network stack
  written by hand (the AgujaESP node runs imported ESP-IDF Wi-Fi/TLS/WireGuard bytes).
