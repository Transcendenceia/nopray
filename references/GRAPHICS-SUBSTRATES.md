# Direct I/O & Graphics Substrates (`DMCS-1.1`)

> Status (2026-11-26): X11/Xwayland (`calc`, `nopray3d`, `nopray-doom`), native PipeWire/PulseAudio
> PCM audio (`nopray-doom`) and TCP/HTTP (`nopray-httpd`) are verified live in mapped x86-64 examples.
> Wayland `wl_shm`, `/dev/fb0` and ANSI TTY are documented here but no NOPRAY app has
> exercised them yet: treat their byte layouts as UNVERIFIED until one does.

How to talk directly to display servers, terminals, and networks from raw machine code with **zero client libraries** (`libX11`, `libwayland-client`, `libc`, `SDL`, `GTK`, `Qt` are all unnecessary).

---

## 1. Raw X11 / Xwayland Wire Protocol (`/tmp/.X11-unix/X0`)

Fastest path to a native desktop GUI window on Linux Xorg or Wayland+Xwayland (Hyprland, KDE, GNOME, Sway).

### Step A: Unix Socket Connect + 12-Byte Setup Handshake
- `sockaddr_un` (20 bytes): `01 00` (`AF_UNIX`) + `"/tmp/.X11-unix/X0\0"`.
- Setup request (12 bytes, no auth): `6c 00 0b 00 00 00 00 00 00 00 00 00` (`'l'` LSBFirst, proto `11.0`, auth_name_len `0`, auth_data_len `0`).
- Read 8-byte header into `setup_buf`:
  - `setup_buf[0] == 1` → Success! (`0` = Failed / needs Xauthority, `2` = Authenticate).
  - Remaining bytes = `u16 setup_buf[6] * 4`. Read them into `setup_buf + 8`.
- Parse Screen 0 fields (exact X11 core protocol offsets):
  - `res_base = u32 setup_buf[12]` → allocate `win_id = res_base + 1`, `gc_id = res_base + 2`.
  - `vendor_len = u16 setup_buf[24]`, `vpad = (vendor_len + 3) & ~3`.
  - `num_formats = u8 setup_buf[29]`.
  - `screen = setup_buf + 40 + vpad + num_formats * 8`:
    - `root_win = u32 screen[0]`
    - `root_visual = u32 screen[32]`
    - `root_depth = u8 screen[38]`

### Step B: Essential Window Requests (Sent Sequentially over the Socket)

1. **`CreateWindow` (Opcode `1`, 40 bytes = 10 dwords):**
   - `[0..3]`: `u8 1`, `u8 root_depth`, `u16 10`
   - `[4..7]`: `u32 win_id`, `[8..11]`: `u32 root_win`
   - `[12..15]`: `s16 x`, `s16 y`; `[16..19]`: `u16 W`, `u16 H`
   - `[20..23]`: `u16 border=0`, `u16 class=1 (InputOutput)`
   - `[24..27]`: `u32 root_visual`
   - `[28..31]`: `u32 value_mask = 0x00000802` (`CWBackPixel | CWEventMask`)
   - `[32..35]`: `u32 bg_pixel` (`0x00RRGGBB`)
   - `[36..39]`: `u32 event_mask` (`KeyPress=0x1 | ButtonPress=0x4 | ButtonRelease=0x8 | PointerMotion=0x40 | Exposure=0x8000`)

2. **`ChangeProperty` for Window Title (`WM_NAME`, Opcode `18`):**
   - `[0..3]`: `u8 18`, `u8 0 (Replace)`, `u16 (6 + ceil(len/4))`
   - `[4..7]`: `u32 win_id`, `[8..11]`: `u32 39 (WM_NAME)`, `[12..15]`: `u32 31 (STRING)`
   - `[16..19]`: `u8 8 (format)`, `3B pad`; `[20..23]`: `u32 len`
   - `[24..]`: ASCII title padded to 4-byte multiple.

3. **`ChangeProperty` for Fixed Floating Size (`WM_NORMAL_HINTS`, Opcode `18`, 96 bytes = 24 dwords):**
   - Crucial on tiling compositors (Hyprland, Sway, i3)!
   - `[0..3]`: `u8 18`, `u8 0`, `u16 24`
   - `[4..7]`: `u32 win_id`, `[8..11]`: `u32 40 (WM_NORMAL_HINTS)`, `[12..15]`: `u32 41 (WM_SIZE_HINTS)`
   - `[16..19]`: `u8 32 (format)`, `3B pad`; `[20..23]`: `u32 18`
   - Payload (72 bytes = 18 dwords): `d[0] = 0x30 (PMinSize|PMaxSize)`, `d[5] = min_w`, `d[6] = min_h`, `d[7] = max_w`, `d[8] = max_h`, rest `0`.

