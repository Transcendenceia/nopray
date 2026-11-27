//! Symbolic fields: `{kind @symbol}` tokens inside slot files.
//!
//! A fixup fills **one declared address or displacement field** from a symbol declared in the
//! manifest (slot, label or `[[state]]` field). It never chooses an instruction:
//!
//! - byte kinds (`rel8`, `rel16`, `rel32`, `abs8`, `abs16`, `abs32`, `abs64`) stand for the
//!   field bytes themselves; the opcode and every other byte on the line stay authored;
//! - word kinds (`a64.*`, `rv.*`) carry the authored instruction word after `:` with the field
//!   bits left at zero; the tool ORs the field in and refuses a word whose field bits are set.
//!
//! Range, alignment and ISA are checked, so a displacement that does not fit fails the build
//! instead of wrapping silently.

use std::fmt;

use anyhow::{Result, bail};

use crate::manifest::{Isa, Manifest};

/// Field layouts the tool can fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Signed displacement `target - pc`, little-endian, `pc` = end of the byte line.
    Rel(u8),
    /// Unsigned absolute address, little-endian.
    Abs(u8),
    /// A64 `b`/`bl`: `imm26` words, bits 25:0.
    A64Imm26,
    /// A64 `b.cond`, `cbz`/`cbnz`, `ldr` literal: `imm19` words, bits 23:5.
    A64Imm19,
    /// A64 `tbz`/`tbnz`: `imm14` words, bits 18:5.
    A64Imm14,
    /// A64 `adr`: byte offset ±1 MiB split into immlo (30:29) and immhi (23:5).
    A64Adr,
    /// A64 `adrp`: 4 KiB page delta, same split as `adr`.
    A64Adrp,
    /// A64 `add` (immediate, unshifted): low 12 bits of the address, bits 21:10.
    A64AddLo12,
    /// RISC-V `jal`: J-type ±1 MiB.
    RvJal,
    /// RISC-V conditional branch: B-type ±4 KiB.
    RvBranch,
    /// RISC-V `auipc`: upper 20 bits of `target - pc` (rounded for the paired low part).
    RvPcrelHi20,
    /// RISC-V I-type low 12 bits of `target - pc` (pair with the `auipc`, usually `pc=-4`).
    RvPcrelLo12I,
    /// RISC-V S-type low 12 bits of `target - pc`.
    RvPcrelLo12S,
    /// RISC-V `lui`: upper 20 bits of the absolute address.
    RvHi20,
    /// RISC-V I-type low 12 bits of the absolute address.
    RvLo12I,
    /// RISC-V S-type low 12 bits of the absolute address.
    RvLo12S,
}

