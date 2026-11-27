//! Byte layout: places each slot's authored bytes at its declared file offset.
//!
//! This is the only step that writes a binary, and it performs no encoding: it copies the
//! bytes of each slot file, pads the unused tail of the slot with the declared fill pattern,
//! and fills uncovered gaps with the data fill byte. The result is reproducible with
//! `xxd -r -p build/image.hex`.

use std::fs;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::fixup::{self, Fixup};
use crate::hexfile;
use crate::manifest::{AppDir, SlotKind};

/// A slot after loading its source file.
#[derive(Debug, Clone)]
pub struct LoadedSlot {
    pub index: usize,
    /// Authored bytes (without padding), with every `{...}` field resolved.
    pub bytes: Vec<u8>,
    pub file_offset: u64,
    /// Authoring intents `(slot offset, text)` declared with `#=`.
    pub intents: Vec<(u64, String)>,
    /// Symbolic fields filled from manifest symbols (`{rel32 @f}`, `{a64.imm26 @f : ...}`).
    pub fixups: Vec<Fixup>,
}

/// The laid-out image and per-slot provenance.
#[derive(Debug, Clone)]
pub struct Image {
    pub bytes: Vec<u8>,
    pub slots: Vec<LoadedSlot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SlotLock {
    pub name: String,
    pub vaddr: String,
    pub size: u64,
    pub used: u64,
    pub sha256_used: String,
    pub sha256_padded: String,
    /// Number of symbolic fields resolved by the tool (omitted when zero).
    #[serde(skip_serializing_if = "is_zero")]
    pub fields: usize,
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if signature"
)]
fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Serialize)]
pub struct Lock {
    pub tool: String,
    pub app: String,
    pub version: String,
    pub size: u64,
    pub sha256: String,
    pub slots: Vec<SlotLock>,
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Loads every slot file and lays out the image.
pub fn build(app: &AppDir) -> Result<Image> {
    let m = &app.manifest;
    let code_fill = m.parse_code_fill()?;
    let data_fill = m.parse_data_fill()?;
    let mut slots = Vec::with_capacity(m.slots.len());
    let mut end = 0u64;
    for (index, slot) in m.slots.iter().enumerate() {
        let path = app.slot_path(slot);
        let text = fs::read_to_string(&path)
            .with_context(|| format!("reading slot `{}` ({})", slot.name, path.display()))?;
        let parsed = hexfile::parse_full(&text)
            .with_context(|| format!("parsing slot `{}` ({})", slot.name, path.display()))?;
        let mut bytes = parsed.bytes;
        fixup::apply(&mut bytes, &parsed.fixups, m, slot.vaddr).with_context(|| {
            format!(
                "resolving fields in slot `{}` ({})",
                slot.name,
                path.display()
            )
        })?;
        if bytes.len() as u64 > slot.size {
            bail!(
                "slot `{}` holds {} bytes but is declared {} bytes (overflow by {})",
                slot.name,
                bytes.len(),
                slot.size,
                bytes.len() as u64 - slot.size
            );
        }
        let Some(file_offset) = m.file_offset(slot.vaddr) else {
            bail!(
                "slot `{}` at {:#x} is not file-backed by any segment",
                slot.name,
                slot.vaddr
            );
        };
        if m.file_offset(slot.end() - 1) != Some(file_offset + slot.size - 1) {
            bail!("slot `{}` crosses a segment's file-backed range", slot.name);
        }
        if slot.kind == SlotKind::Code {
            let pad = slot.size - bytes.len() as u64;
            if pad % code_fill.len() as u64 != 0
                || bytes.len() as u64 % m.target.isa.insn_align() != 0
            {
                bail!(
                    "slot `{}`: unused tail ({pad} bytes) is not a whole number of {}-byte NOP fills",
                    slot.name,
                    code_fill.len()
                );
            }
        }
        end = end.max(file_offset + slot.size);
        slots.push(LoadedSlot {
            index,
            bytes,
            file_offset,
            intents: hexfile::parse_intents(&text),
            fixups: parsed.fixups,
        });
    }
    let size = m.target.file_size.unwrap_or(end);
    if size < end {
        bail!("target.file_size {size:#x} is smaller than the last slot end {end:#x}");
    }
    let mut image = vec![data_fill; usize::try_from(size).context("image too large")?];
    for ls in &slots {
        let slot = &m.slots[ls.index];
        let start = ls.file_offset as usize;
        let region = &mut image[start..start + slot.size as usize];
        if slot.kind == SlotKind::Code {
            for (i, b) in region.iter_mut().enumerate() {
                *b = code_fill[i % code_fill.len()];
            }
        }
        region[..ls.bytes.len()].copy_from_slice(&ls.bytes);
    }
    Ok(Image {
        bytes: image,
        slots,
    })
}

impl Image {
    /// Bytes of a slot including its padding.
    #[must_use]
    pub fn padded(&self, app: &AppDir, index: usize) -> &[u8] {
        let ls = &self.slots[index];
        let size = app.manifest.slots[ls.index].size as usize;
        &self.bytes[ls.file_offset as usize..ls.file_offset as usize + size]
    }

    #[must_use]
    pub fn lock(&self, app: &AppDir) -> Lock {
        let m = &app.manifest;
        Lock {
            tool: format!("nopray {}", env!("CARGO_PKG_VERSION")),
            app: m.app.name.clone(),
            version: m.app.version.clone(),
            size: self.bytes.len() as u64,
            sha256: sha256_hex(&self.bytes),
            slots: self
                .slots
                .iter()
                .map(|ls| {
                    let s = &m.slots[ls.index];
                    SlotLock {
                        name: s.name.clone(),
                        vaddr: format!("{:#x}", s.vaddr),
                        size: s.size,
                        used: ls.bytes.len() as u64,
                        sha256_used: sha256_hex(&ls.bytes),
                        sha256_padded: sha256_hex(self.padded(app, ls.index)),
                        fields: ls.fixups.len(),
                    }
                })
                .collect(),
        }
    }
}

/// Plain hex dump compatible with `xxd -r -p`.
#[must_use]
pub fn to_plain_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2 + bytes.len() / 32 + 1);
    for chunk in bytes.chunks(32) {
        s.push_str(&hex::encode(chunk));
        s.push('\n');
    }
    s
}
