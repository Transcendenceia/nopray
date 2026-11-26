# Target Environment Discovery Protocol (`DMCS-1.0`)

Before synthesizing a single byte of machine code, an agent MUST fingerprint the exact target host. Machine code is bound directly to CPU ISA, kernel syscall ABI, binary container format, and system I/O protocols.

---

## 1. Universal One-Shot Probe (Linux / POSIX)

Run this read-only diagnostic command on the target host (local or over SSH):

```bash
uname -a
cat /etc/os-release 2>/dev/null | head -n 6
lscpu 2>/dev/null | grep -iE 'architecture|byte order|flags' | head -n 5
id
ls -la /tmp/.X11-unix/ 2>/dev/null
ls -la "/run/user/$(id -u)/" 2>/dev/null | grep -iE 'wayland|hypr|pipewire|pulse|bus'
ps -ef | grep -iE 'wayland|xwayland|kwin|hypr|sway|xorg|mutter|labwc|weston' | head -n 15
which xxd hexdump objdump readelf strace grim scrot hyprctl xdpyinfo xdotool ydotool 2>/dev/null
```

### Decision Matrix from Probe Output

| Observed Signal | Target Synthesis Choice |
| :--- | :--- |
| `x86_64`, Little Endian, Linux | **ELF64** (`e_machine = 0x3E`), Linux x86-64 `syscall` (`0f 05`) ABI. |
| `aarch64`, Little Endian, Linux | **ELF64** (`e_machine = 0xB7`), fixed 32-bit instructions, `svc #0` (`01 00 00 d4`), `x8` syscall nr. |
| `riscv64`, Little Endian, Linux | **ELF64** (`e_machine = 0xF3`), 32-bit base RV64I, `ecall` (`73 00 00 00`), `a7` syscall nr. |
| `/tmp/.X11-unix/X0` present & `Xwayland` has **no `-auth`** (or `xhost` permits local user) | **Raw X11 over Unix socket** — simplest, zero-auth graphical path (~500 bytes setup). |
| `/run/user/<uid>/pulse/native` present (`pipewire-pulse` / PulseAudio) | **Raw PulseAudio native protocol v15 over Unix socket** — zero-library, zero-subprocess real-time PCM audio using the exact same 6 syscalls as X11 (verified live on PipeWire 1.6.11 with 256-byte zero cookie via `SO_PEERCRED`). Prefer over `/dev/snd/pcm*` when PipeWire holds the hardware device. |
| `/tmp/.X11-unix/X0` requires `-auth` | Parse `$XAUTHORITY` (`~/.Xauthority`) in machine code (`openat`/`read`) and embed MIT-MAGIC-COOKIE-1 in the X11 setup request, **or** target native Wayland. |
| Only `$XDG_RUNTIME_DIR/wayland-*` (no Xwayland) | **Native Wayland** (`wl_display`, `wl_registry`, `wl_compositor`, `wl_shm`, `xdg_wm_base`) over Unix socket + `memfd_create` + `SCM_RIGHTS`. |
| Headless / VT / Embedded Linux with `/dev/fb0` | **Linux Framebuffer** (`openat("/dev/fb0")` + `ioctl(FBIOGET_VSCREENINFO)` + `mmap`). |
| Pure SSH / Server / Container (no display) | **ANSI Truecolor TTY** (`\x1b[38;2;R;G;Bm` + `ioctl(TCGETS/TCSETS)` raw mode) or **Network / HTTP / Daemon** (`socket`, `bind`, `listen`, `accept4`, `epoll`). |

---

## 2. Display & Window Manager Sub-Probe (GUI Targets)

When targeting a graphical desktop, check visual depth, byte order, WM behavior, and keyboard layout:

```bash
# X11 / Xwayland capabilities (depth, pixel format, max request size)
DISPLAY=:0 xdpyinfo | head -n 35

# Active keyboard keycodes (critical for non-US layouts like es, fr, de!)
DISPLAY=:0 xmodmap -pk 2>/dev/null | head -n 110

# Hyprland monitor geometry & scaling (if Hyprland is active)
SIG=$(ls /run/user/$(id -u)/hypr/ 2>/dev/null | head -n1)
[ -n "$SIG" ] && HYPRLAND_INSTANCE_SIGNATURE="$SIG" hyprctl monitors
```

### Critical Findings to Record Before Encoding

