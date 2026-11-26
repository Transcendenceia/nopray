# NOPRAY targets: status, ABIs and boot formats

Status labels used below:

- **RUN**: a NOPRAY image ran here and its behaviour was checked.
- **SOURCE**: the value was checked against the cited primary source on 2026-11-24.
- **DOC**: taken from cited documentation in [MCU-RESEARCH-2026-11-24.md](MCU-RESEARCH-2026-11-24.md), not re-checked.
- **UNVERIFIED**: needed but not confirmed. Do not encode it without confirming first.

## Contents

1. Status matrix
2. Linux user space (x86-64, AArch64 / Raspberry Pi OS, RISC-V)
3. ESP32-C3 / C6 (RISC-V, bare metal)
4. RP2040 / RP2350
5. Raspberry Pi bare metal
6. Xtensa ESP32 / S3 and AVR
7. Tooling roadmap for microcontrollers

## 1. Status matrix

| Target | ISA in `nopray.toml` | Decoder | Emulator here | Status |
|---|---|---|---|---|
| Linux x86-64 | `x86-64` | full | native | **RUN**: calc, nopray3d, nopray-doom (X11 + PipeWire PCM), nopray-httpd |
| **Bare-metal x86-64 (Multiboot 1 → Long Mode + AVX2/FMA/F16C)** | `x86-64`, `bare-metal`, `elf32` | full (`slot.bits = 32/64`, far jump CFG) | `qemu-system-x86_64 -enable-kvm` | **RUN**: `qwen3-tts-x86_64-baremetal` (32 KiB VM running Qwen3-TTS-12Hz-0.6B end-to-end on `/dev/kvm`, 0 Linux syscalls, strict audit 0/0) |
| Linux AArch64 (Raspberry Pi OS arm64, Pi 3/4/5) | `aarch64` | flow + syscalls | `qemu-aarch64` | **RUN** under qemu-user (hello-aarch64); not yet on a real Pi |
| Linux RISC-V 64 | `riscv64` | flow + syscalls | `qemu-riscv64` | **RUN** under qemu-user (hello-riscv64) |
| ESP32-C3 / C6 bare metal | `riscv32`, `bare-metal`, `raw` | flow (no syscalls; MMIO windows) | none (Espressif QEMU fork not installed) | header/watchdog facts **SOURCE**; no image yet |
| RP2040 / RP2350 (Arm) | `thumb` | none (`X001`) | none | **DOC**, several **UNVERIFIED** |
| Raspberry Pi bare metal | `aarch64`, `bare-metal`, `raw` | flow | `qemu-system-aarch64` not installed | **DOC**, entry state **UNVERIFIED** |
| ESP32 / S3 (Xtensa), ATmega328P | `xtensa`, `avr` | none | none | out of scope until a decoder exists |

## 2. Linux user space

| | x86-64 | AArch64 | RISC-V 64 |
|---|---|---|---|
| `e_machine` | 62 | 183 | 243 |
| Syscall | `syscall` (`0f 05`) | `svc #0` (`01 00 00 d4`) | `ecall` (`73 00 00 00`) |
| Number register | `rax` | `x8` | `a7` |
| Arguments | `rdi rsi rdx r10 r8 r9` | `x0`-`x5` | `a0`-`a5` |
| Return | `rax` (`-4095..-1` = `-errno`) | `x0` | `a0` |
| Clobbered by syscall | `rcx`, `r11` | none | none |
| NOP | `90` | `1f 20 03 d5` | `13 00 00 00` (`01 00` compressed) |
| `read / write / exit` | 0 / 1 / 60 | 63 / 64 / 93 | 63 / 64 / 93 |
| `openat / close / socket` | 257 / 3 / 41 | 56 / 57 / 198 | 56 / 57 / 198 |
| Segment alignment | 0x1000 | **0x10000** (16K/64K-page kernels, Pi 5) | 0x1000 (0x10000 is harmless) |

AArch64 and RISC-V use the generic syscall table (`include/uapi/asm-generic/unistd.h`); there is
no `open`, `poll` or `fork`: use `openat`, `ppoll`, `clone`. RISC-V `e_flags`: bit 0 = RVC
(compressed) used, bits 1-2 = float ABI (0 soft, 2 single, 4 double); `0` is right for a
soft-float program without compressed instructions. Raspberry Pi kernels may use 16 KiB pages:
check `getconf PAGESIZE` on the device and keep `p_vaddr ≡ p_offset (mod 0x10000)`.

Verification: `qemu-aarch64 build/app`, `qemu-riscv64 build/app` (package `qemu-user`), then on
the real device with `strace -f`.

## 2.5. Bare-metal x86-64 (Multiboot 1 ELF32 → 64-bit Long Mode + AVX2/FMA/F16C)

