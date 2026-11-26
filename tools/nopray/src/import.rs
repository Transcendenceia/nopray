//! Porting existing binaries into the mapped format.
//!
//! `propose` drafts a manifest by reading ELF headers and splitting the code range at NOP
//! padding and call targets. `import` extracts each declared slot's bytes from a binary into
//! annotated slot files and proves the rebuilt image is byte-identical.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::annotate;
use crate::elf;
use crate::image;
use crate::isa::{self, DecodeOpts, Flow};
use crate::manifest::{AppDir, Isa, MANIFEST_FILE, Manifest, SlotKind};

/// Options for drafting a manifest from an existing ELF.
#[derive(Debug, Clone)]
pub struct ProposeOpts {
    pub name: String,
    pub isa: Isa,
    /// First address after the code (start of read-only data). Defaults to the end of the
    /// first executable segment's file bytes.
    pub code_end: Option<u64>,
}

fn perms_of(flags: u32) -> String {
    elf::perms(flags)
}

/// Drafts `nopray.toml` text for `binary`.
pub fn propose(binary: &[u8], opts: &ProposeOpts) -> Result<String> {
    let h = elf::parse(binary)?;
    let loads: Vec<&elf::Phdr> = h
        .phdrs
        .iter()
        .filter(|p| p.p_type == elf::PT_LOAD)
        .collect();
    let Some(text) = loads.iter().find(|p| p.flags & 1 != 0) else {
        bail!("no executable PT_LOAD")
    };
    let base = text.vaddr - text.offset;
    let ph_end = h.phoff + (h.phdrs.len() as u64) * if h.class64 { 56 } else { 32 };
    let text_end = text.vaddr + text.filesz;
    let code_end = opts.code_end.unwrap_or(text_end);
    let fill = opts.isa.default_code_fill();
    let align = opts.isa.insn_align();

    // First code byte after the headers: skip header padding (zeros).
    let mut code_start = base + ph_end;
    while code_start < code_end && binary[(code_start - base) as usize] == 0 {
        code_start += 1;
    }
    code_start = code_start.div_ceil(16) * 16;
    if h.entry < code_start {
        code_start = h.entry - (h.entry % 16);
    }
    let code = &binary[(code_start - base) as usize..(code_end - base) as usize];
    let (insns, _) = isa::decode(opts.isa, code, code_start, &DecodeOpts::default());
    // Hard starts: image code start, entry point and call targets. Soft starts: the first real
    // instruction after NOP padding that follows a block terminator.
    let mut hard: BTreeSet<u64> = BTreeSet::from([code_start, h.entry]);
    for i in &insns {
        if let Flow::Call(t) = i.flow
            && (code_start..code_end).contains(&t)
        {
            hard.insert(t);
        }
    }
    let is_fill = |i: &isa::Insn| i.bytes.chunks(fill.len()).all(|c| c == fill);
    let mut soft: BTreeSet<u64> = BTreeSet::new();
    for w in insns.windows(2) {
        if w[0].flow.ends_block()
            && is_fill(&w[1])
            && let Some(next) = insns.iter().find(|i| i.addr > w[1].addr && !is_fill(i))
        {
            soft.insert(next.addr);
        }
    }
    soft.retain(|a| !hard.contains(a));
    let branches: Vec<(u64, u64)> = insns
        .iter()
        .filter_map(|i| i.flow.direct_target().map(|t| (i.addr, t)))
        .collect();
    let in_code = |a: &u64| *a >= code_start && *a < code_end && a % align == 0;
    // A soft start reached by a branch from the preceding slot is a local label: merge it back.
    loop {
        let all: Vec<u64> = hard.union(&soft).copied().filter(in_code).collect();
        let slot_of = |a: u64| all.iter().rposition(|s| *s <= a);
        let merge = all.iter().enumerate().skip(1).find(|(k, s)| {
            let end = all.get(k + 1).copied().unwrap_or(code_end);
            soft.contains(*s)
                && branches
                    .iter()
                    .any(|(src, t)| slot_of(*src) == Some(k - 1) && (**s..end).contains(t))
        });
        match merge {
            Some((_, s)) => {
                let s = *s;
                soft.remove(&s);
            }
            None => break,
        }
    }
    let starts: Vec<u64> = hard.union(&soft).copied().filter(in_code).collect();
    let slot_index = |a: u64| starts.iter().rposition(|s| *s <= a);
    // Remaining cross-slot targets that are not slot starts become explicit entry labels.
    let labels: BTreeSet<u64> = branches
        .iter()
        .filter(|(src, t)| {
            (code_start..code_end).contains(t)
                && slot_index(*src) != slot_index(*t)
                && !starts.contains(t)
        })
        .map(|(_, t)| *t)
        .collect();

    let mut s = String::new();
    let _ = writeln!(
        s,
        "# Draft map proposed by `nopray propose`: rename slots, write purposes and contracts, then `nopray import`."
    );
    let _ = writeln!(
        s,
        "[app]\nname = \"{}\"\nversion = \"0.0.0-imported\"\nstandard = \"DMCS-1.1\"\ntier = \"L1\"\nexposure = \"local\"\n",
        opts.name
    );
    let entry_name = "_start";
    let _ = writeln!(
        s,
        "[target]\nisa = \"{}\"\nos = \"linux\"\nformat = \"{}\"\nbase = {:#x}\nentry = \"{entry_name}\"\noutput = \"{}\"\nfile_size = {:#x}\n",
        serde_plain_isa(opts.isa),
        if h.class64 { "elf64" } else { "elf32" },
        base,
        opts.name,
        binary.len()
    );
    for (i, p) in loads.iter().enumerate() {
        let _ = writeln!(
            s,
            "[[segment]]\nname = \"seg{i}\"\nvaddr = {:#x}\noffset = {:#x}\nfile_size = {:#x}\nmem_size = {:#x}\nperms = \"{}\"\nalign = {:#x}\n",
            p.vaddr,
            p.offset,
            p.filesz,
            p.memsz,
            perms_of(p.flags),
            p.align
        );
    }
    let slot = |s: &mut String, name: &str, kind: &str, vaddr: u64, size: u64, purpose: &str| {
        let _ = writeln!(
            s,
            "[[slot]]\nname = \"{name}\"\nkind = \"{kind}\"\nvaddr = {vaddr:#x}\nsize = {size:#x}\nfile = \"slots/{:04x}_{name}.hex\"\npurpose = \"{purpose}\"\n",
            vaddr - base
        );
    };
    slot(
        &mut s,
        "elf_header",
        "header",
        base,
        code_start - base,
        "ELF header and program headers",
    );
    for (i, a) in starts.iter().enumerate() {
        let end = starts.get(i + 1).copied().unwrap_or(code_end);
        let name = if *a == h.entry {
            entry_name.to_owned()
        } else {
            format!("fn_{a:x}")
        };
        slot(&mut s, &name, "code", *a, end - a, "TODO: describe");
        for l in labels.iter().filter(|l| (*a..end).contains(*l)) {
            let _ = writeln!(
                s,
                "[[slot.label]]\nname = \"L_{l:x}\"\noffset = {:#x}\nentry = true\nnote = \"cross-slot branch target found by propose: an explicit coupling\"\n",
                l - a
            );
        }
    }
    if code_end < text_end {
        slot(
            &mut s,
            "rodata",
            "rodata",
            code_end,
            text_end - code_end,
            "TODO: describe read-only data",
        );
    }
    for (i, p) in loads
        .iter()
        .enumerate()
        .filter(|(_, p)| p.filesz > 0 && p.vaddr != text.vaddr)
    {
        slot(
            &mut s,
            &format!("data{i}"),
            if p.flags & 2 != 0 { "data" } else { "rodata" },
            p.vaddr,
            p.filesz,
            "TODO: describe",
        );
    }
    Ok(s)
}

