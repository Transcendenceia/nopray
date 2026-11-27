# La Aguja S3 · byte-exact prebuilt application import

[NOPRAY manifest](aguja-s3-app.toml) for the already built `esp32s3-oct/aguja_iot.bin` v1.4.0
from the La Aguja checkout's generated `iot/prebuilt/esp32s3-oct` artifacts.
The input is **1,167,808 bytes**, SHA-256
`e8a1c9dddfbc31d574071f3460df2636a2bf13daca29c33af8b4211b20cafbf3`.
The embedded descriptor reports ESP-IDF 5.5.6 and ELF SHA-256
`c5e6bc5d3147b57a6a5a29899b836bf92f3a97887fb19dee384e94d15aba95e8`.
Neither binary bytes nor vendor dumps are stored in this repository.

This is **only an import at loader-segment granularity**. It preserves all bytes as 17 slots:
24-byte header, seven segment descriptors, seven segment bodies, one XOR/alignment slot,
and the appended digest. Do not infer function boundaries, all jump targets, writable aliases,
or authority to alter code from this layout. Loader segment permissions are descriptive,
not runtime W^X proof. The entry is `0x4037582c`, inside `iram_main`. This app
is not a standalone flash image: its source manifest specifies partition offset `0x20000`
and separate bootloader, partition table, and OTA metadata. No provisioning data was read
or copied to construct the map. Don't use it to overwrite a board.

With the named artifact available locally, from this directory:

```sh
B=/absolute/path/to/prebuilt/esp32s3-oct/aguja_iot.bin
OUT=$(mktemp -d)
printf '%s  %s\n' e8a1c9dddfbc31d574071f3460df2636a2bf13daca29c33af8b4211b20cafbf3 "$B" | sha256sum -c
nopray import --binary "$B" --manifest aguja-s3-app.toml --out "$OUT"
nopray build "$OUT"
cmp "$B" "$OUT/build/aguja-iot-s3-reference.bin"
nopray audit "$OUT" --binary "$B"
```

Observed: byte-identical rebuild and `R000`; 0 errors, 5 warnings, 3 infos.
The three code slots emit `X001`, and two IRAM segments emit `S001`.
The app image was not executed, modified or promoted. The
[separate gain routine](../ports/phy-txbbgain/) is a behavioral port with independent runtime tests.
