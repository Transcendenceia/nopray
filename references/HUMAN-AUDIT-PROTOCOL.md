# Independent audit protocol (`DMCS-1.1`)

How a human or a second agent checks a NOPRAY deliverable without trusting the author. Read
[SECURITY.md](SECURITY.md) first: the audit establishes what the bytes do, not how they were made.

## 1. Identity: the delivered file is the mapped file

```bash
T=./tools/nopray.sh
$T audit APP_DIR --binary delivered.bin      # expect R000 and 0 errors
xxd -r -p APP_DIR/build/image.hex | cmp - delivered.bin && echo IDENTICAL
sha256sum delivered.bin                      # compare with build/manifest.lock.json
```

## 2. Independent tools (do not rely on nopray alone)

```bash
file delivered.bin
readelf -h -l -S delivered.bin     # ET_EXEC, PT_LOAD flags, PT_GNU_STACK RW, no PT_INTERP/PT_DYNAMIC
objdump -D -b binary -m i386:x86-64 -M intel --adjust-vma=0x400000 \
  --start-address=<first code slot> --stop-address=<rodata> delivered.bin
# aarch64: -m aarch64 ; riscv: -m riscv:rv64 (package binutils-multiarch)
strings -n 6 -t x delivered.bin     # every human-readable string and its offset
```

Compare the objdump listing with the slot listings and the syscall table in `build/MAP.md`.

## 3. Questions every review answers in writing

1. Which syscalls (or MMIO windows) can the program reach? Any site unresolved, and why?
2. Which slots handle untrusted input? For each buffer written from input: where is the bound
   check, and what is the limit?
3. Are there indirect branches? Where can their targets come from?
4. Is any segment writable and executable? Is the stack non-executable?
5. Does every cross-slot branch hit a declared entry? Any fall-through (`C015`) left?
6. Do the intents match? Any `I001` left, and is the explanation convincing? Does every
   symbolic field point at the symbol its intent names?
7. What does the audit's `origin` line say, and does each `reauthored` slot's `evidence`
   really compare it with the routine it replaces?
8. What was run, where, with which inputs (including malformed ones), and what was observed?

## 4. Runtime evidence

```bash
strace -f -qq ./delivered.bin            # Linux: compare with the syscall set in MAP.md
qemu-aarch64 -strace ./delivered.bin     # foreign ISA under qemu-user
```

GUI programs: screenshots of every state. Servers: transcript of normal, malformed, oversized
and slow requests, plus the service's restart count before and after.

## 5. Red flags

- `R001`, `E005` (interpreter/dynamic), `S001` on an exposed app, unexplained `C006`/`C007`.
- `E012` section headers or toolchain strings (`GCC:`, `clang`, `rustc`, Go build ID) in a
  binary claimed as hand-authored.
- Bytes owned by no slot (`G001`), stale comments (`A001`), orphan intents.
- Claims of "proven safe" or "mathematical certainty".

## 6. Compliance block for READMEs

```text
Standard:       DMCS-1.1, tier <L0-L3> (attested), exposure <local|lan|internet>
Target:         <isa> / <os> / <format>
Image:          <bytes> bytes, sha256 <digest>
Map:            nopray.toml, <n> slots; audit <e> errors / <w> warnings (tool nopray <version>)
Syscalls:       <list from audit>   (bare metal: MMIO windows)
Runtime check:  <where it ran, how, date>
Not verified:   <explicit list>
```
