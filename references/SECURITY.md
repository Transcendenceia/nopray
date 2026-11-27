# NOPRAY security model

## Contents

1. What changes when there is no compiler
2. What `nopray audit` proves, and what it cannot
3. Tiers and exposure gates
4. Writing input-handling code
5. Linux deployment checklist
6. Bare-metal and hardware checklist
7. Incident record

## 1. What changes when there is no compiler

- **Gone:** library and toolchain supply chain, dynamic loader, hidden runtime code. The whole
  program is a few KiB and every syscall site can be listed.
- **Moved, not removed:** trust moves to whoever wrote the bytes (an agent) and to the process
  that checked them. Hand-written parsers are exactly where memory-safety bugs live.
- **Lost by default:** ASLR (fixed-slot images load at a fixed address), stack canaries,
  bounds-checked types, CFI. A single out-of-bounds write is easier to exploit than in a
  hardened toolchain build, so containment (W^X, seccomp, sandbox, no secrets, loopback only)
  carries more of the weight.

## 2. What `nopray audit` proves, and what it cannot

Proves, for the exact bytes audited:
- the binary equals the map (`--binary`), every byte has an owner, headers match segments;
- code decodes, no branch lands in padding, data, mid-instruction or an undeclared entry;
- the complete statically resolvable syscall surface (x86-64, AArch64, RISC-V Linux);
- memory permissions: W^X, no writes into code or read-only segments for absolute addresses;
- intents: the bytes encode what the author said they encode;
- symbolic fields: each `{kind @symbol}` resolved to the declared symbol, fits its field, and
  the authored word left that field at zero (the tool fills displacement/address bits only);
- origin totals: how many used bytes the manifest declares authored, reauthored or imported.

Cannot prove:
- **how the bytes were produced.** Compiler output with headers rebuilt by hand passes every
  check (E012 only catches leftover section headers). Purity tiers are attestations.
- **functional correctness, bounds safety of register-relative accesses, termination.** Those
  need tests, fuzzing and review of every input path.
- **anything about Thumb, Xtensa or AVR code** (no built-in decoder yet: `X001`).
- **target facts**: a correct UART address or boot header is only as good as its source.
- **that `origin` is true.** Like purity tiers, it is a declaration; `import` stamps
  `imported` automatically, and `reauthored` needs `evidence` (`P001`), but only the evidence
  and review establish equivalence.

The `I001` hint flips single bits of the authored bytes and asks the decoder what they mean.
It locates a typo; it never maps a mnemonic to bits, so it is not an encoder.

Never write "mathematical certainty" or "proven safe". Write what was checked and how.

## 3. Tiers and exposure gates

| Tier | Meaning (attested) | Allowed exposure |
|---|---|---|
| L0 | Raw hex, byte converter only | local |
| L1 | L0 + map, annotated slots, lock file | local |
| L2 | L1 + W^X (no rwx segment, explicit non-executable stack) | local, lan, internet |
| L3 | L2 + declared binary assets (tables, bitmaps, audio) | local, lan, internet |

`lan`/`internet` exposure requires: tier L2+, `[syscalls]` allow-list, `audit --strict` clean
(or every warning justified in writing), `nopray seccomp` applied, sandbox below, a fuzz or
malformed-input run of every parser, and explicit operator approval before anything is published.

## 4. Writing input-handling code

- Every buffer has a declared capacity in the manifest (`[[state]]` note) and every write
  path compares against it before storing. Prefer one bounded copy helper used everywhere
  (`strcpy_bounded`, `html_escape` in nopray-httpd) over ad-hoc loops.
- Parse with explicit lengths, never by trusting terminators from the network.
- Reject early: method, path characters, `..`, NUL, oversized requests.
- Escape all text that reaches HTML (`& < > "`), and keep a strict CSP at the proxy.
- No secrets in the image or its working directory.

## 5. Linux deployment checklist

```ini
# drop-in: /etc/systemd/system/<unit>.service.d/10-nopray-hardening.conf
[Service]
DynamicUser=yes
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ReadOnlyPaths=/srv/<app>
CapabilityBoundingSet=
MemoryDenyWriteExecute=yes
LockPersonality=yes
RestrictNamespaces=yes
RestrictAddressFamilies=AF_INET          # only what the app uses
SystemCallArchitectures=native
SystemCallFilter=<output of nopray seccomp> # no SystemCallErrorNumber: violations kill (SIGSYS)
IPAddressDeny=any
IPAddressAllow=localhost                 # serve behind a reverse proxy
ProtectProc=invisible
ProcSubset=pid
UMask=0077
LimitCORE=0
```

Then: `systemd-analyze security <unit>`, check `Seccomp: 2` in `/proc/<pid>/status`, exercise
every route (including malformed and oversized input), confirm `NRestarts` did not grow, keep
the rollback command in the runbook. The reverse proxy adds TLS, rate limits, size limits and
security headers.

## 6. Bare-metal and hardware checklist

- Emulate first (QEMU machine, Espressif QEMU fork, simulators) and say which fidelity limits
  apply.
- Before flashing: explicit approval, exact device/port identified, current firmware backed up
  when the tool allows (`esptool read-flash`, `picotool save`), recovery path known (ESP ROM
  download mode, RP2040 BOOTSEL, spare SD card for Raspberry Pi).
- Watchdogs, clocks and pin mux come from primary sources cited in `[[mmio]].note`; anything
  else is UNVERIFIED.
- Never write eFuses, secure-boot or flash-encryption settings from a NOPRAY image.
- A bare-metal node on a tailnet or LAN cannot satisfy the Linux parts of the exposure gate
  (seccomp, W^X on writable IRAM, a native Xtensa decode). Deploy it only on the owner's explicit
  request, record that the gate is unmet, and state the compensating controls: API bound to the
  tailnet with a token, LAN and SoftAP off, no approved routes ([example](../examples/agujaesp-nopray-s3/node/)).

## 7. Incident record

- **2026-11-24, `nopray-httpd` public preview hardening:** a security review found an early
  internet-reachable hand-written HTTP server running without a seccomp syscall filter, plus
  early docs that overstated what static audits prove. Public access was immediately gated behind
  loopback/reverse-proxy only, the service received an exact 13-syscall `seccomp` allow-list
  derived from its audited bytes plus full `systemd` sandboxing (exposure score 3.4 → 1.1), and
  this honest security model replaced the earlier claims.
