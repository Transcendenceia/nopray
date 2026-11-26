//! RISC-V (RV32/RV64 with the C extension) decoding from raw bits: lengths, validity of major
//! opcodes, control flow, `ecall` and the `a7` syscall register. Text comes from objdump when
//! available.

use super::{Flow, Insn, SyscallReg, sext};

/// `a7`, the Linux syscall-number register.
const A7: u32 = 17;

pub(super) fn decode(bytes: &[u8], addr: u64, rv64: bool) -> Vec<Insn> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let pc = addr + pos as u64;
        let Some(lo) = bytes.get(pos..pos + 2) else {
            out.push(Insn::new(
                pc,
                &bytes[pos..],
                "(truncated)".into(),
                Flow::Invalid,
            ));
            break;
        };
        let half = u16::from_le_bytes([lo[0], lo[1]]);
        if half & 3 != 3 {
            let mut insn = compressed(half, pc, rv64);
            insn.bytes = lo.to_vec();
            out.push(insn);
            pos += 2;
            continue;
        }
        let Some(full) = bytes.get(pos..pos + 4) else {
            out.push(Insn::new(
                pc,
                &bytes[pos..],
                "(truncated)".into(),
                Flow::Invalid,
            ));
            break;
        };
        let w = u32::from_le_bytes([full[0], full[1], full[2], full[3]]);
        let mut insn = standard(w, pc, rv64);
        insn.bytes = full.to_vec();
        out.push(insn);
        pos += 4;
    }
    out
}

fn standard(w: u32, pc: u64, rv64: bool) -> Insn {
    let opcode = w & 0x7f;
    let rd = (w >> 7) & 0x1f;
    let rs1 = (w >> 15) & 0x1f;
    let funct3 = (w >> 12) & 7;
    let i_imm = sext(u64::from(w >> 20), 12);
    let mut insn = Insn::new(pc, &[], String::new(), Flow::Next);
    let writes_rd = match opcode {
        0x03 | 0x13 | 0x17 | 0x33 | 0x37 | 0x2f => true,
        0x1b | 0x3b => {
            if !rv64 {
                insn.flow = Flow::Invalid;
                insn.text = "(bad) RV64-only opcode on RV32".into();
            }
            true
        }
        0x07 | 0x27 | 0x43 | 0x47 | 0x4b | 0x4f | 0x53 | 0x0f | 0x23 => false,
        0x63 => {
            let imm = (u64::from(w >> 31) << 12)
                | (u64::from((w >> 7) & 1) << 11)
                | (u64::from((w >> 25) & 0x3f) << 5)
                | (u64::from((w >> 8) & 0xf) << 1);
            insn.flow = Flow::CondJump(pc.wrapping_add(sext(imm, 13) as u64));
            false
        }
        0x6f => {
            let imm = (u64::from(w >> 31) << 20)
                | (u64::from((w >> 12) & 0xff) << 12)
                | (u64::from((w >> 20) & 1) << 11)
                | (u64::from((w >> 21) & 0x3ff) << 1);
            let target = pc.wrapping_add(sext(imm, 21) as u64);
            insn.flow = if rd == 0 {
                Flow::Jump(target)
            } else {
                Flow::Call(target)
            };
            true
        }
        0x67 => {
            insn.flow = match (rd, rs1, i_imm) {
                (0, 1, 0) => Flow::Return,
                (0, _, _) => Flow::IndirectJump,
                _ => Flow::IndirectCall,
            };
            true
        }
        0x73 => {
            match w {
                0x0000_0073 => insn.flow = Flow::Syscall,
                0x0010_0073 => {
                    insn.flow = Flow::Trap;
                    insn.note = Some("ebreak: breakpoint");
                }
                0x3020_0073 => {
                    insn.flow = Flow::Return;
                    insn.note = Some("mret: machine-mode return (bare metal)");
                }
                0x1050_0073 => insn.note = Some("wfi: wait for interrupt"),
                _ if funct3 != 0 => insn.note = Some("csr access"),
                _ => {}
            }
            funct3 != 0
        }
        _ => {
            insn.flow = Flow::Invalid;
            insn.text = format!("(bad) unknown major opcode {opcode:#04x}");
            false
        }
    };
    if writes_rd && rd != 0 {
        insn.writes.push(format!("x{rd}"));
        if rd == A7 {
            insn.syscall_reg = if opcode == 0x13 && funct3 == 0 && rs1 == 0 {
                SyscallReg::Const(i_imm as u64)
            } else {
                SyscallReg::Unknown
            };
        }
    }
    if opcode == 0x17 {
        insn.addr_refs
            .push(pc.wrapping_add((sext(u64::from(w >> 12), 20) << 12) as u64));
    }
    insn
}

