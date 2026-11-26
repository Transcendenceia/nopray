//! Minimal, bounds-checked ELF32/ELF64 header and program-header reader (little-endian).

use anyhow::{Result, bail};

pub const PT_LOAD: u32 = 1;
pub const PT_DYNAMIC: u32 = 2;
pub const PT_INTERP: u32 = 3;
pub const PT_GNU_STACK: u32 = 0x6474_e551;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phdr {
    pub p_type: u32,
    pub flags: u32,
    pub offset: u64,
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}

#[derive(Debug, Clone)]
pub struct Elf {
    pub class64: bool,
    pub e_type: u16,
    pub machine: u16,
    pub entry: u64,
    pub phoff: u64,
    pub shoff: u64,
    pub shnum: u16,
    pub phdrs: Vec<Phdr>,
}

fn rd(b: &[u8], off: usize, n: usize) -> Result<u64> {
    let Some(s) = b.get(off..off + n) else {
        bail!(
            "ELF field at {off:#x}+{n} is outside the {}-byte file",
            b.len()
        );
    };
    let mut v = 0u64;
    for (i, byte) in s.iter().enumerate() {
        v |= u64::from(*byte) << (8 * i);
    }
    Ok(v)
}

/// Parses the ELF header and all program headers.
pub fn parse(b: &[u8]) -> Result<Elf> {
    if b.get(..4) != Some(b"\x7fELF".as_slice()) {
        bail!("missing ELF magic");
    }
    let class64 = match b.get(4) {
        Some(2) => true,
        Some(1) => false,
        other => bail!("unknown ELF class {other:?}"),
    };
    if b.get(5) != Some(&1) {
        bail!("only little-endian ELF is supported");
    }
    let (w, phoff_at, shoff_at, flags_at) = if class64 {
        (8, 0x20, 0x28, 0x30)
    } else {
        (4, 0x1c, 0x20, 0x24)
    };
    let e_type = rd(b, 0x10, 2)? as u16;
    let machine = rd(b, 0x12, 2)? as u16;
    let entry = rd(b, 0x18, w)?;
    let phoff = rd(b, phoff_at, w)?;
    let shoff = rd(b, shoff_at, w)?;
    let phentsize = rd(b, flags_at + 6, 2)? as usize;
    let phnum = rd(b, flags_at + 8, 2)? as usize;
    let shnum = rd(b, flags_at + 12, 2)? as u16;
    let expected = if class64 { 56 } else { 32 };
    if phnum > 0 && phentsize != expected {
        bail!("e_phentsize is {phentsize}, expected {expected}");
    }
    let mut phdrs = Vec::with_capacity(phnum);
    for i in 0..phnum {
        let o = usize::try_from(phoff)? + i * expected;
        let ph = if class64 {
            Phdr {
                p_type: rd(b, o, 4)? as u32,
                flags: rd(b, o + 4, 4)? as u32,
                offset: rd(b, o + 8, 8)?,
                vaddr: rd(b, o + 16, 8)?,
                filesz: rd(b, o + 32, 8)?,
                memsz: rd(b, o + 40, 8)?,
                align: rd(b, o + 48, 8)?,
            }
        } else {
            Phdr {
                p_type: rd(b, o, 4)? as u32,
                offset: rd(b, o + 4, 4)?,
                vaddr: rd(b, o + 8, 4)?,
                filesz: rd(b, o + 16, 4)?,
                memsz: rd(b, o + 20, 4)?,
                flags: rd(b, o + 24, 4)? as u32,
                align: rd(b, o + 28, 4)?,
            }
        };
        phdrs.push(ph);
    }
    Ok(Elf {
        class64,
        e_type,
        machine,
        entry,
        phoff,
        shoff,
        shnum,
        phdrs,
    })
}

/// `rwx`-style string for ELF `p_flags`.
#[must_use]
pub fn perms(flags: u32) -> String {
    format!(
        "{}{}{}",
        if flags & 4 != 0 { 'r' } else { '-' },
        if flags & 2 != 0 { 'w' } else { '-' },
        if flags & 1 != 0 { 'x' } else { '-' }
    )
}

/// One named field of the ELF header or of a program header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// File offset of the first byte.
    pub offset: u64,
    pub len: u64,
    /// Field name, e.g. `e_entry` or `p_align`.
    pub name: &'static str,
    /// Program-header index for `p_*` fields.
    pub phdr: Option<usize>,
    pub value: u64,
}

