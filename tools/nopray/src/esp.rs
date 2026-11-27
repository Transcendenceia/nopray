//! ESP32-S3 loader-image container (`target.container = "esp-image"`).
//!
//! Reads the segment table and the two integrity fields the ROM and second-stage loader
//! check: the segment-data XOR byte and the optional appended SHA-256. This is arithmetic
//! over bytes that already exist; it never creates or encodes an instruction.

use std::fmt::Write as _;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

const MAGIC: u8 = 0xE9;
const HEADER_LEN: usize = 24;
const XOR_SEED: u8 = 0xEF;
/// `esp_chip_id_t` for ESP32-S3; the only memory map pinned from primary sources here.
const CHIP_ID_ESP32S3: u16 = 0x0009;

/// One loader segment: an 8-byte descriptor followed by its data.
#[derive(Debug, Clone)]
pub struct Segment {
    pub load_addr: u32,
    /// File offset of the 8-byte descriptor.
    pub descriptor: usize,
    /// File offset of the segment data.
    pub data: usize,
    pub len: usize,
}

/// Appended SHA-256 over the carrier, as stored and as recomputed from the stored bytes.
#[derive(Debug, Clone)]
pub struct AppendedDigest {
    pub offset: usize,
    pub stored: [u8; 32],
    pub computed: [u8; 32],
}

/// Parsed container layout with stored and recomputed integrity fields.
#[derive(Debug, Clone)]
pub struct Image {
    pub chip_id: u16,
    pub entry: u32,
    pub segments: Vec<Segment>,
    /// File offset of the XOR byte (last byte of the 16-byte aligned carrier).
    pub xor_offset: usize,
    pub stored_xor: u8,
    pub computed_xor: u8,
    pub digest: Option<AppendedDigest>,
    /// Bytes the container accounts for; must equal the file size.
    pub end: usize,
}

/// Parses an ESP loader image. Fails on a bad magic or truncation.
pub fn parse(b: &[u8]) -> Result<Image> {
    if b.len() < HEADER_LEN || b[0] != MAGIC {
        bail!(
            "not an ESP loader image (magic {:#04x})",
            b.first().copied().unwrap_or(0)
        );
    }
    let count = usize::from(b[1]);
    let entry = u32::from_le_bytes(b[4..8].try_into()?);
    let chip_id = u16::from_le_bytes(b[12..14].try_into()?);
    let hash_appended = b[23] == 1;
    let mut pos = HEADER_LEN;
    let mut xor = XOR_SEED;
    let mut segments = Vec::with_capacity(count);
    for i in 0..count {
        let d = b
            .get(pos..pos + 8)
            .with_context(|| format!("segment {i} descriptor runs past the end of the file"))?;
        let load_addr = u32::from_le_bytes(d[0..4].try_into()?);
        let len = u32::from_le_bytes(d[4..8].try_into()?) as usize;
        let data = pos + 8;
        let body = b
            .get(data..data + len)
            .with_context(|| format!("segment {i} data runs past the end of the file"))?;
        xor = body.iter().fold(xor, |acc, v| acc ^ v);
        segments.push(Segment {
            load_addr,
            descriptor: pos,
            data,
            len,
        });
        pos = data + len;
    }
    // Padding then the XOR byte, so the carrier length is a multiple of 16.
    let carrier_end = (pos + 16) & !15;
    let xor_offset = carrier_end - 1;
    let stored_xor = *b
        .get(xor_offset)
        .context("image ends before its XOR byte")?;
    let mut end = carrier_end;
    let digest = if hash_appended {
        let stored: [u8; 32] = b
            .get(carrier_end..carrier_end + 32)
            .context("image ends before its appended SHA-256")?
            .try_into()?;
        let computed: [u8; 32] = Sha256::digest(&b[..carrier_end]).into();
        end += 32;
        Some(AppendedDigest {
            offset: carrier_end,
            stored,
            computed,
        })
    } else {
        None
    };
    Ok(Image {
        chip_id,
        entry,
        segments,
        xor_offset,
        stored_xor,
        computed_xor: xor,
        digest,
        end,
    })
}