fn compressed(h: u16, pc: u64, rv64: bool) -> Insn {
    let mut insn = Insn::new(pc, &[], String::new(), Flow::Next);
    if h == 0 {
        insn.flow = Flow::Invalid;
        insn.text = "(bad) c.unimp / all-zero halfword".into();
        return insn;
    }
    let q = h & 3;
    let f3 = (h >> 13) & 7;
    let b = |i: u16| u64::from((h >> i) & 1);
    let rd = u32::from((h >> 7) & 0x1f);
    let rs2 = (h >> 2) & 0x1f;
    let bit12 = (h >> 12) & 1;
    match (q, f3) {
        (1, 1 | 5) if f3 == 5 || !rv64 => {
            let imm = (b(12) << 11)
                | (b(11) << 4)
                | (b(10) << 9)
                | (b(9) << 8)
                | (b(8) << 10)
                | (b(7) << 6)
                | (b(6) << 7)
                | (b(5) << 3)
                | (b(4) << 2)
                | (b(3) << 1)
                | (b(2) << 5);
            let target = pc.wrapping_add(sext(imm, 12) as u64);
            insn.flow = if f3 == 5 {
                Flow::Jump(target)
            } else {
                Flow::Call(target)
            };
            if f3 == 1 {
                insn.writes.push("x1".into());
            }
        }
        (1, 6 | 7) => {
            let imm = (b(12) << 8)
                | (b(11) << 4)
                | (b(10) << 3)
                | (b(6) << 7)
                | (b(5) << 6)
                | (b(4) << 2)
                | (b(3) << 1)
                | (b(2) << 5);
            insn.flow = Flow::CondJump(pc.wrapping_add(sext(imm, 9) as u64));
        }
        (1, 2) => {
            let imm = sext((b(12) << 5) | u64::from((h >> 2) & 0x1f), 6);
            write_c(&mut insn, rd, Some(imm as u64));
        }
        (2, 4) if rs2 == 0 => match (bit12, rd) {
            (0, 1) => insn.flow = Flow::Return,
            (0, r) if r != 0 => insn.flow = Flow::IndirectJump,
            (1, 0) => {
                insn.flow = Flow::Trap;
                insn.note = Some("c.ebreak: breakpoint");
            }
            _ => {
                insn.flow = Flow::IndirectCall;
                insn.writes.push("x1".into());
            }
        },
        (1, 0 | 1 | 3) | (2, 0 | 2 | 3 | 4) => write_c(&mut insn, rd, None),
        _ => {}
    }
    insn
}

fn write_c(insn: &mut Insn, rd: u32, li_imm: Option<u64>) {
    if rd == 0 {
        return;
    }
    insn.writes.push(format!("x{rd}"));
    if rd == A7 {
        insn.syscall_reg = li_imm.map_or(SyscallReg::Unknown, SyscallReg::Const);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(ws: &[u32]) -> Vec<u8> {
        ws.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    #[test]
    fn li_a7_and_ecall() {
        // addi a7,zero,64 ; ecall ; jal ra,-8 ; ret (jalr x0,0(ra))
        let v = decode(
            &words(&[0x0400_0893, 0x0000_0073, 0xff9f_f0ef, 0x0000_8067]),
            0x1_0000,
            true,
        );
        assert_eq!(v[0].syscall_reg, SyscallReg::Const(64));
        assert_eq!(v[1].flow, Flow::Syscall);
        assert_eq!(v[2].flow, Flow::Call(0x1_0000));
        assert_eq!(v[3].flow, Flow::Return);
    }

    #[test]
    fn compressed_jump_and_ret() {
        // c.j .+0 (0xa001) ; c.jr ra (0x8082)
        let code = [0x01, 0xa0, 0x82, 0x80];
        let v = decode(&code, 0x2000, true);
        assert_eq!(v[0].flow, Flow::Jump(0x2000));
        assert_eq!(v[1].flow, Flow::Return);
    }
}
