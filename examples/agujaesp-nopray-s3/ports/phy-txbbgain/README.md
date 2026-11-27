# ESP32-S3 PHY · transmit baseband gain-to-index port

A **52-byte routine** in a **240-byte standalone test image**. This is a behavioral port,
not an imported instruction stream. Its instruction bytes were authored by hand as CALL0
Xtensa; it has no stack frame, no radio MMIO, no vendor library and no network. The existing
Espressif object is used for observation only. Stock mask ROM is the loader boundary.

## Original behavior

Reference: ESP-IDF v5.5's [pinned ESP32-S3 libphy.a](https://github.com/espressif/esp-phy-lib/blob/f6746ceea821b3800e8700d7e535f5df7304b8e5/esp32s3/libphy.a),
SHA-256 `af64b3d0ab790b03e297246538fdf6ed5c9a286387c07ed8b8c7c626a2c9c785`.
Member `phy_tx_gain.o`, SHA-256 `f3969333b1fe26b564995fc1a5d001d6b9637ca17b3cfd4c8d40c5e70663e1f3`;
42-byte section `.text.rom_txbbgain_to_index`, object file offset `0x124`. Its six
`R_XTENSA_SLOT0_OP` records refer to in-section branch destinations. The observed original
uses `entry/retw.n` (windowed ABI); the port uses an explicit `callx0/ret.n` pair.
The new routine must **not** be dropped into a stock windowed call site without an adapter.

| Low 16 bits of input `a2` | Original and port result in `a2` |
|---:|---:|
| 32 | 3 |
| 128 | 1 |
| 160 | 4 |
| 256 | 2 |
| Anything else | 0 |

Derivation: the original starts with a low-16-bit extraction. For values at or below 128,
it accepts 128 first and then 32; for values above 128, it accepts 160 then 256. Other
values terminate at the zero result. The port compares the same four values in a different
order after masking. Neither routine accesses RF hardware. The map declares `a3/a4` clobbered
by the routine, and `a0` is the CALL0 return link placed by the caller.

## Proof and reproduction

From this directory use `nopray annotate --check .`, `nopray build .` and
`nopray audit . --binary build/phy-txbbgain.bin`. SHA-256:
`f4cd821a7f02e6d42214f50388889ccb1af10acc91d1260cf793ec94751050d9`.

```sh
./verify-qemu.sh /path/to/espressif/qemu-system-xtensa ./new-evidence
```

Success: UART prints `P` after all seven cases match. The cases include all four mapped
inputs, default, high bits set on a matching input, and high bits set on a non-matching input.
Changing a single expected value in a scratch fixture (and fixing its trailer integrity)
prints `F`. The emulator disables networking and runs the stock SPI ROM path. On an
ESP32-S3 rev 0.2 N16R8, the same 240-byte image was read back from the authoring map,
its 172-byte segment was uploaded into RAM via mask ROM with no stub, and UART emitted
`P` (`0x50`). The existing flash was preserved and RTS reset the target after the test.

The audit has no errors, but `S001`, `X001`, and the unproven indirect call warning
`C012` remain. The observed indirect call and UART output are external evidence, not a
decoder upgrade or strict pass. This is not a radio driver, PHY bring-up, or proof of Wi-Fi,
Headscale registration, OTA, or all 2^32 inputs exercised on hardware.

To repeat the positive hardware test on the explicitly selected, idle USB-UART device:
`python ../../verify-ram.py --profile phy-gain --port <device>` (host esptool 5.7.0).
This rejects the original UART probe if passed as a gain image before opening the serial
port. Both profiles were tested on hardware after adding the selector.
