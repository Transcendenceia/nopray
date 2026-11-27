#!/usr/bin/env bash
# Rebuild and audit the public part of the AgujaESP-NOPRAY flash set from the La Aguja 1.4.0
# esp32s3-oct artifacts. Writes nothing to a device. NVS (secrets) is generated privately.
#
#   ./reproduce.sh /path/to/La-Aguja/iot/prebuilt/esp32s3-oct [OUT_DIR]
#
# Needs `nopray` on PATH (or NOPRAY=/path/to/nopray) and `patch`.
set -euo pipefail

SRC=${1:?usage: reproduce.sh PREBUILT_DIR [OUT_DIR]}
OUT=${2:-$(mktemp -d)}
N=${NOPRAY:-nopray}
HERE=$(cd "$(dirname "$0")" && pwd)

(cd "$SRC" && sha256sum -c --quiet) <<'EOF'
bd0d1ceda20d3e5159236358ce99228c52f73b1f2f9b329a3f4de60c1bd53ff6  bootloader.bin
8f24b37d6d7e42b08a08be3e5634c964808d730f161ae17aa43bcec987eeb396  partition-table.bin
7d2c7ac4888bfd75cd5f56e8d61f69595121183afc81556c876732fd3782c62f  ota_data_initial.bin
e8a1c9dddfbc31d574071f3460df2636a2bf13daca29c33af8b4211b20cafbf3  aguja_iot.bin
EOF

map() { # name manifest input
  "$N" import --binary "$SRC/$3" --manifest "$2" --out "$OUT/$1" >/dev/null
  "$N" build "$OUT/$1" >/dev/null
  "$N" audit "$OUT/$1" --binary "$SRC/$3" | grep -E '^result'
}
map bootloader "$HERE/manifests/bootloader.toml" bootloader.bin
map partition-table "$HERE/manifests/partition-table.toml" partition-table.bin
map otadata "$HERE/manifests/otadata.toml" ota_data_initial.bin
map app-1.4.0 "$HERE/../reference-import/aguja-s3-app.toml" aguja_iot.bin

# The slot-level edit installed over the tailnet (version + web title, XOR and SHA-256 refreshed).
cp -a "$OUT/app-1.4.0" "$OUT/app-1.4.0-nopray.1"
rm -rf "$OUT/app-1.4.0-nopray.1/build"
(cd "$OUT/app-1.4.0-nopray.1" && patch -p1 -s < "$HERE/edits/1.4.0-nopray.1.patch")
"$N" build "$OUT/app-1.4.0-nopray.1" >/dev/null
"$N" audit "$OUT/app-1.4.0-nopray.1" | grep -E '^result'
"$N" diff "$OUT/app-1.4.0" "$OUT/app-1.4.0-nopray.1" | sed -n '3,8p'

want=cdfca3e6636b75c29079087801cb04eb1c98f1867bea11404a0c8a0ebd4e882c
got=$(sha256sum "$OUT/app-1.4.0-nopray.1/build/aguja-iot-s3-reference.bin" | cut -d' ' -f1)
[ "$got" = "$want" ] || { echo "unexpected 1.4.0-nopray.1 SHA-256 $got" >&2; exit 1; }
echo "OK: maps rebuilt in $OUT; 1.4.0-nopray.1 = $want"
