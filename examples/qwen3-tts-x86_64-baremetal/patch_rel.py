#!/usr/bin/env python3
"""Pure arithmetic branch-displacement and RIP-relative address patcher for NOPRAY hex slots.

Never chooses or encodes opcodes: reads hand-authored hex bytes and `#=` intents, and patches
ONLY the displacement bytes (rel8, rel32, or RIP-relative disp32) where `#=` names a symbol
from nopray.toml (slot, label, state field) or a local `@label:` marker in the same slot.
"""
import re
import struct
import tomllib
from pathlib import Path

VM_DIR = Path(__file__).resolve().parent


def load_symbols(manifest: dict) -> dict[str, int]:
    syms: dict[str, int] = {}
    for s in manifest.get("slot", []):
        syms[s["name"]] = int(s["vaddr"])
        for lbl in s.get("label", []):
            syms[lbl["name"]] = int(s["vaddr"]) + int(lbl["offset"])
    for st in manifest.get("state", []):
        syms[st["name"]] = int(st["vaddr"])
    return syms


def parse_line_hex(line: str) -> list[int]:
    code_part = line.split("#", 1)[0].strip()
    if not code_part:
        return []
    return [int(t, 16) for t in code_part.split()]


def patch_slot(path: Path, base_vaddr: int, syms: dict[str, int], bits: int) -> int:
    lines = path.read_text().splitlines()
    # Pass 1: collect local `# @name:` markers at current address
    local_syms = dict(syms)
    cur = base_vaddr
    for line in lines:
        m_loc = re.match(r"^\s*#\s*@([A-Za-z_][A-Za-z0-9_]*)\s*:", line)
        if m_loc:
            local_syms[m_loc.group(1)] = cur
        raw = parse_line_hex(line)
        cur += len(raw)

    # Pass 2: patch rel8/rel32/rip-rel32 where `#=` references a symbol or @local
    out_lines = []
    cur = base_vaddr
    patched = 0
    for line in lines:
        raw = parse_line_hex(line)
        if not raw:
            out_lines.append(line)
            continue
        n = len(raw)
        next_ip = cur + n
        intent = line.split("#=", 1)[1].strip() if "#=" in line else ""
        if intent and bits == 64:
            m_br = re.match(r"^(call|jmp|j[a-z]{1,3})\s+@?([A-Za-z_][A-Za-z0-9_]*)$", intent)
            if m_br and m_br.group(2) in local_syms:
                sym = m_br.group(2)
                target = local_syms[sym]
                rel = target - next_ip
                if (raw[0] in (0xE8, 0xE9) and n == 5) or (raw[0] == 0x0F and 0x80 <= raw[1] <= 0x8F and n == 6):
                    raw[-4:] = list(struct.pack("<i", rel))
                    patched += 1
                    if sym not in syms:
                        # Local label: replace intent target with exact hex addr for nopray audit
                        line = line.split("#=", 1)[0].rstrip() + f"  #= {m_br.group(1)} {target:#x}"
                elif (raw[0] == 0xEB or (0x70 <= raw[0] <= 0x7F)) and n == 2:
                    if not (-128 <= rel <= 127):
                        raise ValueError(f"{path.name}@{cur:#x}: rel8 out of range ({rel}) for {sym}")
                    raw[1] = rel & 0xFF
                    patched += 1
                    if sym not in syms:
                        line = line.split("#=", 1)[0].rstrip() + f"  #= {m_br.group(1)} {target:#x}"
            m_mem = re.search(r"\[([A-Za-z_][A-Za-z0-9_]*)\]", intent)
            if m_mem and m_mem.group(1) in local_syms and n >= 6:
                target = local_syms[m_mem.group(1)]
                rel = target - next_ip
                raw[-4:] = list(struct.pack("<i", rel))
                patched += 1
        new_hex = " ".join(f"{b:02x}" for b in raw)
        if "#" in line:
            comment = line.split("#", 1)[1]
            pad = max(2, 34 - len(new_hex))
            out_lines.append(f"{new_hex}{' ' * pad}#{comment}")
        else:
            out_lines.append(new_hex)
        cur = next_ip
    path.write_text("\n".join(out_lines) + "\n")
    return patched


def main():
    m = tomllib.loads((VM_DIR / "nopray.toml").read_text())
    syms = load_symbols(m)
    total = 0
    for s in m["slot"]:
        if s["kind"] != "code":
            continue
        bits = int(s.get("bits", 64))
        p = VM_DIR / s["file"]
        c = patch_slot(p, int(s["vaddr"]), syms, bits)
        if c:
            print(f"[patch_rel] {s['name']}: patched {c} operand(s)")
        total += c
    print(f"[patch_rel] total patched: {total}")


if __name__ == "__main__":
    main()
