# Chapter 8 · Letting other people's bytes in

> A compiler gave you guard rails you never saw. Without one, you build the rails: bounded
> parsers inside, containment outside.

## 8.1 Principle

Removing the toolchain removes a supply chain and hidden runtime code. It also removes ASLR,
stack canaries, bounds-checked types and CFI ([SECURITY.md](../references/SECURITY.md) §1).
In a hand-written parser, one out-of-bounds write is easier to exploit than the same bug in a
hardened build. So, from the moment a program reads bytes it did not produce (network, files,
other processes):

- **Inside, every input is bounded.** Each buffer has a declared capacity, and every write
  compares against it. Parsing uses explicit lengths and rejects early.
- **Outside, the process is contained.** It runs with W^X, an exact syscall allow-list
  enforced by seccomp, a sandbox, no secrets, and loopback only behind a reverse proxy.
- **Exposure is a gate, not a default.** A tier and a checklist decide whether `lan` or
  `internet` is allowed. Publishing needs the owner's explicit approval.

Static audit is strong here: every syscall site is listable, so the allow-list is exact.
It does not prove the parser correct. Malformed-input runs do part of that work.

## 8.2 Procedure

1. Declare `[app].exposure` honestly. `local` is the default.
2. Reach tier L2 with no `rwx` segment and an explicit non-executable stack
   ([tiers](../references/SECURITY.md#3-tiers-and-exposure-gates)).
3. Declare every buffer's capacity in `[[state]]`. Route all copies through one bounded
   helper, and escape everything that reaches HTML.
4. Keep the `[syscalls]` allow-list exact. `nopray seccomp` turns it into
   `SystemCallFilter=` with no error number, so a violation kills the process.
5. Apply the systemd drop-in ([checklist](../references/SECURITY.md#5-linux-deployment-checklist)),
   bind to `127.0.0.1` only, and put TLS, rate limits, size limits and headers in the proxy.
6. Run every parser against malformed, oversized and truncated input. Check `Seccomp: 2` in
   `/proc/<pid>/status`, and check that `NRestarts` did not grow.
7. Ask the owner before anything becomes reachable from outside. Record the rollback command.

## 8.3 Traps

- **"The audit passed, so it is safe."** `audit` proves layout, permissions and syscall
  sites. It does not prove parser correctness.
- **`SystemCallErrorNumber=`** turns a kill into an error code the program may not handle.
  Leave it out.
- **Trusting terminators from the network.** Read lengths, then compare.
- **Secrets beside the image.** A working directory is part of the attack surface.
- **A bare-metal node cannot pass the Linux gate.** There is no seccomp, and its IRAM is
  writable. Deploy it only on request, record that the gate is unmet, and state the
  compensating controls ([§6](../references/SECURITY.md#6-bare-metal-and-hardware-checklist)).

## 8.4 Worked examples

**nopray-httpd v2** ([README](../examples/nopray-httpd-x86_64-linux/README.md)) is a 40 KiB
HTTP/1.1 server at tier L2, with 13 syscalls and no libc, OpenSSL or dynamic libraries. It
copies through `strcpy_bounded` and escapes through `html_escape`, and it runs only on
loopback behind Caddy.

Its history is the lesson ([incident record](../references/SECURITY.md#7-incident-record)).
An early public preview faced the internet with no seccomp filter, and its docs overstated
what static audit proves. The fix had three parts:

- gating it to loopback behind the proxy
- an exact 13-syscall allow-list derived from the audited bytes
- the full systemd sandbox, which took the exposure score from 3.4 to 1.1

The honest security model replaced the early claims.

**AgujaESP node** ([node](../examples/agujaesp-nopray-s3/node/)) is the bare-metal case of
the last trap. Its gate is recorded as unmet. The compensating controls are:

- the API bound to the tailnet, with a token (401 without it)
- LAN access and SoftAP off
- no approved routes

## 8.5 Problems

1. ★ **Exact allow-list.** Run `nopray seccomp` on `examples/calc-x86_64-cachyos`.
   - Accept: you explain every syscall in the list.
   - Accept: you name the slot that issues it.
2. ★★ **Bounded copy.** Write a bounded copy slot with a contract and a declared capacity.
   - Accept: input of exactly the capacity is stored.
   - Accept: capacity + 1 is rejected with no write past the end, proven with a guard byte.
3. ★★ **Malformed input.** Run httpd v2 locally on loopback and send 1,000 malformed and
   oversized requests.
   - Accept: no crash and no `SIGSYS`.
   - Accept: `NRestarts` is unchanged, and every response is a 4xx or a close.
4. ★★★ **Threat model.** For an exposed bare-metal device of your choice, list each unmet
   gate item, its compensating control and the evidence for it.
   - Accept: a reviewer can check every line without asking you.

## 8.6 State of knowledge

- **Verified:** loopback deployment behind Caddy, with an exact seccomp allow-list and the
  systemd sandbox (Linux x86-64).
- **Open:**
  - a fuzzing harness shipped with the tool
  - MMU/PMP-based W^X on bare metal
  - signed OTA images for MCU targets ([DESIGN](../examples/agujaesp-nopray-s3/DESIGN.md))