4. **`CreateGC` (Opcode `55`, 16 bytes) + `MapWindow` (Opcode `8`, 8 bytes):**
   - `CreateGC`: `37 00 04 00`, `gc_id`, `win_id`, `value_mask = 0`.
   - `MapWindow`: `08 00 02 00`, `win_id`.

### Step C: Flicker-Free Software Framebuffer Blitting (`PutImage`, Opcode `72`)
Maintain a `W × H × 4` BGRA pixel buffer in `.bss` (`0x00RRGGBB` per pixel).
Because standard X11 request length is `u16` in 4-byte units (max `262,140` bytes per request without `BIG-REQUESTS`), slice tall framebuffers into horizontal strips of height `SH` where `24 + W * SH * 4 <= 262,140`:
- 24-byte `PutImage` header:
  - `[0..3]`: `u8 72`, `u8 2 (ZPixmap)`, `u16 ((24 + W*SH*4) / 4)`
  - `[4..7]`: `u32 win_id`, `[8..11]`: `u32 gc_id`
  - `[12..15]`: `u16 W`, `u16 SH`
  - `[16..19]`: `s16 dst_x=0`, `s16 dst_y`
  - `[20..23]`: `u8 left_pad=0`, `u8 root_depth`, `u16 0`
- Send 24-byte header via `write_all`, then `W * SH * 4` raw pixel bytes via `write_all`.

### Step D: X11 32-Byte Events
Read exact 32-byte packets (`ev[0] & 0x7F`):
- `2` (`KeyPress`) / `3` (`KeyRelease`): `u8 ev[1]` = keycode, `u16 ev[28]` = modifier mask (bit 0 = Shift, bit 2 = Ctrl).
  - For smooth FPS held-key movement, set `event_mask = 0x804f` (`KeyPress | KeyRelease | ButtonPress | ButtonRelease | PointerMotion | Exposure`) and drain **all** pending events with non-blocking `poll(fd, 1, 0)` before ticking physics; X11 auto-repeat pairs (`KeyRelease` + `KeyPress` in the same frame) then leave the held-key bit set seamlessly.
  - Verified Linux/Xwayland evdev keycodes (CachyOS `xmodmap -pke`): `Esc=0x09`, `Q=0x18`, `W=0x19`, `E=0x1a`, `R=0x1b`, `Ctrl_L=0x25`, `A=0x26`, `S=0x27`, `D=0x28`, `Shift_L=0x32`, `M=0x3a`, `Space=0x41`, `Up=0x6f`, `Left=0x71`, `Right=0x72`, `Down=0x74`.
- `4` (`ButtonPress`) / `5` (`ButtonRelease`): `u8 ev[1]` = button (`1`=left, `3`=right, `4/5`=wheel), `s16 ev[24]` = `x`, `s16 ev[26]` = `y`.
- `6` (`MotionNotify`): `s16 ev[24]` = `x`, `s16 ev[26]` = `y`.
- `12` (`Expose`): redraw when `u16 ev[16] == 0` (final expose in batch).
- EOF (`read_exact == 0`): window closed by WM → `exit(0)`.

### Step E: Verified 640×400 FPS Recipes (`nopray-doom-x86_64-cachyos`)
- **4-Strip `PutImage` Math for `640×400` BGRA:** `4` horizontal strips of `640×100` (`256,000 B = 0x3e800` each). Header length is `(24 + 256,000) / 4 = 64,006 = 0xfa06` dwords, safely under X11's `0xffff` `u16` cap without `BIG-REQUESTS`.
- **Procedural Assets in `.bss`:** Keeping 7 × `64×64×4` textures (`114,688 B` at `0x510000`) and 6 × `32×32×4` sprites (`24,576 B` at `0x530000`) generated at startup in `PT_LOAD rw-` `.bss` (`p_filesz = 0`) keeps the ELF64 binary at `32 KiB` on disk while delivering 139 KiB of textured graphics.
- **16.16 Fixed-Point Floor/Ceiling Casting + 8-Insn SWAR RGB Fog:** Compute row distance and step once per scanline in SSE2 `f64`, convert `(floorX, floorY, stepX, stepY)` to 16.16 `i32` so the 640-pixel inner loop uses only shifts/masks (`(floorX >> 10) & 63`). Shade packed `0x00RRGGBB` by `shade ∈ [0..255]` in 2 multiplies with zero channel bleed:
  `rb = (((c & 0x00ff00ff) * shade) >> 8) & 0x00ff00ff; g = (((c & 0x0000ff00) * shade) >> 8) & 0x0000ff00; out = rb | g`.

