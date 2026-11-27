//! Regenerates the comments of slot files from their bytes. Bytes are never changed.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;

use anyhow::{Context, Result, bail};

use crate::audit::resolve_syscall;
use crate::elf;
use crate::fixup::{self, Fixup};
use crate::hexfile::{self, ListingLine};
use crate::isa::{self, Coverage, DecodeOpts, Flow};
use crate::manifest::{AppDir, Format, Os, Slot, SlotKind};
use crate::syscalls;

/// Renders the annotated listing for one slot from its resolved bytes.
/// `intents` are `(slot offset, text)` pairs preserved from the previous listing; `fixups`
/// are symbolic fields re-emitted as their `{...}` token.
#[must_use]
pub fn render_slot(
    app: &AppDir,
    slot: &Slot,
    bytes: &[u8],
    intents: &[(u64, String)],
    fixups: &[Fixup],
) -> String {
    let m = &app.manifest;
    let header = vec![
        format!("nopray slot `{}` · {} · vaddr {:#x} · size {:#x} · used {} bytes", slot.name, format!("{:?}", slot.kind).to_lowercase(), slot.vaddr, slot.size, bytes.len()),
        format!("purpose: {}", slot.purpose),
        "Hex bytes are authoritative; everything after `#` is ignored by `nopray build`.".into(),
        "Comments are regenerated from the bytes by `nopray annotate`: edit bytes, then re-annotate and review.".into(),
    ];
    let mut labels: Vec<(u64, String)> = slot
        .labels
        .iter()
        .map(|l| (slot.vaddr + l.offset, l.name.clone()))
        .collect();
    labels.push((slot.vaddr, slot.name.clone()));
    let elf_header = slot.kind == SlotKind::Header
        && matches!(m.target.format, Format::Elf64 | Format::Elf32)
        && m.file_offset(slot.vaddr) == Some(0);
    let lines = if slot.kind == SlotKind::Code {
        code_lines(app, slot, bytes)
    } else if elf_header {
        header_lines(slot, bytes)
    } else {
        hexfile::data_lines(slot.vaddr, bytes)
    };
    let absolute: Vec<(u64, String)> = intents
        .iter()
        .map(|(o, t)| (slot.vaddr + o, t.clone()))
        .collect();
    let fields: Vec<hexfile::FieldToken<'_>> = fixups
        .iter()
        .map(|f| (slot.vaddr + f.offset, f.kind.size(), f.token.as_str()))
        .collect();
    hexfile::render(&header, &lines, &labels, &absolute, &fields)
}

/// One line per ELF field, so a misplaced field is visible by name; other bytes as data.
fn header_lines(slot: &Slot, bytes: &[u8]) -> Vec<ListingLine> {
    let fields: Vec<elf::Field> = elf::fields(bytes)
        .into_iter()
        .filter(|f| f.offset + f.len <= bytes.len() as u64)
        .collect();
    if fields.is_empty() {
        return hexfile::data_lines(slot.vaddr, bytes);
    }
    let mut lines = Vec::new();
    let mut cursor = 0u64;
    for f in &fields {
        if f.offset < cursor {
            continue;
        }
        if f.offset > cursor {
            lines.extend(hexfile::data_lines(
                slot.vaddr + cursor,
                &bytes[cursor as usize..f.offset as usize],
            ));
        }
        let end = (f.offset + f.len) as usize;
        lines.push(ListingLine {
            addr: slot.vaddr + f.offset,
            bytes: bytes[f.offset as usize..end].to_vec(),
            text: f.display(),
        });
        cursor = f.offset + f.len;
    }
    if (cursor as usize) < bytes.len() {
        lines.extend(hexfile::data_lines(
            slot.vaddr + cursor,
            &bytes[cursor as usize..],
        ));
    }
    lines
}