fn serde_plain_isa(isa: Isa) -> &'static str {
    match isa {
        Isa::X86_64 => "x86-64",
        Isa::Aarch64 => "aarch64",
        Isa::Riscv64 => "riscv64",
        Isa::Riscv32 => "riscv32",
        Isa::Thumb => "thumb",
        Isa::Xtensa => "xtensa",
        Isa::Avr => "avr",
    }
}

/// Extracts slot bytes from `binary` into `out` using the manifest at `manifest_path`.
pub fn import(binary: &[u8], manifest_path: &Path, out: &Path) -> Result<AppDir> {
    let text = fs::read_to_string(manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest: Manifest = toml::from_str(&text).context("parsing manifest")?;
    manifest.validate()?;
    fs::create_dir_all(out.join("slots")).with_context(|| format!("creating {}", out.display()))?;
    fs::write(out.join(MANIFEST_FILE), &text)?;
    let app = AppDir {
        root: out.to_path_buf(),
        manifest,
    };
    let m = &app.manifest;
    let code_fill = m.parse_code_fill()?;
    let data_fill = m.parse_data_fill()?;
    for slot in &m.slots {
        let Some(off) = m.file_offset(slot.vaddr) else {
            bail!("slot `{}` is not file-backed", slot.name)
        };
        let range = off as usize..(off + slot.size) as usize;
        let Some(raw) = binary.get(range.clone()) else {
            bail!("slot `{}` ({range:?}) is outside the binary", slot.name)
        };
        let mut used = raw.len();
        if slot.kind == SlotKind::Code {
            let n = code_fill.len();
            while used >= n && raw[used - n..used] == code_fill[..] {
                used -= n;
            }
        } else {
            while used > 0 && raw[used - 1] == data_fill {
                used -= 1;
            }
        }
        let path = app.slot_path(slot);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, annotate::render_slot(&app, slot, &raw[..used], &[]))?;
    }
    let built = image::build(&app)?;
    if built.bytes != binary {
        let first = built.bytes.iter().zip(binary).position(|(a, b)| a != b);
        bail!(
            "imported map does not rebuild the binary (sizes {} vs {}, first difference {first:?}); fix slot bounds or file_size",
            built.bytes.len(),
            binary.len()
        );
    }
    Ok(app)
}