---

## 2. Native Wayland Wire Protocol (`$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY`)

For pure Wayland environments **without** Xwayland. Remarkably clean binary protocol over `AF_UNIX` `SOCK_STREAM`!

### Wire Framing
Every message is 4-byte aligned:
- `u32 object_id`
- `u32 (msg_size_bytes << 16) | opcode`
- Arguments (integers, length-prefixed NUL-padded strings, `new_id`, and file descriptors passed out-of-band via `sendmsg` + `SCM_RIGHTS`).

### Minimal Window Bring-Up Sequence
1. Connect to `$XDG_RUNTIME_DIR/wayland-0` (or `wayland-1`).
2. `wl_display(1).get_registry(op=1, new_id=2)` + `wl_display(1).sync(op=0, new_id=3)`.
3. Read `wl_registry(2).global` events (`op=0`): capture global `name` for `"wl_compositor"`, `"wl_shm"`, `"xdg_wm_base"`, and `"wl_seat"`.
4. Bind them with `wl_registry(2).bind(op=0)` to fixed client IDs (`4=compositor`, `5=shm`, `6=xdg_wm_base`, `7=seat`).
5. Create surface: `wl_compositor(4).create_surface(op=0, new_id=8)` → `xdg_wm_base(6).get_xdg_surface(op=2, new_id=9, surface=8)` → `xdg_surface(9).get_toplevel(op=1, new_id=10)` → `wl_surface(8).commit(op=6)`.
6. Allocate shared pixel buffer via syscalls: `fd = memfd_create("fb", 1)` → `ftruncate(fd, W*H*4)` → `fb = mmap(0, W*H*4, PROT_READ|PROT_WRITE, MAP_SHARED, fd, 0)`.
7. Pass `fd` to compositor via `sendmsg` (`SCM_RIGHTS`) with `wl_shm(5).create_pool(op=0, new_id=11, size=W*H*4)` → `wl_shm_pool(11).create_buffer(op=0, new_id=12, offset=0, w=W, h=H, stride=W*4, format=1 /* XRGB8888 */)`.
8. On `xdg_surface(9).configure(op=0, serial)`: reply `xdg_surface(9).ack_configure(op=4, serial)`, render pixels into `fb`, send `wl_surface(8).attach(op=1, buf=12, 0, 0)` + `damage_buffer(op=9, 0, 0, W, H)` + `commit(op=6)`.
9. Always answer `xdg_wm_base(6).ping(op=0, serial)` with `pong(op=3, serial)`.

---

## 3. Linux Framebuffer (`/dev/fb0`) & ANSI Truecolor TTY

- **`/dev/fb0` (Kiosk / Console / Embedded):** `openat(AT_FDCWD, "/dev/fb0", O_RDWR)` → `ioctl(fd, 0x4600 /* FBIOGET_VSCREENINFO */, info_buf)` → `mmap` framebuffer → draw directly with the exact same `fill_rect` / `draw_glyph` primitives!
- **ANSI Truecolor TTY (Zero-GUI / SSH Dashboards):** Put stdin in raw mode via `ioctl(0, 0x5401 /* TCGETS */)` → clear `ICANON|ECHO` (`~0x0A`) → `ioctl(0, 0x5402 /* TCSETS */)`, emit `\x1b[2J\x1b[H` + `\x1b[48;2;R;G;Bm` half-block (`▀`) graphics and `poll` stdin + timers!

---

## 4. Direct Network Servers & Clients (`AF_INET` / `AF_UNIX`)

- **Minimal HTTP/1.1 Server (~1.5 KB total ELF64):**
  `socket(2, 1, 0)` → `setsockopt(fd, 1, 2, &one, 4)` (`SO_REUSEADDR`) → `bind(fd, sockaddr_in{2, htons(port), 0}, 16)` → `listen(fd, 16)` → loop: `accept` → `read` request → parse method/path → `write_all` `"HTTP/1.1 200 OK\r\n..."` → `close`.
