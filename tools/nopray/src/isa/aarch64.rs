//! `AArch64` decoding: validity and text via `yaxpeax-arm`, control flow decoded from raw bits.

use yaxpeax_arch::{Decoder, U8Reader};
use yaxpeax_arm::armv8::a64::InstDecoder;

use super::{Flow, Insn, MemRef, SyscallReg, sext};

/// Encoding of `svc #0`.
const SVC_MASK: u32 = 0xffe0_001f;

pub(super) fn decode(bytes: &[u8], addr: u64) -> Vec<Insn> {
    let decoder = InstDecoder::default();
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for (i, chunk) in bytes.chunks(4).enumerate() {
        let pc = addr + (i as u64) * 4;
        if chunk.len() < 4 {
            out.push(Insn::new(pc, chunk, "(truncated)".into(), Flow::Invalid));
            break;
        }
        let w = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let mut reader = U8Reader::new(chunk);
        let Ok(parsed) = decoder.decode(&mut reader) else {
            out.push(Insn::new(
                pc,
                chunk,
                format!("(bad) .inst {w:#010x}"),
                Flow::Invalid,
            ));
            continue;
        };
        let text = absolute_pc_rel(&parsed.to_string(), pc);
        let (flow, note) = flow_of(w, pc);
        let mut insn = Insn::new(pc, chunk, text, flow);
        insn.note = note;
        effects(w, pc, &mut insn);
        out.push(insn);
    }
    out
}

/// Rewrites `$+0x3c` / `$-0x8` (PC-relative, as printed by the decoder) to absolute addresses.
fn absolute_pc_rel(text: &str, pc: u64) -> String {
    let Some(pos) = text.find('$') else {
        return text.to_owned();
    };
    let rest = &text[pos + 1..];
    let (sign, digits) = match rest.as_bytes().first() {
        Some(b'+') => (1i64, &rest[1..]),
        Some(b'-') => (-1i64, &rest[1..]),
        _ => return text.to_owned(),
    };
    let hex: String = digits
        .trim_start_matches("0x")
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect();
    let consumed = digits.len() - digits.trim_start_matches("0x").len() + hex.len();
    let Ok(v) = i64::from_str_radix(&hex, 16) else {
        return text.to_owned();
    };
    let target = pc.wrapping_add((sign * v) as u64);
    format!("{}{target:#x}{}", &text[..pos], &rest[1 + consumed..])
}

fn flow_of(w: u32, pc: u64) -> (Flow, Option<&'static str>) {
    let rel = |imm: u64, bits: u32| pc.wrapping_add((sext(imm, bits) * 4) as u64);
    match w {
        _ if w & 0xfc00_0000 == 0x1400_0000 => {
            (Flow::Jump(rel(u64::from(w & 0x03ff_ffff), 26)), None)
        }
        _ if w & 0xfc00_0000 == 0x9400_0000 => {
            (Flow::Call(rel(u64::from(w & 0x03ff_ffff), 26)), None)
        }
        _ if w & 0xff00_0010 == 0x5400_0000 => {
            (Flow::CondJump(rel(u64::from((w >> 5) & 0x7ffff), 19)), None)
        }
        _ if w & 0x7e00_0000 == 0x3400_0000 => {
            (Flow::CondJump(rel(u64::from((w >> 5) & 0x7ffff), 19)), None)
        }
        _ if w & 0x7e00_0000 == 0x3600_0000 => {
            (Flow::CondJump(rel(u64::from((w >> 5) & 0x3fff), 14)), None)
        }
        _ if w & 0xffff_fc1f == 0xd65f_0000 => (Flow::Return, None),
        _ if w & 0xffff_fc1f == 0xd61f_0000 => (Flow::IndirectJump, None),
        _ if w & 0xffff_fc1f == 0xd63f_0000 => (Flow::IndirectCall, None),
        _ if w & SVC_MASK == 0xd400_0001 => (Flow::Syscall, None),
        _ if w & SVC_MASK == 0xd400_0002 => (Flow::Trap, Some("hvc: hypervisor call")),
        _ if w & SVC_MASK == 0xd400_0003 => (Flow::Trap, Some("smc: secure monitor call")),
        _ if w & 0xffe0_001f == 0xd420_0000 => (Flow::Trap, Some("brk: breakpoint")),
        _ if w & 0xffe0_001f == 0xd440_0000 => (Flow::Trap, Some("hlt: halting debug")),
        0xd69f_03e0 => (Flow::Return, Some("eret: exception return (bare metal)")),
        _ if w & 0xfff0_0000 == 0xd510_0000 => (Flow::Next, Some("msr: system register write")),
        _ => (Flow::Next, None),
    }
}