impl Field {
    /// Human rendering of the value (symbolic where the ELF spec defines names).
    #[must_use]
    pub fn display(&self) -> String {
        let v = self.value;
        let sym = match self.name {
            "e_type" => match v {
                1 => Some("ET_REL"),
                2 => Some("ET_EXEC"),
                3 => Some("ET_DYN"),
                _ => None,
            },
            "p_type" => match v as u32 {
                PT_LOAD => Some("PT_LOAD"),
                PT_DYNAMIC => Some("PT_DYNAMIC"),
                PT_INTERP => Some("PT_INTERP"),
                PT_GNU_STACK => Some("PT_GNU_STACK"),
                _ => None,
            },
            _ => None,
        };
        let prefix = self.phdr.map(|i| format!("phdr[{i}].")).unwrap_or_default();
        match (self.name, sym) {
            ("e_ident", _) => format!("{prefix}e_ident"),
            ("p_flags", _) => format!("{prefix}p_flags = {} ({v:#x})", perms(v as u32)),
            (_, Some(s)) => format!("{prefix}{} = {s} ({v:#x})", self.name),
            _ => format!("{prefix}{} = {v:#x}", self.name),
        }
    }
}

/// Field layout of the ELF header and program headers found in `b` (best effort, bounds-checked).
#[must_use]
pub fn fields(b: &[u8]) -> Vec<Field> {
    let Ok(h) = parse(b) else { return Vec::new() };
    let header: &[(u64, u64, &str)] = if h.class64 {
        &[
            (0, 16, "e_ident"),
            (16, 2, "e_type"),
            (18, 2, "e_machine"),
            (20, 4, "e_version"),
            (24, 8, "e_entry"),
            (32, 8, "e_phoff"),
            (40, 8, "e_shoff"),
            (48, 4, "e_flags"),
            (52, 2, "e_ehsize"),
            (54, 2, "e_phentsize"),
            (56, 2, "e_phnum"),
            (58, 2, "e_shentsize"),
            (60, 2, "e_shnum"),
            (62, 2, "e_shstrndx"),
        ]
    } else {
        &[
            (0, 16, "e_ident"),
            (16, 2, "e_type"),
            (18, 2, "e_machine"),
            (20, 4, "e_version"),
            (24, 4, "e_entry"),
            (28, 4, "e_phoff"),
            (32, 4, "e_shoff"),
            (36, 4, "e_flags"),
            (40, 2, "e_ehsize"),
            (42, 2, "e_phentsize"),
            (44, 2, "e_phnum"),
            (46, 2, "e_shentsize"),
            (48, 2, "e_shnum"),
            (50, 2, "e_shstrndx"),
        ]
    };
    let phdr: &[(u64, u64, &str)] = if h.class64 {
        &[
            (0, 4, "p_type"),
            (4, 4, "p_flags"),
            (8, 8, "p_offset"),
            (16, 8, "p_vaddr"),
            (24, 8, "p_paddr"),
            (32, 8, "p_filesz"),
            (40, 8, "p_memsz"),
            (48, 8, "p_align"),
        ]
    } else {
        &[
            (0, 4, "p_type"),
            (4, 4, "p_offset"),
            (8, 4, "p_vaddr"),
            (12, 4, "p_paddr"),
            (16, 4, "p_filesz"),
            (20, 4, "p_memsz"),
            (24, 4, "p_flags"),
            (28, 4, "p_align"),
        ]
    };
    let entsize = if h.class64 { 56 } else { 32 };
    let mut out = Vec::new();
    let mut add = |offset: u64, len: u64, name: &'static str, phdr: Option<usize>| {
        let value = if len > 8 {
            0
        } else {
            rd(b, offset as usize, len as usize).unwrap_or(0)
        };
        out.push(Field {
            offset,
            len,
            name,
            phdr,
            value,
        });
    };
    for (o, l, n) in header {
        add(*o, *l, n, None);
    }
    for i in 0..h.phdrs.len() {
        let base = h.phoff + (i as u64) * entsize;
        for (o, l, n) in phdr {
            add(base + o, *l, n, Some(i));
        }
    }
    out.sort_by_key(|f| f.offset);
    out
}