impl Kind {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "rel8" => Kind::Rel(1),
            "rel16" => Kind::Rel(2),
            "rel32" => Kind::Rel(4),
            "abs8" => Kind::Abs(1),
            "abs16" => Kind::Abs(2),
            "abs32" => Kind::Abs(4),
            "abs64" => Kind::Abs(8),
            "a64.imm26" => Kind::A64Imm26,
            "a64.imm19" => Kind::A64Imm19,
            "a64.imm14" => Kind::A64Imm14,
            "a64.adr" => Kind::A64Adr,
            "a64.adrp" => Kind::A64Adrp,
            "a64.add_lo12" => Kind::A64AddLo12,
            "rv.jal" => Kind::RvJal,
            "rv.branch" => Kind::RvBranch,
            "rv.pcrel_hi20" => Kind::RvPcrelHi20,
            "rv.pcrel_lo12_i" => Kind::RvPcrelLo12I,
            "rv.pcrel_lo12_s" => Kind::RvPcrelLo12S,
            "rv.hi20" => Kind::RvHi20,
            "rv.lo12_i" => Kind::RvLo12I,
            "rv.lo12_s" => Kind::RvLo12S,
            _ => return None,
        })
    }

    /// Bytes the field occupies in the slot.
    #[must_use]
    pub fn size(self) -> u64 {
        match self {
            Kind::Rel(n) | Kind::Abs(n) => u64::from(n),
            _ => 4,
        }
    }

    /// Whether the token carries an authored instruction word after `:`.
    #[must_use]
    pub fn is_word(self) -> bool {
        !matches!(self, Kind::Rel(_) | Kind::Abs(_))
    }

    fn pc_relative(self) -> bool {
        !matches!(
            self,
            Kind::Abs(_) | Kind::A64AddLo12 | Kind::RvHi20 | Kind::RvLo12I | Kind::RvLo12S
        )
    }

    fn isa_ok(self, isa: Isa) -> bool {
        match self {
            Kind::Rel(_) | Kind::Abs(_) => true,
            Kind::A64Imm26
            | Kind::A64Imm19
            | Kind::A64Imm14
            | Kind::A64Adr
            | Kind::A64Adrp
            | Kind::A64AddLo12 => isa == Isa::Aarch64,
            _ => matches!(isa, Isa::Riscv64 | Isa::Riscv32),
        }
    }

    /// Instruction-word bits the field owns; the authored word must leave them zero.
    fn mask(self) -> u32 {
        match self {
            Kind::Rel(_) | Kind::Abs(_) => 0,
            Kind::A64Imm26 => 0x03ff_ffff,
            Kind::A64Imm19 => 0x00ff_ffe0,
            Kind::A64Imm14 => 0x0007_ffe0,
            Kind::A64Adr | Kind::A64Adrp => 0x60ff_ffe0,
            Kind::A64AddLo12 => 0x003f_fc00,
            Kind::RvJal | Kind::RvPcrelHi20 | Kind::RvHi20 => 0xffff_f000,
            Kind::RvBranch | Kind::RvPcrelLo12S | Kind::RvLo12S => 0xfe00_0f80,
            Kind::RvPcrelLo12I | Kind::RvLo12I => 0xfff0_0000,
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Kind::Rel(n) => return write!(f, "rel{}", u32::from(*n) * 8),
            Kind::Abs(n) => return write!(f, "abs{}", u32::from(*n) * 8),
            Kind::A64Imm26 => "a64.imm26",
            Kind::A64Imm19 => "a64.imm19",
            Kind::A64Imm14 => "a64.imm14",
            Kind::A64Adr => "a64.adr",
            Kind::A64Adrp => "a64.adrp",
            Kind::A64AddLo12 => "a64.add_lo12",
            Kind::RvJal => "rv.jal",
            Kind::RvBranch => "rv.branch",
            Kind::RvPcrelHi20 => "rv.pcrel_hi20",
            Kind::RvPcrelLo12I => "rv.pcrel_lo12_i",
            Kind::RvPcrelLo12S => "rv.pcrel_lo12_s",
            Kind::RvHi20 => "rv.hi20",
            Kind::RvLo12I => "rv.lo12_i",
            Kind::RvLo12S => "rv.lo12_s",
        };
        f.write_str(s)
    }
}

/// `@symbol`, optionally `+N` / `-N` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymRef {
    pub symbol: String,
    pub addend: i64,
}

/// Where `pc` sits for a pc-relative field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pc {
    /// Byte kinds: end of the line (the next instruction). Word kinds: the word itself.
    Default,
    /// `pc=±N`: N bytes from the start of this field.
    Offset(i64),
    /// `pc=@symbol±N`.
    Symbol(SymRef),
}

/// One `{...}` token as written in a slot file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixup {
    /// Slot offset of the first byte of the field (or of the word).
    pub offset: u64,
    /// Slot offset of the end of the line holding the token (default `pc` for byte kinds).
    pub line_end: u64,
    pub kind: Kind,
    pub target: SymRef,
    pub pc: Pc,
    /// Authored instruction word (word kinds), little-endian as stored.
    pub word: Option<[u8; 4]>,
    /// The token exactly as written, re-emitted by `annotate`.
    pub token: String,
    /// 1-based source line, for error messages.
    pub line: usize,
}

fn parse_num(s: &str) -> Option<i64> {
    let (neg, body) = match s.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let v = match body.strip_prefix("0x") {
        Some(h) => i64::from_str_radix(h, 16).ok()?,
        None if !body.is_empty() && body.bytes().all(|b| b.is_ascii_digit()) => {
            body.parse().ok()?
        }
        None => return None,
    };
    Some(if neg { -v } else { v })
}

