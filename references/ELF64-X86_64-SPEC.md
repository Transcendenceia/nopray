# ELF64 & x86-64 Byte-Exact Reference (`DMCS-1.1`)

> AArch64 and RISC-V ABIs, alignment and syscall numbers: [TARGETS.md](TARGETS.md). Prefer
> copying the field-per-line header of `examples/hello-aarch64-linux/slots/0000_elf_header.hex`
> (change `e_machine` to `3e 00` for x86-64): `nopray audit` names every field and rejects
> misplaced ones.

Verified byte templates and encoding rules for Linux `x86_64` pure machine-code binaries.

---

## 1. Minimal ELF64 + Single `PT_LOAD` Header (120 Bytes, `0x000..0x077`)

Configure: `e_entry = 0x00400100`, `vaddr = 0x00400000`, `filesz = 0x00001000` (4,096B), `memsz = 0x000A0000` (640 KiB), `PF_R|PF_W|PF_X = 7`.

```hex
# ELF64 Header (64 bytes, 0x00..0x3F)
7f454c46 02010100 00000000 00000000   # magic, 64-bit, LE, ver=1, SysV
02003e00 01000000 00014000 00000000   # ET_EXEC, EM_X86_64, ver=1, e_entry=0x400100
40000000 00000000 00000000 00000000   # e_phoff=0x40, e_shoff=0
00000000 40003800 01000000 00000000   # flags=0, ehsize=64, phentsize=56, phnum=1, sh*=0

# PT_LOAD Program Header (56 bytes, 0x40..0x77) + 8B pad (0x78..0x7F)
01000000 07000000 00000000 00000000   # PT_LOAD=1, RWX=7, p_offset=0
00004000 00000000 00004000 00000000   # p_vaddr=0x400000, p_paddr=0x400000
00100000 00000000 00000a00 00000000   # p_filesz=0x1000, p_memsz=0xA0000
00100000 00000000 00000000 00000000   # p_align=0x1000, 8B pad to 0x80
```

### Hardened `W^X` Dual-`PT_LOAD` Variant (`DMCS-L2`, 176 B with 2 Phdrs / 232 B with `PT_GNU_STACK`)
Change `e_phnum` at offset `0x38` from `0100` to `0300` and emit three 56-byte program headers (`64 + 3 × 56 = 232 bytes = 0xE8`; allocate **`0x100` (`256 B`)** for `elf_header` so code starts cleanly at `0x400100`):
- Entry 0 (`0x40..0x77`): `p_flags = 05000000` (`PF_R|PF_X`), `p_offset = 0`, `p_vaddr = 0x400000`, `p_filesz = 0x8000` (or `0x1000`/`0x2000`), `p_memsz = p_filesz`, `p_align = 0x1000`.
- Entry 1 (`0x78..0xAF`): `p_flags = 06000000` (`PF_R|PF_W`), `p_offset = 0` (when `p_filesz = 0`, pure `.bss`), `p_vaddr = 0x408000` (next page boundary after segment 0), `p_filesz = 0`, `p_memsz = 0x180000`, `p_align = 0x1000`.
- Entry 2 (`0xB0..0xE7`): `PT_GNU_STACK` (`51 e5 74 64`), `p_flags = 06000000`, all other fields `0`, `p_align = 0x10` so the stack is explicitly non-executable (`S003` otherwise). No page is ever writable and executable at once.

---

## 2. Linux `x86_64` Syscall Reference

ABI: `rax = syscall_nr`, args in `rdi, rsi, rdx, r10, r8, r9`, invoke with `0f 05` (`syscall`). Return value in `rax` (negative `-4095..-1` = `-errno`). Clobbers `rcx` and `r11`.