1. **Pixel Format & Depth:** Confirm `depth 24, bits_per_pixel 32` and `LSBFirst` (`0x00RRGGBB` in little-endian memory: `BB GG RR 00`). Always copy `root_depth` dynamically from the X11 setup reply rather than hardcoding `24` blindly, in case a host uses `32` or `16`.
2. **Tiling Window Managers (Hyprland, Sway, i3, bspwm):**
   - Tiling WMs stretch ordinary windows to fill half the monitor, distorting fixed-pixel interfaces.
   - **Solution:** Set `WM_NORMAL_HINTS` (atom `40`, type `WM_SIZE_HINTS` atom `41`, format `32`, 18 dwords) with `flags = PMinSize | PMaxSize (0x30)` and `min_w = max_w = W`, `min_h = max_h = H`. Hyprland and most WMs immediately float the window at exact `W×H`!
3. **Keyboard Layout Differences:**
   - Linux evdev keycodes are physical key positions (`evdev + 8`), NOT ASCII characters.
   - Example: on Spanish (`es`) keyboards, `+`/`*` is keycode `35`, `-` is `61`, `Shift+7` (`16`) is `/`, `Shift+0` (`19`) is `=`, while on US keyboards `=`/`+` is `21` and `-` is `20`.
   - **Best Practice:** Map BOTH top-row (multi-layout where non-conflicting) AND Numpad keycodes (`79–91`, `63`, `82`, `86`, `104`, `106`), and encode Shift-modified keys cleanly by ORing bit 7 (`keycode | 0x80`) in a unified lookup table.
4. **HiDPI / Fractional Scaling:**
   - Check monitor `scale` (`1`, `1.25`, `1.5`, `2`). Under Xwayland with scale `1`, logical pixels equal physical pixels (`grim -g "X,Y WxH"` matches `W×H` 1:1).
5. **Audio Substrate Sub-Probe (when targeting real-time PCM sound):**
   - Run `ls -la /run/user/$(id -u)/pulse/native /dev/snd/ && pactl info 2>/dev/null | head -n 15`.
   - On PipeWire desktops (`pipewire-pulse`), `/dev/snd/pcm*` is typically held by PipeWire while `/run/user/<uid>/pulse/native` mixes cleanly with all desktop audio and accepts a 256-byte zero cookie over `SO_PEERCRED` (see [GRAPHICS-SUBSTRATES.md](GRAPHICS-SUBSTRATES.md#5-direct-real-time-pcm-audio-via-pipewire--pulseaudio-native-socket-runuseruidpulsenative)).
   - Check mute keycodes too (`DISPLAY=:0 xmodmap -pke | grep -E ' (n|m|XF86AudioMute) '`; on Linux evdev: `M=0x3a`, `N=0x39`, `XF86AudioMute=0x79`).

---

## 3. Non-Linux Targets (Extension Path)

> Microcontrollers and bare metal: start from [TARGETS.md](TARGETS.md), which separates facts
> run or checked against primary sources from documented and UNVERIFIED ones. Also record
> `getconf PAGESIZE` on every Linux target and decide `exposure` (who can reach the program).

The DMCS-1.0 architecture generalizes beyond Linux ELF64:

| Platform | Binary Container | Syscall / OS Entry Mechanism |
| :--- | :--- | :--- |
| **Linux (`x86_64`/`aarch64`/`riscv64`)** | ELF64 (`PT_LOAD`) | Stable kernel syscall ABI (`syscall` / `svc #0` / `ecall`). **Gold standard for zero-library binaries.** |
| **FreeBSD / OpenBSD (`x86_64`)** | ELF64 (`OSABI = 9` / OpenBSD note) | Direct kernel syscalls (note: OpenBSD enforces `pinsyscalls` / `libc` syscall origin checks on modern releases). |
| **Windows (`x86_64`)** | PE32+ (`MZ` + `PE\0\0`, minimal Import Directory) | Windows NT syscall numbers change across builds; standard `DMCS-L3` approach embeds a tiny hand-encoded PE Import Table for `kernel32.dll` + `user32.dll` + `gdi32.dll` (`CreateWindowExA`, `StretchDIBits`) — zero compilers, still ~2–4 KB! |
| **macOS (`arm64` / `x86_64`)** | Mach-O 64-bit (`MH_MAGIC_64`) | Requires ad-hoc code signature (`codesign -s -`) on Apple Silicon; direct syscalls are unstable across macOS versions, so link `libSystem.B.dylib` via minimal Mach-O `LC_LOAD_DYLIB`. |
| **Bare-Metal / UEFI / Embedded** | Flat binary / PE32+ UEFI / Bootsector (`0x55AA`) | Direct MMIO, UEFI Boot Services table, or BIOS/GOP framebuffer. Not demonstrated yet: see TARGETS.md. |
