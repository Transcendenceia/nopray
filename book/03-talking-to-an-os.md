# Chapter 3 · Talking to an operating system

> With an OS, the machine is reached through one narrow door: the syscall instruction. Every
> library you are not using was only a long way of walking to that door.

## 3.1 Principle

A Linux user program needs nothing but syscalls. Graphics, audio and networking are not
libraries; they are **wire protocols** spoken over file descriptors:

- X11 is bytes over the Unix socket `/tmp/.X11-unix/X0`.
- PipeWire/PulseAudio is a native protocol over `/run/user/<uid>/pulse/native`.
- HTTP is bytes over a TCP socket.

So the whole OS surface of a NOPRAY program is a short list of syscall numbers, and the audit
resolves every site statically. That list is the program's attack surface, its seccomp
filter (chapter 8) and, when porting to bare metal, its list of obligations (chapter 4).

## 3.2 Procedure

1. Fingerprint the host: ISA, kernel, page size, display/audio/network substrate
   ([ENV-DISCOVERY.md](../references/ENV-DISCOVERY.md)).
2. Choose the protocol and read its wire format
   ([GRAPHICS-SUBSTRATES.md](../references/GRAPHICS-SUBSTRATES.md)); write the messages you
   need as labelled data, not as code that builds them byte by byte.
3. Put syscall wrappers in small helper slots (`write_all` handles short writes, `read_exact`
   handles partial reads). Load the syscall number with an immediate right before the syscall
   so the audit resolves it.
4. Declare `[syscalls] allow` with exactly the set used; `audit` fails on anything else.
5. Run under `strace -f` (or `qemu-<arch> -strace`) and compare with the syscall set in `MAP.md`.

## 3.3 Traps

- **Short reads and writes.** A socket returns fewer bytes than asked; a single `write` of a
  frame is a latent bug. Loop.
- **Blocking forever.** Interactive programs `poll` with a timeout; a blocked `read` on the X
  socket freezes the frame loop.
- **Syscall numbers differ per ISA.** `write` is 1 on x86-64 and 64 on AArch64/RISC-V. The
  tool's tables know both; your intent should name the syscall, not the number.
- **Endianness and padding in protocols.** X11 requests are padded to 4 bytes and carry their
  length in 4-byte units.
- **Undeclared dynamic syscall numbers.** A number that comes from a register the audit cannot
  resolve is `C007`; justify it or restructure.

## 3.4 Worked examples

| Example | Protocols and syscalls | Look at |
|---|---|---|
| [`calc-x86_64-cachyos`](../examples/calc-x86_64-cachyos/) | X11 over a Unix socket with 5 syscalls (`read`, `write`, `socket`, `connect`, `exit`) | `0080_read_exact.hex`, `00b0_write_all.hex`, `0960_render_and_blit.hex` |
| [`nopray-doom-x86_64-cachyos`](../examples/nopray-doom-x86_64-cachyos/) | X11 at ~60 FPS **plus** native PipeWire/PulseAudio PCM, still only 6 syscalls (adds `poll`) | `1080_main_loop.hex`, `3c80_blit_x11.hex`, `3d80_audio_engine.hex` |
| [`nopray-httpd-x86_64-linux/v2`](../examples/nopray-httpd-x86_64-linux/v2/) | HTTP/1.1 over TCP, 13 syscalls, every input-driven copy bounded | `02b0_handle_request.hex`, `06c0_strcpy_bounded.hex`, `06e0_html_escape.hex` |
| [`hello-aarch64-linux`](../examples/hello-aarch64-linux/) | The minimal surface: `write` + `exit` via `svc` with the number in `x8` | `0100_start.hex` |

## 3.5 Problems

1. ★ **`wc`.** Workload W2 of [BENCHMARK-PLAN.md](../references/BENCHMARK-PLAN.md): count
   bytes, lines and words of stdin on x86-64 Linux. Accept: output equals `wc` on at least
   five inputs including empty, no trailing newline, binary and a file larger than your buffer;
   `strace -c` shows only the declared syscalls.
2. ★★ **Same program, other ISA.** Port problem 1 to AArch64 or RISC-V with the same slot
   names and contracts. Accept: same outputs under `qemu-aarch64`/`qemu-riscv64`.
3. ★★★ **Static HTTP server.** Workload W3: GET/HEAD, three routes, 404/405, path traversal
   rejected. Accept: a request script with valid, malformed and oversized requests; every
   input-driven write has a bound check you can point to; then deploy it as in chapter 8.
4. ★★★ **A protocol nobody has written yet.** Draw a window with native Wayland (`wl_shm`).
   Accept: it runs on a Wayland session without Xwayland, and §3.6 below is updated.

## 3.6 State of knowledge

- Verified live: X11/Xwayland, PipeWire/PulseAudio PCM, TCP servers (x86-64); minimal
  `write`/`exit` on AArch64 and RISC-V under qemu-user.
- Documented, not yet demonstrated: native Wayland `wl_shm`, `/dev/fb0`, ALSA direct, TTY UIs.
