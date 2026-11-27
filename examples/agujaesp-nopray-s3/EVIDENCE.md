# Evidence · 2026-11-27

Tested with the Espressif QEMU 9.2.2 release (`esp_develop_9.2.2_20261109`), stock bundled S3 rev0 mask ROM, simulated 4 MiB flash, UART0 file backend and NIC disabled. No physical flash, Wi-Fi, Headscale or OTA were tested.

- SHA-256 (entire 160-byte probe): `2bac9d92778dc26866288ac50fc842654022dd02791322c122304c52b9b25b86`.
- `R000`: delivered file matches the slot map byte-for-byte. Audit: 0 errors, **2 warnings** (`S001`, `X001`); strict mode is not clean.
- `verify-qemu.sh`: exit 0. Good fixture: timeout exit 124 after one marker. The 13 observed MMIO writes were 32-bit writes to `0x60000000`; data hex: `4e4f505241592d5333204f4b0a`.
- Negative fixture: flip bit 0 of file byte `0x7F` (not an instruction), checksum `0x12 → 0x13`. ROM rejected the image; no application marker.

UART extracts (CRLF normalized):

```text
ESP-ROM:esp32s3-20210327
Build:Mar 27 2021
rst:0x1 (POWERON),boot:0x4 (SPI_FLASH_BOOT)
SPIWP:0xee
mode:DIO, clock div:2
load:0x403c8700,len:0x58
entry 0x403c8704
NOPRAY-S3 OK

Checksum failure. Calculated 0x12 stored 0x13
```

First decoded translation block (external observation, not a native audit):

```text
0x403c8704:  l32r a2, 0x403c8700
0x403c8707:  movi a3, 78
0x403c870a:  s32i a3, a2, 0
0x403c870d:  movi a3, 79
0x403c8710:  s32i a3, a2, 0
0x403c8713:  movi a3, 80
0x403c8716:  s32i a3, a2, 0
0x403c8719:  movi a3, 82
0x403c871c:  s32i a3, a2, 0
0x403c871f:  movi a3, 65
0x403c8722:  s32i a3, a2, 0
0x403c8725:  movi a3, 89
0x403c8728:  s32i a3, a2, 0
0x403c872b:  movi a3, 45
0x403c872e:  s32i a3, a2, 0
0x403c8731:  movi a3, 83
0x403c8734:  s32i a3, a2, 0
0x403c8737:  movi a3, 51
0x403c873a:  s32i a3, a2, 0
0x403c873d:  movi a3, 32
0x403c8740:  s32i a3, a2, 0
0x403c8743:  movi a3, 79
0x403c8746:  s32i a3, a2, 0
0x403c8749:  movi a3, 75
0x403c874c:  s32i a3, a2, 0
0x403c874f:  movi a3, 10
0x403c8752:  s32i a3, a2, 0
0x403c8755:  j 0x403c8755
```

## Edit rehearsal

In a scratch copy, changed the immediate in one existing `MOVI` from `4b` (`K`) to `21` (`!`),
updated its intent, and refreshed the XOR checksum and the SHA-256 trailer. `nopray diff` reported
three changed slots (`_start`, `checksum`, `image_digest`), no moves, and still 160 bytes. The
edited image booted and UART returned `NOPRAY-S3 O!`. Its SHA-256 was
`448dc3352d0b61a9bed6fc3aa906e148b04aae833e18771278b62203e886329c`. The original remains unchanged.

Repository verification: `cargo test --locked` for the host-side auditor passed all 17 tests, including
the locked-byte example suite. `shellcheck verify-qemu.sh` passed. The 160-byte image was never compiled.
