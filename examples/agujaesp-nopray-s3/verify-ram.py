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

IMAGE_SHA256 = "2bac9d92778dc26866288ac50fc842654022dd02791322c122304c52b9b25b86"
MARKER = b"NOPRAY-S3 OK\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", required=True, help="Explicit, idle USB-UART device")
    parser.add_argument("--image", type=Path, default=Path(__file__).parent / "build/agujaesp-s3-uart.bin")
    args = parser.parse_args()

    if esptool.__version__ != "5.7.0":
        parser.error("requires esptool 5.7.0 (serial capture uses its private transport attribute)")
    data = args.image.read_bytes()
    if len(data) != 160 or hashlib.sha256(data).hexdigest() != IMAGE_SHA256:
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
        while MARKER not in samples and time.monotonic() < until:
            port.read(min(port.in_waiting, 256) or 1)
        if MARKER not in samples:
            raise SystemExit(f"FAIL: no marker in {len(samples)} sampled bytes (contents withheld)")
        print(f"PASS: hardware UART marker {MARKER.strip().decode()} / hex {MARKER.hex()}")
    finally:
        port.read = original_read
        try:
            esp.hard_reset()
        finally:
            port.close()


if __name__ == "__main__":
    main()
