#!/usr/bin/env python3
"""Host-only test for esptool 5.7.0. Reset the board, load RAM, observe UART, reset.
The only program uploaded is the pinned hex-authored probe. Never write flash.
"""

import argparse
import hashlib
from pathlib import Path
import time

import esptool
from esptool import cmds

PROFILES = {
    "uart": (
        "build/agujaesp-s3-uart.bin", 160,
        "2bac9d92778dc26866288ac50fc842654022dd02791322c122304c52b9b25b86",
        b"NOPRAY-S3 OK\n",
    ),
    "phy-gain": (
        "ports/phy-txbbgain/build/phy-txbbgain.bin", 240,
        "f4cd821a7f02e6d42214f50388889ccb1af10acc91d1260cf793ec94751050d9",
        b"P",
    ),
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", required=True, help="Explicit, idle USB-UART device")
    parser.add_argument("--image", type=Path)
    parser.add_argument("--profile", choices=PROFILES, default="uart")
    args = parser.parse_args()

    if esptool.__version__ != "5.7.0":
        parser.error("requires esptool 5.7.0 (serial capture uses its private transport attribute)")
    path, size, digest, marker = PROFILES[args.profile]
    data = (args.image or Path(__file__).parent / path).read_bytes()
    if len(data) != size or hashlib.sha256(data).hexdigest() != digest:
        parser.error("not the pinned probe image; board not touched")

    esp = esptool.detect_chip(args.port, baud=115200, connect_attempts=3)
    port = esp._port
    original_read = port.read
    samples = bytearray()

    def observe(size=1):
        chunk = original_read(size)
        # MEM_END can read into the marker before returning; keep its input too.
        samples.extend(chunk)
        return chunk

    try:
        if esp.CHIP_NAME != "ESP32-S3" or esp.IS_STUB:
            raise SystemExit("unexpected chip or transport; no RAM upload performed")
        port.read = observe
        cmds.load_ram(esp, data)
        port.timeout = 0.25
        until = time.monotonic() + 3
        while time.monotonic() < until:
            port.read(min(port.in_waiting, 256) or 1)
        if not samples.endswith(marker):
            raise SystemExit(f"FAIL: no marker in {len(samples)} sampled bytes (contents withheld)")
        print(f"PASS: hardware UART marker {marker.strip().decode()} / hex {marker.hex()}")
    finally:
        port.read = original_read
        try:
            esp.hard_reset()
        finally:
            port.close()


if __name__ == "__main__":
    main()
