//! Per-ISA linear-sweep decoding into a common, audit-oriented instruction model.

mod aarch64;
mod external;
mod riscv;
mod x86_64;

use crate::manifest::Isa;

/// Control-flow effect of one instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Next,
    Jump(u64),
    CondJump(u64),
    Call(u64),
    IndirectJump,
    IndirectCall,
    Return,
    Syscall,
    /// Traps, breakpoints, privileged or exception-raising instructions.
    Trap,
    Invalid,
}

impl Flow {
    #[must_use]
    pub fn direct_target(self) -> Option<u64> {
        match self {
            Flow::Jump(t) | Flow::CondJump(t) | Flow::Call(t) => Some(t),
            _ => None,
        }
    }

    /// True when execution cannot fall through to the next instruction.
    #[must_use]
    pub fn ends_block(self) -> bool {
        matches!(
            self,
            Flow::Jump(_) | Flow::IndirectJump | Flow::Return | Flow::Invalid
        )
    }
}

/// How an instruction touches the syscall-number register (`rax`, `x8`, `a7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallReg {
    Untouched,
    Const(u64),
    Unknown,
}

/// A statically resolved memory access or address computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemRef {
    pub addr: u64,
    pub write: bool,
}

/// An access relative to the declared state register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRef {
    pub offset: i64,
    pub write: bool,
}

#[derive(Debug, Clone)]
pub struct Insn {
    pub addr: u64,
    pub bytes: Vec<u8>,
    pub text: String,
    pub flow: Flow,
    pub syscall_reg: SyscallReg,
    /// Registers written (full-width names), best effort per ISA.
    pub writes: Vec<String>,
    /// Registers saved on the stack by this instruction (x86 `push`).
    pub saves: Vec<String>,
    /// Memory accesses with statically known absolute addresses.
    pub mem: Vec<MemRef>,
    /// Addresses materialised as values (lea, adr, immediates pointing into the image).
    pub addr_refs: Vec<u64>,
    pub state: Vec<StateRef>,
    /// Human note for traps or privileged instructions.
    pub note: Option<&'static str>,
}

impl Insn {
    fn new(addr: u64, bytes: &[u8], text: String, flow: Flow) -> Self {
        Self {
            addr,
            bytes: bytes.to_vec(),
            text,
            flow,
            syscall_reg: SyscallReg::Untouched,
            writes: Vec::new(),
            saves: Vec::new(),
            mem: Vec::new(),
            addr_refs: Vec::new(),
            state: Vec::new(),
            note: None,
        }
    }

    #[must_use]
    pub fn end(&self) -> u64 {
        self.addr + self.bytes.len() as u64
    }
}

/// How much of the code the decoder understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Coverage {
    /// Validity, control flow, syscalls, memory and register effects.
    Full,
    /// Validity (partial), control flow and syscalls; register effects approximate.
    ControlFlow,
    /// No decoder: bytes are opaque, external evidence required.
    None,
}

/// Decoding options shared by all ISAs.
#[derive(Debug, Clone, Default)]
pub struct DecodeOpts {
    /// Register holding the state base (e.g. `r15`).
    pub state_register: Option<String>,
    /// Optional x86 bitness override (`16`, `32`, or `64`); defaults to ISA native width.
    pub bitness: Option<u32>,
}

/// Decodes `bytes` placed at `addr` with a linear sweep.
#[must_use]
pub fn decode(isa: Isa, bytes: &[u8], addr: u64, opts: &DecodeOpts) -> (Vec<Insn>, Coverage) {
    match isa {
        Isa::X86_64 => (x86_64::decode(bytes, addr, opts), Coverage::Full),
        Isa::Aarch64 => (aarch64::decode(bytes, addr), Coverage::ControlFlow),
        Isa::Riscv64 => (riscv::decode(bytes, addr, true), Coverage::ControlFlow),
        Isa::Riscv32 => (riscv::decode(bytes, addr, false), Coverage::ControlFlow),
        Isa::Thumb | Isa::Xtensa | Isa::Avr => (Vec::new(), Coverage::None),
    }
}

/// Replaces empty instruction texts with `objdump` output when available.
pub fn annotate_external(isa: Isa, insns: &mut [Insn], bytes: &[u8], addr: u64) {
    if insns.iter().all(|i| !i.text.is_empty()) {
        return;
    }
    if let Some(map) = external::objdump_text(isa, bytes, addr) {
        for insn in insns.iter_mut().filter(|i| i.text.is_empty()) {
            if let Some(t) = map.get(&insn.addr) {
                insn.text.clone_from(t);
            }
        }
    }
}

/// External listing for ISAs without a built-in decoder (annotation only, never trusted).
#[must_use]
pub fn external_listing(isa: Isa, bytes: &[u8], addr: u64) -> Option<Vec<(u64, Vec<u8>, String)>> {
    external::objdump_listing(isa, bytes, addr)
}

/// Sign-extends the low `bits` of `v`.
#[must_use]
pub(crate) fn sext(v: u64, bits: u32) -> i64 {
    let shift = 64 - bits;
    ((v << shift) as i64) >> shift
}