| Nr (Dec / Hex) | Syscall | Signature | Common Use |
| :--- | :--- | :--- | :--- |
| `0` / `0x00` | `read` | `(fd, buf, count)` | Sockets, files, stdin |
| `1` / `0x01` | `write` | `(fd, buf, count)` | Sockets, files, stdout/TTY |
| `3` / `0x03` | `close` | `(fd)` | Close descriptors |
| `7` / `0x07` | `poll` | `(fds, nfds, timeout_ms)` | Multiplex sockets, timers, stdin |
| `9` / `0x09` | `mmap` | `(addr, len, prot, flags, fd, off)` | Framebuffers, Wayland `wl_shm`, big allocations |
| `16` / `0x10` | `ioctl` | `(fd, req, arg)` | Raw TTY (`TCGETS/TCSETS`), `/dev/fb0`, ALSA |
| `20` / `0x14` | `writev` | `(fd, iov, iovcnt)` | Scatter-gather writes |
| `35` / `0x23` | `nanosleep` | `(req_ts, rem_ts)` | Frame pacing / game loops |
| `41` / `0x29` | `socket` | `(domain, type, proto)` | `AF_UNIX=1`, `AF_INET=2`, `SOCK_STREAM=1`, `SOCK_DGRAM=2` |
| `42` / `0x2A` | `connect` | `(fd, addr, addrlen)` | X11/Wayland/TCP clients |
| `43` / `0x2B` | `accept` | `(fd, addr, addrlen)` | TCP/Unix servers |
| `46` / `0x2E` | `sendmsg` | `(fd, msghdr, flags)` | Wayland `SCM_RIGHTS` FD passing |
| `49` / `0x31` | `bind` | `(fd, addr, addrlen)` | Bind server port / socket |
| `50` / `0x32` | `listen` | `(fd, backlog)` | Listen for connections |
| `54` / `0x36` | `setsockopt` | `(fd, level, optname, val, len)` | `SOL_SOCKET=1`, `SO_REUSEADDR=2` |
| `60` / `0x3C` | `exit` | `(status)` | Clean process termination |
| `77` / `0x4D` | `ftruncate` | `(fd, length)` | Size `memfd_create` buffer for Wayland SHM |
| `228` / `0xE4` | `clock_gettime` | `(clk_id, timespec*)` | Monotonic timers (`CLOCK_MONOTONIC=1`) |
| `257` / `0x101` | `openat` | `(dirfd, path, flags, mode)` | `AT_FDCWD=-100`, open files/devices |
| `319` / `0x13F` | `memfd_create` | `(name, flags)` | Anonymous SHM FD for Wayland (`MFD_CLOEXEC=1`) |

---

## 3. Core `x86_64` Opcode Quick Reference

| Instruction | Hex Encoding | Size | Notes |
| :--- | :--- | :--- | :--- |
| `xor eax, eax` / `xor edi, edi` / `xor edx, edx` | `31 c0` / `31 ff` / `31 d2` | 2B | Zero-extends to full 64-bit register |
| `mov eax/edi/esi/edx/ecx/ebx/ebp, imm32` | `b8/bf/be/ba/b9/bb/bd <imm32>` | 5B | Zero-extends to 64-bit (`mov r15d, imm32` = `41 bf <imm32>`, 6B) |
| `mov r8d..r15d, imm32` | `41 b8..bf <imm32>` | 6B | Zero-extends to 64-bit |
| `mov rax, [r15 + disp8]` | `49 8b 47 <disp8>` | 4B | `disp8` MUST be `0x00..0x7F` (signed!) |
| `mov [r15 + disp8], rax` | `49 89 47 <disp8>` | 4B | For `disp8 = 0`: `49 89 07` (3B) |
| `mov eax, [r15 + disp8]` / `mov [r15 + disp8], eax` | `41 8b 47 <d8>` / `41 89 47 <d8>` | 4B | 32-bit state fields |
| `cmp dword [r15 + disp8], imm8` | `41 83 7f <d8> <imm8>` | 5B | Signed `imm8` |
| `mov dword [r15 + disp8], imm32` | `41 c7 47 <d8> <imm32>` | 8B | Direct state constant store |
| `call rel32` | `e8 <rel32>` | 5B | `rel32 = target - (addr + 5)` |
| `jmp rel8` / `jmp rel32` | `eb <rel8>` / `e9 <rel32>` | 2B / 5B | Unconditional jump |
| `je/jne/jl/jge/jle/jg/js/jns/jae/jo rel8` | `74/75/7c/7d/7e/7f/78/79/73/70 <rel8>` | 2B | Range `-128..+127` from `addr + 2` |
| `je/jne/jl/jae rel32` | `0f 84/85/8c/83 <rel32>` | 6B | Range `±2 GiB` from `addr + 6` |
| `push/pop rbx, rbp, r12, r13, r14` | `53/5b`, `55/5d`, `41 54/5c`, `41 55/5d`, `41 56/5e` | 1–2B | Callee-saved prologue/epilogue |
| `rep stosd` | `f3 ab` | 2B | Fast 32-bit pixel span fill (`rdi`, `eax`, `rcx`) |
| `cqo; idiv rcx` | `48 99` `48 f7 f9` | 5B | Signed 64-bit divide (`rdx:rax / rcx`) |
| `syscall` / `ret` / `nop` | `0f 05` / `c3` / `90` | 2B/1B/1B | Core control |

---

## 4. Battle-Tested Encoding Pitfalls (Mandatory Pre-Flight Checklist)

Every one of these traps causes silent bugs or crashes if overlooked:

1. **Signed `imm8` Sign-Extension Trap (`83` group):**
   - `83 c8 80` is `or eax, 0xFFFFFF80`, **NOT** `or eax, 0x80`!
   - Always use `0c 80` (`or al, 0x80`, 2B) or `0d 80 00 00 00` (`or eax, 0x80`, 5B) for constants `>= 0x80`.
2. **Signed `disp8` Wraparound (`>= 0x80`):**
   - `[r15 + 0x80]` encoded with `mod=01` (`disp8`) actually accesses `[r15 - 128]`!
   - Keep `[r15 + disp8]` state fields strictly in `0x00..0x7F`; load addresses `>= 0x80` with `mov reg32, 0x402080` or `mod=10` (`disp32`).
3. **`r12` and `rsp` Always Require a SIB Byte (`0x24`):**
   - `rm=4` signals SIB presence. `[r12 + disp8]` MUST be encoded with `rm=4` + SIB `24` (e.g. `lea edi, [r12 + 1]` = `41 8d 7c 24 01`).
4. **`rbp` and `r13` Cannot Use `mod=00, rm=5` Directly:**
   - `mod=00, rm=5` means `[rip + disp32]`! To access `[rbp]` or `[r13]`, ALWAYS use `mod=01` with `disp8 = 0x00` (e.g. `mov dword [rbp+0], imm32` = `c7 45 00 <imm32>`).
5. **Hardware `#DE` Trap on `idiv` Overflow:**
   - `idiv` raises `SIGFPE` not only on divisor `== 0`, but also when the signed quotient does not fit in 64 bits (including `INT64_MIN / -1` or wide `rdx:rax` products).
   - Always guard `divisor == 0` AND verify `|rdx| * 2 < |divisor|` before 128-by-64 `idiv`.
6. **Stack Alignment (`RSP % 16`):**
   - Linux `_start` enters with `RSP % 16 == 0`. Raw syscalls do not care about 16-byte alignment, **but** SSE2 aligned moves (`movaps`/`movdqa`) will `SIGSEGV` if `RSP` is misaligned after an odd number of `push`es. Keep frames 16-byte aligned if using SSE/AVX.
7. **Replacing a Running Binary (`ETXTBSY`):**
   - Opening a currently-executing ELF for write fails with `ETXTBSY`. Always `rm -f ./app` (or write `./app.new && mv ./app.new ./app`) when updating.
8. **Exact Stream Socket I/O (`read_exact` / `write_all`):**
   - Never assume one `read()` or `write()` transfers a whole packet or framebuffer strip. Always wrap both in 30-byte byte-counting loops.
9. **Missing or extra bytes inside a header:**
   - A program header is exactly 56 bytes (ELF64) or 32 bytes (ELF32): 2 x u32 then 6 x u64.
     Dropping one 8-byte field silently shifts every later field (a real case: `p_align` landed
     in `p_memsz`). Write one field per line with a `#= <field> <value>` intent; the audit
     reports `I002`/`I003`/`E013`.
10. **No ASLR at a fixed load address:**
    - `ET_EXEC` at `0x400000` loads at the same address on every run. Keep exposed programs at
      tier L2 with seccomp and a sandbox; see [SECURITY.md](SECURITY.md).
11. **`REX.R / REX.X / REX.B` Bit Confusion in SIB Addressing (`r8`–`r15`):**
    - `REX` byte = `0x40 | (W<<3) | (R<<2) | (X<<1) | B`: `R` extends ModRM `reg`, `X` extends SIB `index`, `B` extends SIB `base` (or ModRM `rm`).
    - Real bugs caught statically by `#=` intents in `nopray-doom`: `45 8d 1c 2e` decoded `lea r11d, [r14+rbp]` instead of `[r14+r13]` (needed `REX.X` → `47 8d 1c 2e`), and `4b 8d 34 b6` decoded `lea rsi, [r14+r14*4]` instead of `[rsi+r14*4]` (`REX.B` turned base `rsi` into `r14`; needed `4a 8d 34 b6`). Never skip `#=` intents.
12. **SSE2 16-Byte Alignment for `andpd`/`movaps` & High-Performance Idioms:**
    - `andpd xmm, [mem]` (e.g. `abs_mask = 0x7fffffffffffffff` × 2) faults with `SIGSEGV` unless `[mem]` is 16-byte aligned (`vaddr % 16 == 0`). Always place SSE2 masks at `0x10`-aligned labelled `rodata` offsets.
    - **Branchless hot loops:** in branch-heavy loops (e.g. Collatz), `shr rax, 1` (`48 d1 e8`) + `cmovb` (`48 0f 42 ...`) aligned to 16 bytes cuts CPU time ~33% and beats `gcc -O3 -march=native`.
    - **SWAR RGB shading (8 instructions, zero channel bleed):** shade packed `0x00RRGGBB` by `shade ∈ [0..255]` via `rb = (((c & 0x00ff00ff) * shade) >> 8) & 0x00ff00ff` and `g = (((c & 0x0000ff00) * shade) >> 8) & 0x0000ff00`, then `rb | g`.
