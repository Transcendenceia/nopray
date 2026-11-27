# Native-node design and acceptance gates

**Owner:** Angie · **Scope:** NOPRAY example, map, workflow and emulator lab; no writes to a real board, Headscale, ACLs or other hosts until the relevant gate is met. This is the work contract and handoff; unchecked rows below are not delivered features.

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
| Physical bring-up | Exact memory profile + matching original flash backup and reachable recovery; UART after cold boot | ROM identification passed (N16R8 memory profile); no backup, flash write, or NOPRAY boot on hardware yet |
| Wi-Fi / IP | Supported radio boundary established; real association, authenticated network join, IP and recovery after reconnect | Not implemented |
| Headscale / data | Own identity and node record, persistence on reboot, authorized traffic, denied peer | Not implemented |
| Remote config / OTA | Authz, versioned config, image signature check, successful upgrade and previous-image rollback | Not implemented |
| Release approval | Explicit production target, private provisioning, review of actual permissions/decoder coverage and deployment | Not authorized by a local successful boot |