fn parse_symref(s: &str) -> Option<SymRef> {
    let body = s.strip_prefix('@')?;
    let split = body.find(['+', '-']).unwrap_or(body.len());
    let (name, rest) = body.split_at(split);
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if !valid {
        return None;
    }
    let addend = if rest.is_empty() { 0 } else { parse_num(rest)? };
    Some(SymRef {
        symbol: name.to_owned(),
        addend,
    })
}

/// Parses the inside of a `{...}` token (without braces) placed at `offset`.
pub fn parse_token(inner: &str, offset: u64, line: usize) -> Result<Fixup> {
    let token = format!("{{{}}}", inner.trim());
    let (spec, word_hex) = match inner.split_once(':') {
        Some((a, b)) => (a, Some(b)),
        None => (inner, None),
    };
    let mut parts = spec.split_whitespace();
    let Some(kind_text) = parts.next() else {
        bail!("line {line}: empty `{{}}` field");
    };
    let Some(kind) = Kind::parse(kind_text) else {
        bail!(
            "line {line}: unknown field kind `{kind_text}` (rel8/16/32, abs8/16/32/64, a64.imm26/imm19/imm14/adr/adrp/add_lo12, rv.jal/branch/pcrel_hi20/pcrel_lo12_i/pcrel_lo12_s/hi20/lo12_i/lo12_s)"
        );
    };
    let Some(target) = parts.next().and_then(parse_symref) else {
        bail!("line {line}: `{token}` needs a target `@symbol` (optionally `@symbol+N`)");
    };
    let mut pc = Pc::Default;
    for extra in parts {
        let Some(v) = extra.strip_prefix("pc=") else {
            bail!(
                "line {line}: unexpected `{extra}` in `{token}` (only `pc=±N` or `pc=@symbol` may follow the target)"
            );
        };
        pc = if let Some(r) = parse_symref(v) {
            Pc::Symbol(r)
        } else if let Some(n) = parse_num(v) {
            Pc::Offset(n)
        } else {
            bail!("line {line}: invalid `pc={v}` in `{token}`");
        };
    }
    if pc != Pc::Default && !kind.pc_relative() {
        bail!("line {line}: `{kind}` is absolute; `pc=` does not apply");
    }
    let word = match (kind.is_word(), word_hex) {
        (true, Some(h)) => {
            let digits: String = h.chars().filter(|c| !c.is_whitespace()).collect();
            let bytes = hex::decode(&digits).map_err(|_| {
                anyhow::anyhow!("line {line}: `{token}`: the authored word after `:` must be hex")
            })?;
            let Ok(w) = <[u8; 4]>::try_from(bytes.as_slice()) else {
                bail!("line {line}: `{token}`: `{kind}` needs exactly 4 authored bytes after `:`");
            };
            Some(w)
        }
        (true, None) => bail!(
            "line {line}: `{kind}` fills a field inside an instruction word: write `{{{kind} @target : <4 authored bytes with the field at zero>}}`"
        ),
        (false, Some(_)) => bail!(
            "line {line}: `{kind}` stands for the field bytes themselves; write the opcode bytes outside the braces"
        ),
        (false, None) => None,
    };
    Ok(Fixup {
        offset,
        line_end: offset,
        kind,
        target,
        pc,
        word,
        token,
        line,
    })
}

fn fits_signed(v: i64, bits: u32) -> bool {
    let lim = 1i64 << (bits - 1);
    (-lim..lim).contains(&v)
}

