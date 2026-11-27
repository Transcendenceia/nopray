# Chapter 7 · Changing a program that already works

> The fixed map exists for this chapter. A change made by hand should cost the size of the
> change, not the size of the program.

## 7.1 Principle

In assembly or C, inserting one instruction shifts everything after it, and the toolchain
recomputes every offset. NOPRAY has no toolchain, so it removes the shift instead. Every
routine lives at a fixed address with slack after it. A change stays inside one slot, and
everything outside that slot is byte-identical. A reviewer can check that claim with `diff`.

Three things make a change safe:

- **Locality.** Edit one slot and keep its address and size. Free bytes are the budget.
- **Visible blast radius.** `context` names the callers, the state the slot touches and the
  fields pointing in and out. `diff` names what actually changed.
- **Derived bytes are recomputed, never patched by eye.** Checksums, digests and lengths are
  functions of other bytes. Each one is held in its own slot and `audit` recomputes it.

## 7.2 Procedure

1. **Copy before touching.** Create `app-v1.1/` beside `app/`. The old version is the oracle
   and the rollback.
2. **Orient:** run `nopray context app-v1.1 <slot>`. Read the contract, free bytes, callers,
   state touched and incoming fields.
3. **Edit the bytes and the intent together.** Write any reference to another slot as a field
   (`{rel32 @target}`) so it follows a move on rebuild.
4. **If the change outgrows the slot:**
   - move the routine to a free region and leave a jump, or
   - enlarge the slot and push its neighbour.
   Either way the map changes on purpose, and the change shows in `diff`.
5. **Recompute derived bytes** (XOR, SHA-256, sizes) with the tool or a reviewed script.
   Never type them from memory.
6. Run `annotate`, then `audit --strict`, then `diff app app-v1.1`. The diff must list only
   the slots you meant to change.
7. **Re-test every caller the diff lists**, then the whole program. Deliver the evidence and
   the diff.

## 7.3 Traps

- **Editing in place without a copy** leaves no oracle and no rollback.
- **Stale integrity slots.** The bytes changed and the checksum did not. `audit` reports
  `E015`/`E016` on an ESP image. On other carriers you only find out at boot.
- **Verifiers that pin the old output.** A test written for v1 can reject a correct v2. Read
  the test before concluding that the edit is wrong.
- **"Only one slot changed" with a moved address.** A hard-coded `abs64` to the old address
  still points there. Fields prevent this. Literal pointers need `C017` labels or a manual
  search.
- **Context drift between agents.** The agent that wrote v1 is gone. Phase B of the jstat
  benchmark gave v1 to a fresh agent, and the manifest and intents were the only memory it had.

## 7.4 Worked examples

**AgujaESP probe: a three-byte edit, chapter 4's fixture** ([README](../examples/agujaesp-nopray-s3/README.md)).
To change the UART marker's last character from `K` to `!`:

1. Change `32 a0 4b` to `32 a0 21` and update its intent. No address moves.
2. XOR bytes `20–77`, starting from `0xEF`. The new checksum is `0x78`.
3. Hash `00–7F` with SHA-256 and store the result in the last slot.

The shipped verifiers pin the original marker on purpose (trap 3).

**AgujaESP node: 27 bytes over the tailnet** ([EVIDENCE](../examples/agujaesp-nopray-s3/EVIDENCE.md#live-node--2026-11-27)).
This edit changed 27 data bytes in slot `drom` of an imported ESP-IDF app: the version
string became `1.4.0-nopray.1`, and the web title changed too. Then:

- The XOR byte went from `0x6b` to `0x1b`, and the appended SHA-256 was recomputed.
- The integrity computation first reproduced the original values exactly.
- `diff` listed exactly `drom`, `alignment_checksum` and `appended_digest`.
- The same edit with the old integrity slots fails with `E015` and `E016`.
- Install went through the board's authenticated OTA endpoint. The board ran the new
  version beyond its 180 s rollback window, and a serial read-back matched the map (`R000`).

This is chapter 5's route B paying off: a 1.17 MB program the agent did not write, changed by
hand with a 27-byte reviewable delta.

**jstat Phase B: maintenance by a stranger** ([BENCHMARKS](../references/BENCHMARKS.md) Part 2).
A fresh agent with no context extended the NOPRAY JSON validator to v2.

| | Phase A (write) | Phase B (extend) |
|---|---|---|
| NOPRAY, n = 3 | 348 s | 208 s |
| C, for scale | 124 s | 148 s |

For NOPRAY, the extension took less time than writing the original, although it was still
slower than C. The slot map, contracts and intents carried the knowledge across agents.

## 7.5 Problems

1. ★ **Change a string.** In a copy of `examples/calc-x86_64-cachyos`, change one message.
   - Accept: `diff` lists only the data slot.
   - Accept: all callers re-tested.
2. ★★ **Outgrow the slot.** Add an input check to a routine that has fewer free bytes than
   the check needs. Do it once by moving the routine and once by enlarging the slot.
   - Accept: both versions pass the original tests.
   - Accept: a written comparison of the two diffs.
3. ★★ **Derived bytes.** Repeat the probe edit with a different marker.
   - Accept: `audit` reports 0 errors.
   - Accept: the QEMU run shows the new marker.
   - Accept: the verifier is updated deliberately, with the reason recorded.
4. ★★★ **Hand-off.** Write a small program, then give only its directory to a fresh agent
   with a change request.
   - Accept: the agent succeeds without asking about the code.
   - Accept: you note every question it did ask and turn each into a contract or intent
     you should have written.

## 7.6 State of knowledge

- **Verified:**
  - in-place data edits with recomputed ESP integrity, installed over OTA on real hardware
  - maintenance across agents (jstat Phase B, n = 3)
- **Open:**
  - code (not data) edits inside an imported ESP-IDF app, which still lacks an Xtensa decoder
    ([TARGETS.md](../references/TARGETS.md))
  - automated caller re-testing from `diff` output