- Combine with `poll(7)` (`timeout_ms = 16`) for real-time 60 FPS animations, clocks, system monitors, or networked GUI dashboards in a single thread with zero libraries.

---

## 5. Direct Real-Time PCM Audio via PipeWire / PulseAudio Native Socket (`/run/user/<uid>/pulse/native`)

**VERIFIED LIVE (2026-11-26)** on CachyOS / PipeWire 1.6.11 (`pipewire-pulse`) in `examples/nopray-doom-x86_64-cachyos/`: zero `libpulse`, zero `libalsa`, zero `fork`/`execve` subprocesses, and **zero extra syscalls** beyond the X11 set (`socket`, `connect`, `read`, `write`, `poll`, `exit`).

### Wire Framing (`pstream` 20-byte big-endian descriptor)
Every packet on `/run/user/<uid>/pulse/native` (`AF_UNIX`, `SOCK_STREAM`) starts with 20 big-endian bytes:
- `[0..3]`: `u32 payload_len`
- `[4..7]`: `u32 channel` (`0xFFFFFFFF` for control commands, or the stream channel returned by `CREATE_PLAYBACK_STREAM` for raw PCM payloads)
- `[8..15]`: `u64 offset = 0`
- `[16..19]`: `u32 flags = 0`

### 3-Packet Handshake (Protocol Version 15 — minimal tagstruct)
Tagstruct primitives: `'L'` + BE `u32`, `'t'` + NUL-terminated string, `'x'` + BE `u32 len` + raw bytes, `'1'`/`'0'` bool, `'N'` NULL, `'a'` + `u8 fmt` + `u8 ch` + BE `u32 rate` (sample spec), `'m'` + `u8 n` + `n` channel positions, `'v'` + `u8 n` + `n` BE `u32` volumes (`0x10000` = 100%), `'P'` + `(key_str, u32 val_len, arb val)`* + `'N'` (proplist).

1. **`PA_COMMAND_AUTH` (`cmd=8`, tag `1`, version `15`, 256-byte zero cookie):** on local PipeWire (`pipewire-pulse`), `SO_PEERCRED` authenticates the same UID without needing `openat` on `~/.config/pulse/cookie`. Read 20-byte descriptor + `bswap(len)` reply bytes; verify reply command at `payload[1..5] == 0x00000002` (`PA_COMMAND_REPLY`).
2. **`PA_COMMAND_SET_CLIENT_NAME` (`cmd=9`, tag `2`, proplist `application.name`):** read reply.
3. **`PA_COMMAND_CREATE_PLAYBACK_STREAM` (`cmd=3`, tag `3`, 100-byte payload):**
   - `sample_spec`: `fmt=3 (PA_SAMPLE_S16LE)`, `ch=1`, `rate=24000`
   - `channel_map`: `[0 (MONO)]`, `sink_idx=0xFFFFFFFF`, `sink_name=NULL`
   - `maxlength=16000`, `corked=false`, `tlength=3200` (~66 ms), `prebuf=1600` (~33 ms), `minreq=800` (400 samples = 1 frame @ 60 Hz), `syncid=0`, `cvolume=[0x10000]`
   - Protocol v12–v15 booleans (`7` false + `2` false + proplist `media.name="PCM"` + `2` false + `3` false).
   - Reply `payload[11..15]` contains the big-endian `stream_channel` — copy those 4 bytes directly into `[pstream_hdr + 4]` for all subsequent PCM frames.

### Streaming Loop & Clean Mute
- Each 60 FPS frame: drain any pending `PA_COMMAND_REQUEST` server packets using non-blocking `poll(audio_fd, 1, 0)` + `read` so the socket receive buffer never fills, then `write_all` `20 + 800 = 820` bytes (`400` S16LE mono samples).
- For click-free instant mute, keep sending `800` zero bytes (`rep stosq`) when muted rather than closing/corking the stream. If `connect` fails (headless hosts without PipeWire), store `audio_fd = -1` and skip audio writes cleanly.

---

## 6. Embedded `5×7` Bitmap Font Convention

Zero external font dependencies: store 7 bytes per glyph (`bit 4` = leftmost pixel, `bit 0` = rightmost pixel). `22` glyphs = `154` bytes; full printable ASCII (`95` glyphs) = only `665` bytes! Render cleanly at any integer scale `S` (`2×`, `3×`, `4×`) by invoking `fill_rect(x + col*S, y + row*S, S, S, color)`.