impl Fixup {
    /// Computes the field bytes for a slot placed at `slot_vaddr`.
    pub fn resolve(&self, m: &Manifest, slot_vaddr: u64) -> Result<Vec<u8>> {
        let ctx = || format!("line {}: `{}`", self.line, self.token);
        if !self.kind.isa_ok(m.target.isa) {
            bail!(
                "{}: `{}` does not apply to {}",
                ctx(),
                self.kind,
                m.target.isa
            );
        }
        let lookup = |r: &SymRef| -> Result<i64> {
            match m.intent_symbol(&r.symbol) {
                Some(a) => Ok(a as i64 + r.addend),
                None => bail!(
                    "{}: `@{}` is not a slot, label or state field declared in nopray.toml",
                    ctx(),
                    r.symbol
                ),
            }
        };
        let target = lookup(&self.target)?;
        let here = (slot_vaddr + self.offset) as i64;
        let pc = match &self.pc {
            Pc::Default if self.kind.is_word() => here,
            Pc::Default => (slot_vaddr + self.line_end) as i64,
            Pc::Offset(n) => here + n,
            Pc::Symbol(r) => lookup(r)?,
        };
        let d = target - pc;
        let range = |ok: bool, what: &str| -> Result<()> {
            if ok {
                Ok(())
            } else {
                bail!(
                    "{}: {what} (target {target:#x}, pc {pc:#x}, delta {d:+#x})",
                    ctx()
                )
            }
        };
        if let Kind::Rel(n) = self.kind {
            range(
                fits_signed(d, u32::from(n) * 8),
                "displacement does not fit",
            )?;
            return Ok(d.to_le_bytes()[..usize::from(n)].to_vec());
        }
        if let Kind::Abs(n) = self.kind {
            let ok = target >= 0 && (n == 8 || (target as u64) >> (u32::from(n) * 8) == 0);
            range(ok, "address does not fit the field")?;
            return Ok((target as u64).to_le_bytes()[..usize::from(n)].to_vec());
        }
        let word = u32::from_le_bytes(self.word.expect("word kinds carry a word"));
        if word & self.kind.mask() != 0 {
            bail!(
                "{}: the authored word {word:#010x} already sets bits of the `{}` field ({:#010x}); leave the field at zero, the tool fills only that field",
                ctx(),
                self.kind,
                self.kind.mask()
            );
        }
        let field = |v: i64, mask: u64| (v as u64 & mask) as u32;
        let a64_split = |v: i64| (field(v, 3) << 29) | (field(v >> 2, 0x7ffff) << 5);
        let hi20 = |v: i64| (v + 0x800) >> 12;
        let lo12 = |v: i64| v - (hi20(v) << 12);
        let s_type = |v: i64| (field(v >> 5, 0x7f) << 25) | (field(v, 0x1f) << 7);
        let bits = match self.kind {
            Kind::A64Imm26 | Kind::A64Imm19 | Kind::A64Imm14 => {
                let width = match self.kind {
                    Kind::A64Imm26 => 26,
                    Kind::A64Imm19 => 19,
                    _ => 14,
                };
                range(d % 4 == 0, "branch target is not word aligned")?;
                range(fits_signed(d / 4, width), "branch is out of range")?;
                let w = field(d / 4, (1 << width) - 1);
                if self.kind == Kind::A64Imm26 {
                    w
                } else {
                    w << 5
                }
            }
            Kind::A64Adr => {
                range(fits_signed(d, 21), "`adr` reaches only ±1 MiB")?;
                a64_split(d)
            }
            Kind::A64Adrp => {
                let pages = ((target & !0xfff) - (pc & !0xfff)) >> 12;
                range(fits_signed(pages, 21), "`adrp` reaches only ±4 GiB")?;
                a64_split(pages)
            }
            Kind::A64AddLo12 => field(target, 0xfff) << 10,
            Kind::RvJal => {
                range(d % 2 == 0, "jump target is not 2-byte aligned")?;
                range(fits_signed(d, 21), "`jal` reaches only ±1 MiB")?;
                (field(d >> 20, 1) << 31)
                    | (field(d >> 1, 0x3ff) << 21)
                    | (field(d >> 11, 1) << 20)
                    | (field(d >> 12, 0xff) << 12)
            }
            Kind::RvBranch => {
                range(d % 2 == 0, "branch target is not 2-byte aligned")?;
                range(fits_signed(d, 13), "branch reaches only ±4 KiB")?;
                (field(d >> 12, 1) << 31)
                    | (field(d >> 5, 0x3f) << 25)
                    | (field(d >> 1, 0xf) << 8)
                    | (field(d >> 11, 1) << 7)
            }
            Kind::RvPcrelHi20 => {
                range(
                    fits_signed(hi20(d), 20),
                    "pc-relative offset exceeds ±2 GiB",
                )?;
                field(hi20(d), 0xfffff) << 12
            }
            Kind::RvPcrelLo12I => field(lo12(d), 0xfff) << 20,
            Kind::RvPcrelLo12S => s_type(lo12(d)),
            Kind::RvHi20 => {
                range(
                    target >= 0 && hi20(target) >> 20 == 0,
                    "address needs more than 32 bits",
                )?;
                field(hi20(target), 0xfffff) << 12
            }
            Kind::RvLo12I => field(lo12(target), 0xfff) << 20,
            Kind::RvLo12S => s_type(lo12(target)),
            Kind::Rel(_) | Kind::Abs(_) => unreachable!("byte kinds return above"),
        };
        Ok((word | bits).to_le_bytes().to_vec())
    }
}

