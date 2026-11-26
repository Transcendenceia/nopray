//! x86-64 decoding through `iced-x86` (full coverage).

use iced_x86::{
    Decoder, DecoderOptions, FlowControl, Formatter, Instruction, InstructionInfoFactory,
    IntelFormatter, Mnemonic, OpAccess, OpKind, Register,
};

use super::{DecodeOpts, Flow, Insn, MemRef, StateRef, SyscallReg};

fn reg_name(r: Register) -> String {
    format!("{r:?}").to_lowercase()
}

fn is_write(a: OpAccess) -> bool {
    matches!(
        a,
        OpAccess::Write | OpAccess::CondWrite | OpAccess::ReadWrite | OpAccess::ReadCondWrite
    )
}

fn parse_reg(name: &str) -> Option<Register> {
    Register::values().find(|r| reg_name(*r) == name.to_lowercase())
}

pub(super) fn decode(bytes: &[u8], addr: u64, opts: &DecodeOpts) -> Vec<Insn> {
    let state_reg = opts
        .state_register
        .as_deref()
        .and_then(parse_reg)
        .map(Register::full_register);
    let mut decoder = Decoder::with_ip(64, bytes, addr, DecoderOptions::NONE);
    let mut formatter = IntelFormatter::new();
    formatter.options_mut().set_hex_prefix("0x");
    formatter.options_mut().set_hex_suffix("");
    formatter.options_mut().set_uppercase_hex(false);
    formatter.options_mut().set_show_branch_size(false);
    formatter.options_mut().set_branch_leading_zeros(false);
    let mut factory = InstructionInfoFactory::new();
    let mut out = Vec::new();
    let mut ins = Instruction::default();
    while decoder.can_decode() {
        let pos = decoder.position();
        decoder.decode_out(&mut ins);
        let len = ins.len().max(1);
        let raw = &bytes[pos..(pos + len).min(bytes.len())];
        if ins.is_invalid() {
            out.push(Insn::new(ins.ip(), raw, "(bad)".into(), Flow::Invalid));
            continue;
        }
        let mut text = String::new();
        formatter.format(&ins, &mut text);
        let flow = flow_of(&ins);
        let mut insn = Insn::new(ins.ip(), raw, text, flow);
        if flow == Flow::Trap {
            insn.note = Some("trap / privileged / exception-raising instruction");
        }
        let info = factory.info(&ins);
        for ur in info.used_registers() {
            if is_write(ur.access()) {
                let full = ur.register().full_register();
                if full != Register::RIP && full != Register::None {
                    let n = reg_name(full);
                    if !insn.writes.contains(&n) {
                        insn.writes.push(n);
                    }
                }
            }
        }
        if ins.mnemonic() == Mnemonic::Push && ins.op0_kind() == OpKind::Register {
            insn.saves
                .push(reg_name(ins.op0_register().full_register()));
        }
        for um in info.used_memory() {
            let write = is_write(um.access());
            if um.base() == Register::None && um.index() == Register::None {
                insn.mem.push(MemRef {
                    addr: um.displacement(),
                    write,
                });
            } else if Some(um.base()) == state_reg && um.index() == Register::None {
                insn.state.push(StateRef {
                    offset: um.displacement() as i64,
                    write,
                });
            }
        }
        if ins.mnemonic() == Mnemonic::Lea && ins.op1_kind() == OpKind::Memory {
            if ins.is_ip_rel_memory_operand() {
                insn.addr_refs.push(ins.ip_rel_memory_address());
            } else if ins.memory_base() == Register::None && ins.memory_index() == Register::None {
                insn.addr_refs.push(ins.memory_displacement64());
            } else if Some(ins.memory_base()) == state_reg && ins.memory_index() == Register::None {
                insn.state.push(StateRef {
                    offset: ins.memory_displacement64() as i64,
                    write: false,
                });
            }
        }
        for op in 0..ins.op_count() {
            if matches!(
                ins.op_kind(op),
                OpKind::Immediate32 | OpKind::Immediate64 | OpKind::Immediate32to64
            ) {
                insn.addr_refs.push(ins.immediate(op));
            }
        }
        insn.syscall_reg = syscall_reg_effect(&ins, &insn.writes);
        out.push(insn);
    }
    out
}

fn flow_of(ins: &Instruction) -> Flow {
    if ins.mnemonic() == Mnemonic::Syscall {
        return Flow::Syscall;
    }
    match ins.flow_control() {
        FlowControl::Next => {
            if matches!(
                ins.mnemonic(),
                Mnemonic::Hlt | Mnemonic::Sysenter | Mnemonic::Sysexit | Mnemonic::Sysret
            ) {
                Flow::Trap
            } else {
                Flow::Next
            }
        }
        FlowControl::UnconditionalBranch => {
            if ins.op0_kind() == OpKind::NearBranch64 {
                Flow::Jump(ins.near_branch_target())
            } else {
                Flow::IndirectJump
            }
        }
        FlowControl::ConditionalBranch => Flow::CondJump(ins.near_branch_target()),
        FlowControl::Call => {
            if ins.op0_kind() == OpKind::NearBranch64 {
                Flow::Call(ins.near_branch_target())
            } else {
                Flow::IndirectCall
            }
        }
        FlowControl::IndirectBranch => Flow::IndirectJump,
        FlowControl::IndirectCall => Flow::IndirectCall,
        FlowControl::Return => Flow::Return,
        FlowControl::Interrupt | FlowControl::Exception | FlowControl::XbeginXabortXend => {
            Flow::Trap
        }
    }
}

fn syscall_reg_effect(ins: &Instruction, writes: &[String]) -> SyscallReg {
    let rax_written = writes.iter().any(|w| w == "rax");
    if !rax_written {
        return SyscallReg::Untouched;
    }
    let op0_rax = ins.op_count() >= 1
        && ins.op0_kind() == OpKind::Register
        && ins.op0_register().full_register() == Register::RAX;
    if op0_rax
        && ins.mnemonic() == Mnemonic::Mov
        && matches!(
            ins.op1_kind(),
            OpKind::Immediate32 | OpKind::Immediate64 | OpKind::Immediate32to64
        )
    {
        return SyscallReg::Const(ins.immediate(1));
    }
    if op0_rax
        && matches!(ins.mnemonic(), Mnemonic::Xor | Mnemonic::Sub)
        && ins.op1_kind() == OpKind::Register
        && ins.op1_register() == ins.op0_register()
        && matches!(ins.op0_register(), Register::EAX | Register::RAX)
    {
        return SyscallReg::Const(0);
    }
    SyscallReg::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_write_syscall_and_branch() {
        // mov eax,1 ; syscall ; jmp -7 (to the mov)
        let code = [0xb8, 1, 0, 0, 0, 0x0f, 0x05, 0xeb, 0xf7];
        let v = decode(&code, 0x0040_0000, &DecodeOpts::default());
        assert_eq!(v[0].syscall_reg, SyscallReg::Const(1), "{:?}", v[0]);
        assert_eq!(v[1].flow, Flow::Syscall);
        assert_eq!(v[2].flow, Flow::Jump(0x0040_0000));
    }

    #[test]
    fn maps_state_register_accesses() {
        // mov dword [r15+0x40], 1
        let code = [0x41, 0xc7, 0x47, 0x40, 0x01, 0, 0, 0];
        let opts = DecodeOpts {
            state_register: Some("r15".into()),
        };
        let v = decode(&code, 0x0040_0100, &opts);
        assert_eq!(
            v[0].state,
            vec![StateRef {
                offset: 0x40,
                write: true
            }]
        );
    }
}
