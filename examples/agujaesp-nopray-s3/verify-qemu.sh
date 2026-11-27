#!/usr/bin/env bash
# Host-only test: no guest instruction encoding, no real device or network access.
set -euo pipefail
if [[ $# != 2 ]]; then
  echo "Usage: $0 /path/to/qemu-system-xtensa evidence-dir (new directory)" >&2
  exit 2
fi
qemu=$(realpath "$1")
app=$(cd -- "$(dirname -- "$0")" && pwd)
image="$app/build/agujaesp-s3-uart.bin"
mkdir -- "$2"
out=$(realpath "$2")
test "$(wc -c < "$image")" -eq 160
# Independently confirm the trailer over the already-authored bytes.
xor=239
for b in $(xxd -p -s 32 -l 88 -c 1 "$image"); do xor=$((xor ^ 16#$b)); done
[[ "$(printf '%02x' "$xor")" == "$(xxd -p -s 127 -l 1 "$image")" ]]
[[ "$(head -c 128 "$image" | sha256sum | cut -d' ' -f1)" == "$(xxd -p -s 128 -l 32 -c 32 "$image")" ]]
"$qemu" --version > "$out/version.txt"
sha256sum "$image" | cut -d' ' -f1 > "$out/image.sha256"
# QEMU must see an erased 4 MiB flash fixture. This is NOT a hardware image.
{ cat "$image"; head -c $((0x400000-160)) /dev/zero | tr '\000' '\377'; } > "$out/flash.bin"
cp -- "$out/flash.bin" "$out/flash-bad-checksum.bin"
# Flip the low bit of the stored checksum (no opcode change).
printf '%02x' "$((xor ^ 1))" | xxd -r -p | dd of="$out/flash-bad-checksum.bin" bs=1 seek=127 conv=notrunc status=none
run() {
  local input=$1 label=$2 status=0
  timeout 5s "$qemu" -M esp32s3 -display none -monitor none \
    -serial "file:$out/$label.uart" -nic none -no-reboot \
    -drive "file=$input,format=raw,if=mtd" \
    -d in_asm -trace "enable=memory_region_ops_write,file=$out/$label.trace" \
    > "$out/$label.stderr" 2>&1 || status=$?
  printf '%s\n' "$status" > "$out/$label.exit"
  # An explicit bounded timeout is normal for the intentional final spin.
  if [[ "$status" != 124 && "$status" != 0 ]]; then
    cat "$out/$label.stderr" >&2
    return 1
  fi
}
run "$out/flash.bin" good
run "$out/flash-bad-checksum.bin" bad-checksum
[[ "$(cat "$out/good.exit")" == 124 ]]
grep -Fq 'entry 0x403c8704' "$out/good.uart"
if grep -Fq 'SHA-256 comparison failed' "$out/good.uart"; then exit 1; fi
[[ "$(grep -Fxc 'NOPRAY-S3 OK' "$out/good.uart")" == 1 ]]
grep -Eq '^0x403c8704:[[:space:]]+l32r[[:space:]]+a2, 0x403c8700' "$out/good.trace"
grep -Eq '^0x403c8755:[[:space:]]+j[[:space:]]+0x403c8755' "$out/good.trace"
grep -F 'addr 0x60000000 value ' "$out/good.trace" | tail -n 13 \
  | sed -En "s/.* value 0x([0-9a-f]+) size 4 name 'esp_soc.uart'.*/0x\1/p" \
  | xargs printf '%02x' > "$out/good.marker.hex"
[[ "$(cat "$out/good.marker.hex")" == 4e4f505241592d5333204f4b0a ]]
[[ -s "$out/bad-checksum.uart" ]]
grep -iq 'checksum' "$out/bad-checksum.uart"
if grep -Fq 'NOPRAY-S3 OK' "$out/bad-checksum.uart"; then exit 1; fi
printf 'PASS: ROM boot, UART marker, 13 MMIO writes, checksum rejection\n'