**RUN** (verified live 2026-11-26 on AMD Ryzen 7 5825U `/dev/kvm`, `examples/qwen3-tts-x86_64-baremetal/`):

- **Container:** `format = "elf32"`, `os = "bare-metal"`, `isa = "x86-64"`, `e_machine = 3` (`EM_386`), two W^X `PT_LOAD` segments (`text_rodata r-x` at `0x100000`, `data_bss rw-` above text), plus the 12-byte Multiboot 1 header (`0x1BADB002`, flags `0x3`, checksum `0xE4524FFB`) aligned inside the first 8 KiB (e.g. `+0x80`). QEMU `-kernel <elf>` boots straight into 32-bit protected mode at `e_entry` (`EAX = 0x2BADB002`, `EBX = multiboot_info_t*`).
- **32→64 Long Mode bootstrap (`bits = 32` slot):** zero BSS, construct a 4-level identity page map for `0..4 GiB` using 2 MiB huge pages (`PML4[0] → PDPT`, `PDPT[0..3] → PD0..PD3`, mark `>= 0xFE000000` uncacheable for MMIO), enable `CR4.PAE`, set `IA32_EFER.LME` (`MSR 0xC0000080` bit 8), enable `CR0.PG`, `lgdt [gdt64_ptr]`, and execute `jmp far 0x08:boot64` (`ea <u32> 08 00`). `nopray audit` traces `FarBranch32` across the bitness boundary directly to `boot64`.
- **SIMD / AVX2 / FMA / F16C enablement (64-bit `boot64`):** clear `CR0.EM` (bit 2), set `CR0.MP` (bit 1), set `CR4.OSFXSR | CR4.OSXMMEXCPT | CR4.OSXSAVE` (`0x40600`), then execute `xsetbv` (`0f 01 d1`) with `ecx = 0`, `eax = 7`, `edx = 0` (`XCR0 = x87 | SSE | AVX`), and `ldmxcsr [default_mxcsr]` (`0x1F80`). Missing `XCR0=7` traps with `#UD` on the first VEX instruction.
- **Zero-copy `DMCS-L3` external data via `-initrd`:** Multiboot info `mods_count` (`[ebx+0x14]`) and `mods_addr` (`[ebx+0x18]`) hand the kernel physical `mod_start` / `mod_end` of `-initrd weights.nqts` in RAM with zero parsing or copy; place the dynamic scratch/KV arena 2 MiB-aligned immediately above `mod_end`.
- **Verified bare-metal I/O devices under QEMU/KVM:**
  - `COM1` (`0x3F8`, 115200 8N1) for ASCII telemetry (`-chardev stdio,id=c1 -serial chardev:c1`).
  - `isa-debugcon` (`0xE9`) with `cld; rep outsb` (`fc f3 6e`) for bulk raw binary output (`-chardev file,id=c2,path=out.wav -device isa-debugcon,iobase=0xe9,chardev=c2`) — **>100× faster** than byte-polled COM2 (`0x2F8`) under KVM because `rep outsb` collapses into batched string-I/O exits.
  - `PIT Channel 2` (`0x61`/`0x43`/`0x42`, 10 ms gate) to calibrate `RDTSC` ticks/ms.
  - Clean shutdown via `isa-debug-exit` (`out 0xf4, 0`, exit code `1`) with ACPI S5 (`0x604`/`0xb004`) + `cli; hlt` fallback.

## 3. ESP32-C3 / C6 (RV32IMC, bare metal)

**Image container** read by the ROM from flash offset `0x0` (the second-stage bootloader slot),
`esp_image_header_t`, 24 bytes, little-endian (**SOURCE**: esp-idf
`components/bootloader_support/include/esp_app_format.h`):

| Offset | Size | Field |
|---:|---:|---|
| 0 | 1 | magic `0xE9` |
| 1 | 1 | segment count (max 16) |
| 2 | 1 | SPI mode |
| 3 | 1 | flash speed (low nibble) / size (high nibble) |
| 4 | 4 | entry address |
| 8 | 1 | WP pin (`0xEE` = disabled) |
| 9 | 3 | SPI pin drive |
| 12 | 2 | chip ID: C3 `0x0005`, C6 `0x000D`, S3 `0x0009`, ESP32 `0x0000` |
| 14 | 1 | legacy minimum revision |
| 15 | 2 | minimum full revision (major*100+minor) |
| 17 | 2 | maximum full revision |
| 19 | 4 | reserved |
| 23 | 1 | `hash_appended` (1 = SHA-256 appended) |