/// Writes every resolved field into `bytes` (the parsed slot, fields at their placeholders).
pub fn apply(bytes: &mut [u8], fixups: &[Fixup], m: &Manifest, slot_vaddr: u64) -> Result<()> {
    for f in fixups {
        let v = f.resolve(m, slot_vaddr)?;
        let at = f.offset as usize;
        bytes[at..at + v.len()].copy_from_slice(&v);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(isa: &str) -> Manifest {
        let text = format!(
            "[app]\nname = \"t\"\nversion = \"1\"\nstandard = \"DMCS-1.1\"\ntier = \"L2\"\nexposure = \"local\"\n\
             [target]\nisa = \"{isa}\"\nos = \"linux\"\nformat = \"raw\"\nbase = 0x400000\nentry = \"code\"\noutput = \"t\"\n\
             [[slot]]\nname = \"code\"\nkind = \"code\"\nvaddr = 0x400000\nsize = 0x100\nfile = \"c.hex\"\npurpose = \"t\"\n\
             [[slot.label]]\nname = \"loop\"\noffset = 0x40\n\
             [[slot]]\nname = \"msg\"\nkind = \"rodata\"\nvaddr = 0x401234\nsize = 0x10\nfile = \"m.hex\"\npurpose = \"t\"\n\
             [[slot]]\nname = \"far\"\nkind = \"rodata\"\nvaddr = 0x40500000\nsize = 0x10\nfile = \"f.hex\"\npurpose = \"t\"\n"
        );
        toml::from_str(&text).expect("test manifest")
    }

    fn word(kind: &str, target: &str, w: u32, at: u64, isa: &str) -> Result<u32> {
        let hex: Vec<String> = w.to_le_bytes().iter().map(|b| format!("{b:02x}")).collect();
        let mut f = parse_token(&format!("{kind} {target} : {}", hex.join(" ")), at, 1)?;
        f.line_end = at + 4;
        let v = f.resolve(&manifest(isa), 0x0040_0000)?;
        Ok(u32::from_le_bytes(v.try_into().expect("4 bytes")))
    }

    #[test]
    fn byte_fields_match_hand_encodings() {
        let m = manifest("x86-64");
        // `e8 rel32` at 0x400010, line ends at 0x400015: call loop (0x400040) = +0x2b.
        let mut f = parse_token("rel32 @loop", 1, 1).expect("token");
        f.offset = 0x11;
        f.line_end = 0x15;
        assert_eq!(f.resolve(&m, 0x0040_0000).expect("fits"), [0x2b, 0, 0, 0]);
        let mut back = parse_token("rel8 @code", 0x41, 1).expect("token");
        back.line_end = 0x42;
        assert_eq!(back.resolve(&m, 0x0040_0000).expect("fits"), [0xbe]);
        let abs = parse_token("abs32 @msg+2", 0, 1).expect("token");
        assert_eq!(
            abs.resolve(&m, 0x0040_0000).expect("fits"),
            [0x36, 0x12, 0x40, 0]
        );
        let mut far = parse_token("rel8 @far", 0, 1).expect("token");
        far.line_end = 2;
        assert!(
            far.resolve(&m, 0x0040_0000).is_err(),
            "rel8 overflow must fail"
        );
    }

    #[test]
    fn aarch64_fields_match_reference_encodings() {
        // b loop from 0x400000: +0x40 / 4 = 0x10 -> 0x14000010.
        assert_eq!(
            word("a64.imm26", "@loop", 0x1400_0000, 0, "aarch64").expect("b"),
            0x1400_0010
        );
        // cbz x0, loop from 0x400008: +0x38 / 4 = 14 -> imm19 << 5.
        assert_eq!(
            word("a64.imm19", "@loop", 0xb400_0000, 8, "aarch64").expect("cbz"),
            0xb400_01c0
        );
        // adr x1, msg from 0x400104: +0x1130 -> immlo 0, immhi 0x44c.
        assert_eq!(
            word("a64.adr", "@msg", 0x1000_0001, 0x104, "aarch64").expect("adr"),
            0x1000_8981
        );
        // adrp x1, far from 0x400104: page delta 0x40100.
        let adrp = word("a64.adrp", "@far", 0x9000_0001, 0x104, "aarch64").expect("adrp");
        assert_eq!(adrp, 0x9000_0001 | (0x40100 >> 2) << 5);
        assert_eq!(
            word("a64.add_lo12", "@msg", 0x9100_0021, 0, "aarch64").expect("add"),
            0x9108_d021
        );
        assert!(
            word("a64.imm26", "@msg+2", 0x1400_0000, 0, "aarch64").is_err(),
            "unaligned"
        );
        assert!(
            word("a64.imm26", "@loop", 0x1400_0001, 0, "aarch64").is_err(),
            "field must be zero"
        );
        assert!(
            word("a64.imm26", "@loop", 0x1400_0000, 0, "riscv64").is_err(),
            "wrong ISA"
        );
    }

    #[test]
    fn riscv_fields_match_reference_encodings() {
        // jal x0, +0x40 from 0x400000 -> 0x0400006f.
        assert_eq!(
            word("rv.jal", "@loop", 0x0000_006f, 0, "riscv64").expect("jal"),
            0x0400_006f
        );
        // beq x0, x0, +0x30 from 0x400010 -> imm[4:1]=8, imm[10:5]=1.
        assert_eq!(
            word("rv.branch", "@loop", 0x0000_0063, 0x10, "riscv64").expect("beq"),
            0x0200_0863
        );
        // hello-riscv64: auipc a1 at 0x400104 + addi a1,a1,60 -> 0x400140.
        let mut m = manifest("riscv64");
        m.slots[1].vaddr = 0x0040_0140;
        let mut hi = parse_token("rv.pcrel_hi20 @msg : 97 05 00 00", 0x104, 1).expect("hi");
        hi.line_end = 0x108;
        let mut lo = parse_token("rv.pcrel_lo12_i @msg pc=-4 : 93 85 05 00", 0x108, 2).expect("lo");
        lo.line_end = 0x10c;
        assert_eq!(
            hi.resolve(&m, 0x0040_0000).expect("hi"),
            [0x97, 0x05, 0x00, 0x00]
        );
        assert_eq!(
            lo.resolve(&m, 0x0040_0000).expect("lo"),
            [0x93, 0x85, 0xc5, 0x03]
        );
        // Rounding: a low part >= 0x800 borrows from the upper part.
        let hi2 = word("rv.hi20", "@msg+0x5cc", 0x0000_05b7, 0, "riscv64").expect("lui");
        let lo2 = word("rv.lo12_i", "@msg+0x5cc", 0x0005_8593, 4, "riscv64").expect("addi");
        let value = i64::from((hi2 & 0xffff_f000) as i32) + i64::from((lo2 as i32) >> 20);
        assert_eq!(value, 0x0040_1800);
    }

    #[test]
    fn malformed_tokens_are_rejected() {
        assert!(parse_token("rel32", 0, 1).is_err(), "missing target");
        assert!(parse_token("rel32 loop", 0, 1).is_err(), "target needs @");
        assert!(parse_token("bogus @loop", 0, 1).is_err(), "unknown kind");
        assert!(
            parse_token("a64.imm26 @loop", 0, 1).is_err(),
            "word kinds need a word"
        );
        assert!(
            parse_token("rel32 @loop : 00 00 00 00", 0, 1).is_err(),
            "byte kinds take no word"
        );
        assert!(
            parse_token("abs64 @loop pc=-4", 0, 1).is_err(),
            "absolute kinds take no pc"
        );
        assert!(
            parse_token("a64.imm26 @loop : 00 00 14", 0, 1).is_err(),
            "word must be 4 bytes"
        );
    }
}