/// Slot kind, segment permissions and a short region name for an ESP32-S3 load address
/// (ESP32-S3 Technical Reference Manual, system and memory address map).
fn region(addr: u32) -> Option<(&'static str, &'static str, &'static str)> {
    match addr {
        0x4037_0000..0x403E_0000 => Some(("code", "rwx", "iram")),
        0x4200_0000..0x4400_0000 => Some(("code", "r-x", "irom")),
        0x3C00_0000..0x3E00_0000 => Some(("rodata", "r--", "drom")),
        0x3FC8_8000..0x3FD0_0000 => Some(("data", "rw-", "dram")),
        0x5000_0000..0x5000_2000 => Some(("data", "rw-", "rtc_slow")),
        0x600F_E000..0x6010_0000 => Some(("data", "rw-", "rtc_fast")),
        _ => None,
    }
}

/// Options for [`propose`].
#[derive(Debug, Clone)]
pub struct ProposeOpts {
    pub name: String,
    pub version: String,
    pub output: String,
    pub authored_by: String,
    pub date: String,
}

/// Drafts a container-granularity manifest: header, descriptors, segment bodies, XOR slot and
/// digest. Function boundaries are not inferred.
pub fn propose(b: &[u8], o: &ProposeOpts) -> Result<String> {
    let img = parse(b)?;
    if img.chip_id != CHIP_ID_ESP32S3 {
        bail!(
            "chip id {:#06x} is not ESP32-S3; only the S3 memory map is pinned, map this image by hand",
            img.chip_id
        );
    }
    if img.end != b.len() {
        bail!(
            "container accounts for {} bytes but the file has {}",
            img.end,
            b.len()
        );
    }
    let sha = hex::encode(Sha256::digest(b));
    // (name, vaddr, file offset, size, kind, perms, purpose)
    let mut items: Vec<(String, u64, usize, usize, &str, &str, &str)> = vec![(
        "esp_header".into(),
        0,
        0,
        HEADER_LEN,
        "header",
        "r--",
        "ESP image header and extended header. File coordinates only.",
    )];
    let mut entry_label = None;
    for (i, s) in img.segments.iter().enumerate() {
        items.push((
            format!("descriptor_{i}"),
            s.descriptor as u64,
            s.descriptor,
            8,
            "header",
            "r--",
            "ESP segment descriptor. File coordinates only.",
        ));
        if s.len == 0 {
            continue;
        }
        let (kind, perms, rname) = region(s.load_addr).with_context(|| {
            format!(
                "segment {i} load address {:#x} is outside the pinned ESP32-S3 map",
                s.load_addr
            )
        })?;
        let name = format!("seg{i}_{rname}");
        let lo = u64::from(s.load_addr);
        if (lo..lo + s.len as u64).contains(&u64::from(img.entry)) {
            entry_label = Some((name.clone(), u64::from(img.entry) - lo));
        }
        items.push((
            name,
            lo,
            s.data,
            s.len,
            kind,
            perms,
            "Loader segment; function boundaries not recovered.",
        ));
    }
    let pad_start = img.segments.last().map_or(HEADER_LEN, |s| s.data + s.len);
    items.push((
        "alignment_checksum".into(),
        pad_start as u64,
        pad_start,
        img.xor_offset + 1 - pad_start,
        "rodata",
        "r--",
        "Padding and ESP segment-data XOR byte. File coordinates only.",
    ));
    if let Some(d) = &img.digest {
        items.push((
            "appended_digest".into(),
            d.offset as u64,
            d.offset,
            32,
            "rodata",
            "r--",
            "SHA-256 of the carrier through the XOR byte. File coordinates only.",
        ));
    }

    let mut t = String::new();
    let entry = entry_label.as_ref().map_or("esp_header", |_| "image_entry");
    writeln!(
        t,
        "# Container-granularity import: loader segments only, not hand-authored code.\n\
         [app]\nname = \"{}\"\nversion = \"{}\"\nstandard = \"DMCS-1.1\"\ntier = \"L1\"\nexposure = \"local\"\n\
         description = \"Byte-exact loader-segment import of {}; sha256 {sha}.\"\n\n\
         [target]\nisa = \"xtensa\"\nos = \"bare-metal\"\nformat = \"raw\"\ncontainer = \"esp-image\"\nbase = 0\n\
         entry = \"{entry}\"\noutput = \"{}\"\nfile_size = {:#x}\n\n\
         [provenance]\nauthored_by = \"{}\"\n\
         method = \"Imported pre-existing compilation; only loader-segment bounds are mapped. SHA-256 {sha}.\"\n\
         date = \"{}\"",
        o.name,
        o.version,
        o.output,
        o.output,
        b.len(),
        o.authored_by,
        o.date
    )?;
    for (name, vaddr, off, size, _, perms, _) in &items {
        writeln!(
            t,
            "\n[[segment]]\nname = \"{name}\"\nvaddr = {vaddr:#x}\noffset = {off:#x}\nfile_size = {size:#x}\nmem_size = {size:#x}\nperms = \"{perms}\"\nalign = 1"
        )?;
    }
    for (name, vaddr, off, size, kind, _, purpose) in &items {
        writeln!(
            t,
            "\n[[slot]]\nname = \"{name}\"\nkind = \"{kind}\"\nvaddr = {vaddr:#x}\nsize = {size:#x}\nfile = \"slots/{off:06x}_{name}.hex\"\npurpose = \"{purpose}\""
        )?;
        if let Some((ename, eoff)) = &entry_label {
            if ename == name {
                writeln!(
                    t,
                    "[[slot.label]]\nname = \"image_entry\"\noffset = {eoff:#x}\nentry = true"
                )?;
            }
        }
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One 4-byte IRAM segment, entry at its start, SHA-256 appended.
    fn sample() -> Vec<u8> {
        let mut b = vec![0u8; HEADER_LEN];
        b[0] = MAGIC;
        b[1] = 1;
        b[4..8].copy_from_slice(&0x403c_8700u32.to_le_bytes());
        b[12..14].copy_from_slice(&CHIP_ID_ESP32S3.to_le_bytes());
        b[23] = 1;
        b.extend_from_slice(&0x403c_8700u32.to_le_bytes());
        b.extend_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        while b.len() % 16 != 15 {
            b.push(0);
        }
        b.push(XOR_SEED ^ 0x11 ^ 0x22 ^ 0x33 ^ 0x44);
        let d: [u8; 32] = Sha256::digest(&b).into();
        b.extend_from_slice(&d);
        b
    }

    #[test]
    fn integrity_fields_round_trip_and_detect_edits() {
        let mut b = sample();
        let img = parse(&b).unwrap();
        assert_eq!(img.end, b.len());
        assert_eq!(img.stored_xor, img.computed_xor);
        let d = img.digest.unwrap();
        assert_eq!(d.stored, d.computed);
        b[32] ^= 0x01; // edit one data byte without refreshing the integrity fields
        let img = parse(&b).unwrap();
        assert_ne!(img.stored_xor, img.computed_xor);
        let d = img.digest.unwrap();
        assert_ne!(d.stored, d.computed);
    }

    #[test]
    fn propose_maps_every_byte_and_labels_the_entry() {
        let b = sample();
        let opts = ProposeOpts {
            name: "t".into(),
            version: "1".into(),
            output: "t.bin".into(),
            authored_by: "test".into(),
            date: "2026-11-27".into(),
        };
        let t = propose(&b, &opts).unwrap();
        assert!(t.contains("container = \"esp-image\""));
        assert!(t.contains("name = \"seg0_iram\""));
        assert!(t.contains("name = \"image_entry\"\noffset = 0x0\nentry = true"));
        assert!(t.contains("name = \"appended_digest\""));
        let mut other = b.clone();
        other[12] = 0x0d; // ESP32-C6
        assert!(propose(&other, &opts).is_err());
    }
}
