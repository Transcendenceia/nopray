# Native-node design and acceptance gates

**Owner:** Angie · **Scope:** NOPRAY example, map, workflow and emulator lab. On 2026-11-27 Marlon directed the live deployment: one owner-approved flash write to the N16R8 and one Headscale enrollment; no ACL or route changes. This is the work contract and handoff; unchecked rows below are not delivered features.

**Behavior reference:** La Aguja [source at the observed upstream HEAD](https://github.com/Transcendenceia/La-Aguja/tree/52646207ec6063755b7b0a7a741e454eadd4c1c8/iot). Its S3 profile (`esp32s3-oct` for WROOM N16R8) shares the ESP-IDF/MicroLink firmware tree with C6. Its imager and board registry establish useful conventions, not evidence for NOPRAY or for a specific newly attached module.

## Where strict NOPRAY stops today

Stock mask ROM is the starting platform boundary; no linked ESP-IDF code or opaque Wi-Fi binary is included in this probe. Espressif's [S3 Wi-Fi API](https://docs.espressif.com/projects/esp-idf/en/v5.5/esp32s3/api-reference/network/esp_wifi.html) documents the supported software path; [its Wi-Fi library repository](https://github.com/espressif/esp32-wifi-lib) distributes binary libraries. Neither the SPI boot test nor that API establishes a documented, independently verified ROM-only S3 radio bring-up path. Do not label a bridge, an IDF port, or opaque driver reuse as strict all-authored firmware. The [selected S3 QEMU machine](https://github.com/espressif/qemu/blob/esp-develop-9.2.2-20261109/hw/xtensa/esp32s3.c) models OpenCores Ethernet and UART; no validated Wi-Fi STA model was used. Synthetic Ethernet network tests would not constitute radio evidence.

A future hybrid using vendor wireless code is a separate provenance profile, not a fallback to apply silently. It still needs an evidenced interface and does not waive the audit or deployment gates.

## Proposed boundaries

- **Physical / initial provisioning:** identify the full WROOM suffix, flash, PSRAM, silicon revision, UART routing, and security state. Keep a verified flash backup outside the repo, with a ROM-download recovery path tested and an explicitly approved write. Never burn eFuses for this bring-up. Inject unique Wi-Fi and node credentials locally; no credentials in the example, tracked binary, UART, or provisioning logs.
- **Radio and IP:** compatible Wi-Fi STA association and authentication; address, DNS and clock strategy appropriate to the network. No SoftAP by default. Exercise disconnect/rejoin and wrong credentials on an isolated test network first.
- **Control vs. data:** validate the deployment's TLS trust policy and supported control version; register with a single-use, scoped credential, store unique keys securely, and verify reboots retain the node identity. [Headscale enrollment](https://headscale.net/stable/usage/getting-started/) and [Tailscale control handshake](https://github.com/tailscale/tailscale/blob/main/control/controlbase/handshake.go) are references. Test an authorized peer over WireGuard and a denied peer; report direct versus [DERP](https://tailscale.com/kb/1151/what-is-derp) separately. Do not equate registration with reachability, enable an exit node, advertise routes, or widen ACLs.
- **Remote management:** separate role-checked, scoped configuration operations from release installation. No raw memory writes, unauthenticated console, or implicit trust based on LAN origin. Require versioned configuration validation and reboot persistence. Firmware is changed offline in mapped, intentional slots, then installed through an authenticated OTA channel. Verify a signed release against a pinned signing root and enforce target and version compatibility before staging. Use a distinct enrollment identity and release verification key. Hashes in the ROM image alone are not signatures.
- **Recovery:** retain an immutable recovery entry and known-good A/B slot. Make a pending boot commit only after a bounded self-test. Prove rollback under corrupted images, a failed first boot and interrupted transfer before any rollout. [Espressif's OTA documentation](https://docs.espressif.com/projects/esp-idf/en/v5.5/esp32s3/api-reference/system/ota.html) is a behavior reference, not an implementation dependency for this design. The current ROM probe is not an A/B bootloader.

## Acceptance, in order

| Gate | Evidence required | Status |
|---|---|---|
| ROM boot / UART in lab | SPI ROM loading + byte-exact UART marker + MMIO trace; mutated checksum rejected | **PASS in Espressif QEMU** |
| Physical bring-up | UART with actual Xtensa execution; distinguish ROM RAM load from cold SPI boot | **PASS** on the N16R8: ROM RAM load of the 160-byte probe, and cold SPI boot of the mapped [node flash set](node/) (owner-approved write; blank board backed up first). |
| Wi-Fi / IP | Supported radio boundary established; real association, authenticated network join, IP and recovery after reconnect | **PASS (ported stack, by import)**: WPA2 association and DHCP on the home network. Reconnect and wrong-credential tests not run. Radio code is imported, not re-authored. |
| Headscale / data | Own identity and node record, persistence on reboot, authorized traffic, denied peer | **PASS except denied peer**: node 47 `agujaesp-nopray`, identity kept across two reboots, direct WireGuard ping and API. Denied-peer test not run. |
| Remote config / OTA | Authz, versioned config, image signature check, successful upgrade and previous-image rollback | **Partial**: token-authorized API (401 without token) and OTA of a NOPRAY slot edit, confirmed valid past the rollback window. No image signature; rollback not exercised. |
| Release approval | Explicit production target, private provisioning, review of actual permissions/decoder coverage and deployment | Not authorized by a local successful boot |

## S3 open-MAC feasibility check

[Upstream esp-wifi-hal PR #23](https://github.com/esp32-open-mac/esp-wifi-hal/pull/23)
(closed, not merged; head `909be7021787f0dfb074072a9ea8563c511ba137`) reports real S3 WPA2 and DHCP
with a replacement MAC, but explicitly retains binary/ROM dependencies for PHY, automatic
ACK rates and other helpers. This is third-party evidence, not reproduced here. Espressif also
publishes [precompiled RF libraries](https://github.com/espressif/esp-phy-lib); their presence
does not establish that an independently authored radio is impossible. No ROM-only S3 radio
initialization and calibration contract has yet been validated for the NOPRAY image.

## Port track

1. Preserve the known baseline as an [exact container import](reference-import/). It is not a
   replacement executable and does not settle code coverage.
2. For each function, pin an existing object and symbol, enumerate relocations and data,
   recover the ABI and side effects, and compare separately. The first demonstrated
   [port of the gain-to-index helper](ports/phy-txbbgain/) has seven hardware vectors.
3. Recover the dependencies of the actual channel/RF initialization path next: caller ABI,
   `g_phyFuns` dispatch, `chip_v7_set_chan`, calibration state and timeout behavior.
   Do not call these on real hardware based only on static offsets. Reading fields and
   classifying side effects are not evidence of calibrated or compliant radio transmission.
4. Association and DHCP are still the first networking gate. Identity, data path and signed
   remote update remain subsequent and separate acceptance gates.

### Verified channel-path frontier

Static inspection of the same pinned archive: `phy_rfpll.o:set_chanfreq` loads
`g_phyFuns` from a relocation, dereferences it and calls the function pointer at
byte offset 92 (windowed `callx8`). A separate relocation names
`chip_v7_set_chan`, also in `phy_rfpll.o`. The archive defines `g_phyFuns`
as a four-byte BSS symbol in `phy_init.o`, which also contains
`register_chipv7_phy` and `register_chipv7_phy_init_param`. This is a live
state dependency, not a drop-in RF routine.

The next self-contained candidate is `phy_rfpll.o:rfpll_set_freq`: its observed
body performs integer arithmetic using a literal 3000, returns three bytes via
its caller-supplied pointer, and contains no observed MMIO instruction or external
call. This only separates the frequency arithmetic from programming the radio.
Do not treat register writes, analog calibration, initialization order or
association as recovered by that candidate.

A read-only probe on the physical rev 0.2 silicon, in the mask-ROM download state,
observed `[0x3fcef3d4] = 0x3fcef3d8` and
`[0x3fcef3d8 + 0x5c] = 0x40038fc8`. The S3 rev0 symbol ELF from Espressif's
`esp-rom-elfs/20260528` (SHA-256
`c0ce0f338d1de1bdc6efbef1591779a2a42c1ab7d759d3c6ae8ae63a7dd34cfd`) names this
address `rom_mhz2ieee`. The object loads the table pointer via `phy_get_romfuncs`,
whose ROM body returns the word at `0x3fcef3d4`; the inspected loader setup
overrides some entries but not offset `0x5c`. This resolves only the initial
frequency-to-channel mapping. It does not execute that function, prove the table
throughout runtime, or validate the second call `chip_v7_set_chan`. No RF calls,
flash writes, or other memory ranges were used to obtain this observation.