13. **2-Byte VEX (`C5`) Cannot Encode `REX.B=1` or `REX.X=1` (`r8`–`r15` Memory Operands):**
    - `C5` implicitly hardwires `X'=1, B'=1, mmmmm=0F, W=0`. Using `c5 fc 11 4f 40` intending `vmovups [r15+0x40], ymm1` silently encodes `[rdi+0x40]`! Always switch to 3-byte VEX (`C4`) whenever a memory base or SIB index is `r8`–`r15` (`c4 c1 7c 11 4f 40`).
    - **Double-high SIB trap (`[r12..r15 + r8..r15*scale]`):** VEX Byte 2 bits are `R' X' B' mmmmm` (inverted!). When **both** SIB base and SIB index are `r8`–`r15`, **both** `X'` and `B'` must be `0` (`c4 81` for `0F`, NOT `c4 a1` which sets `B'=1` and turns `r12`/`r13` into `rsp`/`rbp`, nor `c4 c1` which sets `X'=1`). Caught 3× statically by `nopray audit` in `qwen3-tts-x86_64-baremetal`.
14. **VEX `vvvv` 1's-Complement and `R'` Bit Traps:**
    - VEX `vvvv` encodes `~reg` (`ymm0 → 1111`, `ymm3 → 1100`, `ymm14 → 0001`, `ymm15 → 0000`) and `R'` is `0` for `ymm8`–`ymm15`. Wrong `vvvv` silently reads/zeros the wrong SIMD register (e.g. `vaddps ymm3, ymm3, [r9]` needs `0x64`, `vxorps xmm3, xmm3, xmm3` needs `c5 e0`, `vdivps ymm9, ymm1, ymm9` needs `c4 41` with `R'=0, B'=0`).
15. **SIB `mod=00` + `base=5` (`rbp`/`r13`) Drops the Base Register:**
    - Just like non-SIB `mod=00, rm=5` means `[rip+disp32]`, SIB `mod=00` with `base=5` means `[index*scale + disp32]` with **no base register**! To encode `[rbp+rax]` or `[r13+r14*4]`, ALWAYS use `mod=01` with `disp8 = 0x00` (e.g. `48 8d 74 05 00` = `lea rsi, [rbp+rax]`, `c4 81 7a 10 44 b5 00` = `vmovss xmm0, [r13+r14*4]`).
16. **Subtle Opcode / ModRM Direction Traps Caught in AVX2 & 64-Bit Loops:**
    - `vmovaps` (`0F 28`) uses `pp=00` (`c5 f8 28` / `c5 fc 28`), NOT `F3` (`c5 fa`).
    - Two-operand `imul r64, r/m64` (`0F AF /r`) places the destination in `ModRM.reg` (`REX.R`), NOT `ModRM.rm` (`REX.B`) — `49 0f af ca` decodes `imul rcx, r10`, whereas `imul r9, rdx` is `4c 0f af ca`.
    - `add r/m64, r64` (`01 /r`) puts the source in `ModRM.reg` (`REX.R`) and destination in `ModRM.rm` (`REX.B`).
    - SIB scale field is `00=*1, 01=*2, 10=*4, 11=*8` (e.g. `[rdi+r10*2]` needs SIB `0x57`, not `0x97`).
17. **Bare-Metal AVX2/FMA/F16C Prerequisite & Fast Binary Streaming:**
    - In bare-metal x86-64 Long Mode, every VEX instruction traps with `#UD` until `CR0.EM=0`, `CR0.MP=1`, `CR4.{OSFXSR,OSXMMEXCPT,OSXSAVE}=1`, and `xsetbv(ecx=0, eax=7, edx=0)` (`XCR0 = 7`) execute.
    - For streaming multi-hundred-KiB binary outputs (like WAV PCM) out of a bare-metal KVM VM, use QEMU `isa-debugcon` (`port 0xE9`) with `cld; rep outsb` (`fc f3 6e`, 15 B routine) instead of byte-polled 16550A COM2 (`0x2F8`) to avoid hundreds of thousands of single-byte PIO VM-exits.
18. **Arithmetic Displacement Patchers vs `nopray annotate`:**
    - When using a pure arithmetic displacement patcher (Rule 1: never encodes opcodes, only patches `rel8`/`rel32`/`rip-disp32` from `#=` intents), parse intents with `line.split("#=", 1)[1]` rather than assuming `#=` starts right after the first `#`, because `nopray annotate` inserts `# 0xaddr disasm` ahead of `#=`.
