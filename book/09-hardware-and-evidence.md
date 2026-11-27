# Chapter 9 · Real hardware, and what counts as proof

> An emulator tells you the bytes are plausible. A board tells you they are true. A claim is
> worth exactly the evidence written next to it.

## 9.1 Principle

Two disciplines close every NOPRAY job:

1. **Climb the fidelity ladder one rung at a time.** On each rung, record what it proves and
   what it does not.

   | Rung | What it proves | What it does not |
   |---|---|---|
   | Static audit | layout and integrity | behaviour |
   | Emulator | behaviour under the emulator's model | the board |
   | RAM execution on the board | the silicon runs the bytes | boot from flash |
   | Flash with read-back | the device holds exactly the map | that the app works |
   | Live behaviour | the claim | — |

   Skipping a rung moves risk to a place where it is harder to see and harder to undo.

2. **A hardware write is an owner decision.** Before it, you need explicit approval, the exact
   device, a backup and a known recovery path. eFuses, secure boot and flash encryption are
   never written from a NOPRAY image ([SECURITY.md](../references/SECURITY.md#6-bare-metal-and-hardware-checklist)).

Evidence uses one vocabulary everywhere:

- **VERIFIED**: observed, with the command and output recorded.
- **UNVERIFIED**: designed or reasoned, but not observed.
- **Not claimed**: out of scope, said out loud.

A negative test belongs in the evidence too: the system rejected a broken input.

## 9.2 Procedure

1. Run static checks first: `audit --strict`, and `audit --binary` against any reference.
2. Emulate, with a negative fixture: a one-bit corruption that must be rejected. State the
   emulator's fidelity limits (ROM version, peripherals, flash model, `-kernel` shortcuts).
3. Identify the target with read-only commands: chip, revision, memory, security state.
4. Ask for approval and name the device. Back up the existing contents, verify the backup,
   and keep it private.
5. Run on the board without writing flash if the platform allows it (ESP ROM RAM loader,
   RP2040 SRAM, chain-loading).
6. Write with the tool's header-rewriting disabled. Then read back every region and compare it
   with the map (`R000`).
7. Observe the behaviour you claimed, at the boundary where it matters: UART, network, API.
8. Write the evidence file: commands, hashes, observed output, negative cases, what was not
   tested. Keep secrets out of it.

## 9.3 Traps

- **"It ran in QEMU."** The AgujaESP run had no radio and the NIC disabled, and `-kernel`
  would have bypassed the ROM-from-flash path. Say which path ran and what was not modelled.
- **Flashing tools that rewrite headers.** esptool adjusts flash mode and size unless told to
  `keep` them, and the image on the chip then differs from the map.
- **Backups that contain credentials.** A flash dump holds Wi-Fi keys and tokens. It stays
  mode 0600 on the host, never in the repo or a PR.
- **Fixtures tuned for the emulator.** The probe's 4 MiB/DIO header is for QEMU, not a WROOM
  module. Do not flash it.
- **Overclaiming.** Running the bytes is not booting from flash, and booting from flash is not
  a working node. Name the rung reached.

## 9.4 Worked example: AgujaESP, rung by rung

Source: [EVIDENCE.md](../examples/agujaesp-nopray-s3/EVIDENCE.md).

1. **Static.** The 160-byte probe matches its map (`R000`). The audit reports 0 errors and
   2 warnings stated openly: `S001` (writable executable IRAM) and `X001` (no Xtensa decoder).
2. **Emulator.** Espressif QEMU 9.2.2 with the stock S3 ROM booted it through SPI and printed
   `NOPRAY-S3 OK` as 13 writes to `0x60000000`. Flipping one bit of the checksum made the ROM
   reject the image. The decoded translation block matched all 28 authored intents.
3. **Identify.** Read-only ROM commands found an ESP32-S3 rev v0.2 with 16 MB flash and 8 MB
   PSRAM, Secure Boot and Flash Encryption disabled. That is an N16R8. Nothing was written.
4. **RAM execution.** The ROM loader, without the stub, ran the same bytes on the silicon:
   `NOPRAY-S3 OK` over the board's USB-UART, with no flash write.
5. **Flash with read-back** (after approval). The flash was backed up and verified first; the
   board was blank. Then the imported La Aguja flash set was written with the header flags set
   to `keep`. Every region read back identical, and each public region passed `R000`.
6. **Live behaviour.**
   - The board associated with Wi-Fi and registered with Headscale as node 47.
   - `tailscale ping` was direct, and the API returned 200 with a token and 401 without.
   - A 27-byte slot edit was installed over OTA and survived the rollback window (chapter 7).

At every step the README states what has not been shown. Examples: SPI boot of the probe on
real hardware, a signed OTA, rollback under failure.

## 9.5 Problems

1. ★ **Negative fixture.** For any example with an integrity check, write the one-bit
   mutation that must be rejected.
   - Accept: the rejection is observed and recorded.
2. ★★ **Evidence file.** Write `EVIDENCE.md` for problem 4.1 (the Multiboot hello).
   - Accept: rung reached, commands, hashes, negative case and a "Not claimed" list.
   - Accept: a stranger can reproduce it.
3. ★★★ **Second board.** Take the probe to another MCU family (RP2040 or an ESP32-C3) in its
   emulator.
   - Accept: rungs 1–2 pass with a negative fixture.
   - Accept: a written plan for rungs 3–6 that names the recovery path, *before* asking for
     any hardware write.

## 9.6 State of knowledge

- **Verified:**
  - ESP32-S3 at rungs 1–6: probe to RAM execution, imported node to live behaviour
  - QEMU/KVM bare metal on x86-64 (Qwen)
- **Open:**
  - an all-authored image booting from flash on real hardware
  - RISC-V and AArch64 boards
  - RP2040
  - a native Xtensa decoder, which would close `X001`