/// Approximate register, syscall-register and literal effects.
fn effects(w: u32, pc: u64, insn: &mut Insn) {
    let rd = w & 0x1f;
    let op0 = (w >> 25) & 0xf;
    // MOVZ (32/64-bit): sf 10 100101 hw imm16 Rd
    if w & 0x7f80_0000 == 0x5280_0000 {
        let hw = (w >> 21) & 3;
        let imm = u64::from((w >> 5) & 0xffff) << (16 * hw);
        if rd == 8 {
            insn.syscall_reg = SyscallReg::Const(imm);
        }
        insn.writes.push(format!("x{rd}"));
        return;
    }
    // ADR / ADRP
    if w & 0x1f00_0000 == 0x1000_0000 {
        let immlo = u64::from((w >> 29) & 3);
        let immhi = u64::from((w >> 5) & 0x7ffff);
        let imm = sext((immhi << 2) | immlo, 21);
        let target = if w & 0x8000_0000 == 0 {
            pc.wrapping_add(imm as u64)
        } else {
            (pc & !0xfff).wrapping_add((imm << 12) as u64)
        };
        insn.addr_refs.push(target);
        mark_write(insn, rd);
        return;
    }
    // LDR (literal): opc 011 V 00 imm19 Rt
    if w & 0x3b00_0000 == 0x1800_0000 {
        let target = pc.wrapping_add((sext(u64::from((w >> 5) & 0x7ffff), 19) * 4) as u64);
        insn.mem.push(MemRef {
            addr: target,
            write: false,
        });
        if w & 0x0400_0000 == 0 {
            mark_write(insn, rd);
        }
        return;
    }
    if matches!(insn.flow, Flow::Call(_) | Flow::IndirectCall) {
        insn.writes.push("x30".into());
        return;
    }
    let data_processing = op0 & 0b1110 == 0b1000 || op0 & 0b0111 == 0b0101;
    let load_store = op0 & 0b0101 == 0b0100;
    let is_load = load_store && (w >> 22) & 1 == 1;
    if (data_processing || is_load) && insn.flow == Flow::Next {
        mark_write(insn, rd);
    }
}

fn mark_write(insn: &mut Insn, rd: u32) {
    if rd == 31 {
        return;
    }
    if rd == 8 {
        insn.syscall_reg = SyscallReg::Unknown;
    }
    insn.writes.push(format!("x{rd}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(ws: &[u32]) -> Vec<u8> {
        ws.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    #[test]
    fn decodes_syscall_and_branches() {
        // movz x8,#64 ; svc #0 ; bl -8 ; cbz x2,+8 ; ret
        let code = words(&[
            0xd280_0808,
            0xd400_0001,
            0x97ff_fffe,
            0xb400_0042,
            0xd65f_03c0,
        ]);
        let v = decode(&code, 0x40_0000);
        assert_eq!(v[0].syscall_reg, SyscallReg::Const(64));
        assert_eq!(v[1].flow, Flow::Syscall);
        assert_eq!(v[2].flow, Flow::Call(0x40_0000));
        assert_eq!(v[3].flow, Flow::CondJump(0x40_0014));
        assert_eq!(v[4].flow, Flow::Return);
    }

    #[test]
    fn adr_resolves_forward_reference() {
        // adr x1, .+0xbc
        let v = decode(&words(&[0x1000_05e1]), 0x40_0144);
        assert_eq!(v[0].addr_refs, vec![0x40_0200]);
        assert!(v[0].text.contains("0x400200"), "{}", v[0].text);
    }
}
