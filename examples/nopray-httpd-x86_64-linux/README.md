# `nopray-httpd` (`v2`) — Hardened SSR HTTP/1.1 Reference Server

A **40,960-byte (`40 KiB`)** pure hand-authored x86-64 ELF64 HTTP/1.1 server (`DMCS-1.1`, tier `L2` W^X, `PT_GNU_STACK rw-`, **13 syscalls**, zero `libc`, zero OpenSSL, zero dynamic libraries).

## Security & Deployment Model
Because NOPRAY images have no compiler bounds checks, stack canaries, or ASLR, **network-exposed NOPRAY services MUST never face the internet directly**. Always run behind a reverse proxy (Caddy / Nginx) on loopback (`127.0.0.1`) with the exact seccomp allow-list generated from the audited bytes:

```bash
# 1. Verify the deployed binary matches the map byte-for-byte
./tools/nopray.sh audit examples/nopray-httpd-x86_64-linux/v2 --binary /path/to/nopray-httpd

# 2. Generate the exact systemd SystemCallFilter= from statically resolved syscall sites
./tools/nopray.sh seccomp examples/nopray-httpd-x86_64-linux/v2
```

Apply the full systemd sandbox template documented in [`references/SECURITY.md`](../../references/SECURITY.md#5-linux-deployment-checklist) (`DynamicUser=yes`, `MemoryDenyWriteExecute=yes`, `IPAddressDeny=any`, `IPAddressAllow=localhost`, `SystemCallFilter=...` without `SystemCallErrorNumber` so any unexpected syscall terminates with `SIGSYS`).