fn code_lines(app: &AppDir, slot: &Slot, bytes: &[u8]) -> Vec<ListingLine> {
    let m = &app.manifest;
    let opts = DecodeOpts {
        state_register: m.target.state_register.clone(),
        bitness: slot.bits,
    };
    let (mut insns, coverage) = isa::decode(m.target.isa, bytes, slot.vaddr, &opts);
    if coverage == Coverage::None {
        if let Some(ext) = isa::external_listing(m.target.isa, bytes, slot.vaddr) {
            let total: usize = ext.iter().map(|(_, b, _)| b.len()).sum();
            if total == bytes.len() {
                return ext
                    .into_iter()
                    .map(|(addr, b, t)| ListingLine {
                        addr,
                        bytes: b,
                        text: format!("{t}  [objdump, unaudited]"),
                    })
                    .collect();
            }
        }
        return hexfile::data_lines(slot.vaddr, bytes);
    }
    isa::annotate_external(m.target.isa, &mut insns, bytes, slot.vaddr);
    let mut targets: BTreeSet<u64> = insns
        .iter()
        .filter_map(|i| i.flow.direct_target())
        .collect();
    targets.extend(m.entry_points().into_iter().map(|(a, _)| a));
    insns
        .iter()
        .enumerate()
        .map(|(idx, i)| {
            let mut text = i.text.clone();
            if let Some(t) = i.flow.direct_target().or_else(|| {
                i.addr_refs
                    .iter()
                    .copied()
                    .find(|a| m.symbol_at(*a).is_some())
            }) && let Some(name) = m.symbol_at(t)
            {
                let _ = write!(text, "  → {name}");
            }
            if i.flow == Flow::Syscall && m.target.os == Os::Linux {
                let nr = resolve_syscall(&insns, idx, &targets);
                let name = nr.and_then(|n| syscalls::name(m.target.isa, n));
                let _ = write!(
                    text,
                    "  ; {}",
                    name.map_or_else(
                        || nr.map_or("UNRESOLVED".into(), |n| format!("nr {n}")),
                        str::to_owned
                    )
                );
            }
            ListingLine {
                addr: i.addr,
                bytes: i.bytes.clone(),
                text,
            }
        })
        .collect()
}

/// Outcome of annotating one slot file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Annotation {
    Unchanged,
    Updated,
    Stale,
}

/// Rewrites (or with `check_only`, compares) the annotations of every slot.
pub fn run(app: &AppDir, check_only: bool) -> Result<Vec<(String, Annotation)>> {
    let mut out = Vec::new();
    for slot in &app.manifest.slots {
        let path = app.slot_path(slot);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let parsed =
            hexfile::parse_full(&text).with_context(|| format!("parsing {}", path.display()))?;
        let mut bytes = parsed.bytes.clone();
        fixup::apply(&mut bytes, &parsed.fixups, &app.manifest, slot.vaddr)
            .with_context(|| format!("resolving fields in {}", path.display()))?;
        let intents = hexfile::parse_intents(&text);
        let rendered = render_slot(app, slot, &bytes, &intents, &parsed.fixups);
        let again = hexfile::parse_full(&rendered)?;
        let same_fields = again.fixups.len() == parsed.fixups.len()
            && again
                .fixups
                .iter()
                .zip(&parsed.fixups)
                .all(|(a, b)| a.offset == b.offset && a.token == b.token);
        if again.bytes != parsed.bytes || !same_fields {
            bail!(
                "internal error: annotation of `{}` would change its bytes or fields",
                slot.name
            );
        }
        if hexfile::parse_intents(&rendered) != intents {
            bail!(
                "internal error: annotation of `{}` would move or drop an intent",
                slot.name
            );
        }
        let state = if rendered == text {
            Annotation::Unchanged
        } else if check_only {
            Annotation::Stale
        } else {
            fs::write(&path, &rendered).with_context(|| format!("writing {}", path.display()))?;
            Annotation::Updated
        };
        out.push((slot.name.clone(), state));
    }
    Ok(out)
}