Then per segment: load address (4), length (4, multiple of 4), data. Checksum: XOR of all
segment data bytes starting from `0xEF` (**SOURCE**: esptool `loader.py`
`ESP_CHECKSUM_MAGIC`), stored as the last byte of the image padded to 16 bytes; with
`hash_appended`, 32 bytes of SHA-256 over everything before follow (**DOC**). The SHA-256 is an
integrity check, not a signature.

**Memory classes** (esptool, **DOC**): DRAM `0x3FC80000-0x3FCE0000`, IRAM
`0x4037C000-0x403E0000`, DROM `0x3C000000-0x3C800000`, IROM `0x42000000-0x42800000`. Which
ranges the ROM accepts for a bootloader replacement, and a safe stack range: **UNVERIFIED**.

**Watchdogs** (**SOURCE**: esptool `targets/esp32c3.py`, `RTCCNTL_BASE_REG = 0x60008000`):

| Step | Register | Value |
|---|---|---|
| Unlock RTC WDT | `0x600080A8` | `0x50D83AA1` |
| Disable RTC WDT | `0x60008090` (WDTCONFIG0) | `0` |
| Relock | `0x600080A8` | `0` |
| Unlock super WDT | `0x600080B0` | `0x8F1D312A` |
| Super WDT auto-feed | `0x600080AC` | read, OR `1 << 31`, write |
| Relock | `0x600080B0` | `0` |

Timer-group MWDT absolute addresses and key: **UNVERIFIED**.

**Output** (**DOC**): UART0 FIFO `0x60000000`, status `0x6000001C` (TX FIFO count in bits
25:16); GPIO output enable set `0x60004024`, output set `0x60004008`, clear `0x6000400C`.
Whether the ROM leaves UART0 at 115200 8N1 for a custom image: **UNVERIFIED**.

**C6** is not a drop-in: different chip ID and, per the research, unconfirmed boot/peripheral
details. Treat every C6 address as **UNVERIFIED** until checked in `soc/esp32c6`.

**Verification path**: Espressif's QEMU fork (`qemu-system-riscv32`) is the intended emulator;
it is not installed here and its machine name for C3 is **UNVERIFIED**. Hardware:
`esptool --chip esp32c3 --port <port> read-flash 0 0x400000 backup.bin` first, then
`write-flash 0x0 image.bin`; recovery is ROM download mode (BOOT button). Flashing needs approval.

## 4. RP2040 / RP2350

- Cortex-M0+ Thumb; NOP `00 bf`. The boot ROM checks a 256-byte boot2 stage with a CRC32 whose
  exact parameters are **UNVERIFIED**: do not hand-compute it from generic CRC32 defaults.
- UF2 blocks (512 bytes): magics `0x0A324655`, `0x9E5D5157`, end `0x0AB16F30`, flag
  `0x00002000` = family ID present, RP2040 family `0xE48BFF56` (**DOC**, microsoft/uf2).
- SIO GPIO: OE set `0xD0000024`, out set `0xD0000014`, out clear `0xD0000018`; UART0 data
  `0x40034000`, flags `0x40034018`, TXFF bit 5 (**DOC**). Pin mux, resets and clocks: **UNVERIFIED**.
- RP2350 uses IMAGE_DEF / picobin blocks and can run Arm or Hazard3 RISC-V cores: nothing from
  RP2040 boot2 applies.
- `nopray` has no Thumb decoder yet, so audits report `X001`: this target is not ready.

## 5. Raspberry Pi bare metal

`kernel8.img` is a raw image loaded by the GPU firmware (`config.txt`: `arm_64bit`,
`kernel_address`, `enable_uart`). Load address per model and firmware, entry exception level,
secondary-core state and UART bases (BCM2837 / BCM2711 / RP1 on Pi 5): **UNVERIFIED** here.
QEMU `raspi3b`/`raspi4b` load images differently from the real firmware. Prefer Linux user
space on a Pi until a bare-metal need is concrete.

## 6. Xtensa ESP32 / S3 and AVR

Xtensa mixes 16- and 24-bit instructions with windowed registers and literal pools; AVR uses
16-bit words, some instructions take two. Both need a decoder in `nopray` before NOPRAY can
honestly audit them.

## 7. Tooling roadmap for microcontrollers

Allowed tooling never encodes instructions; it lays out, checksums and decodes.

1. `nopray pack esp` (planned): wrap `raw` slots into `esp_image_header_t` + segments +
   checksum + optional SHA-256, with chip ID and revision fields from the manifest.
2. Thumb decoder (planned; `yaxpeax-arm` has an ARMv7/Thumb decoder) to lift RP2040 from `X001`.
3. Install the Espressif QEMU fork (needs approval) and record the exact command in this file.
4. First bare-metal app: ESP32-C3 "UART hello + LED blink", emulator first, then hardware with a
   firmware backup.
