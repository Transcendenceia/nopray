//! Static audit: proves the bytes agree with the map and with the security policy.
//!
//! What the audit establishes is limited to the artifact: layout, headers, decodability,
//! control flow between declared entry points, syscall surface and memory permissions. It
//! cannot prove how the bytes were produced; see `references/SECURITY.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Serialize;

use crate::elf;
use crate::image::{Image, sha256_hex};
use crate::intent;
use crate::isa::{self, Coverage, DecodeOpts, Flow, Insn, SyscallReg};
use crate::manifest::{AppDir, Exposure, Format, Isa, Os, Slot, SlotKind, Tier};
use crate::syscalls;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Severity::Error => "ERROR",
            Severity::Warning => "WARN ",
            Severity::Info => "info ",
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "ser_opt_hex"
    )]
    pub addr: Option<u64>,
    pub message: String,
}

#[expect(clippy::ref_option, reason = "serde serialize_with passes &Option<T>")]
fn ser_opt_hex<S: serde::Serializer>(v: &Option<u64>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(a) => s.serialize_str(&format!("{a:#x}")),
        None => s.serialize_none(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SyscallSite {
    pub slot: String,
    pub addr: String,
    pub number: Option<u64>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SlotReport {
    pub name: String,
    pub kind: String,
    pub vaddr: String,
    pub size: u64,
    pub used: u64,
    pub sha256: String,
    pub purpose: String,
    pub coverage: Option<Coverage>,
    pub instructions: usize,
    pub calls: BTreeSet<String>,
    pub called_by: BTreeSet<String>,
    pub jumps_to: BTreeSet<String>,
    pub data_refs: BTreeSet<String>,
    pub referenced_by: BTreeSet<String>,
    pub syscalls: BTreeSet<String>,
    pub writes: BTreeSet<String>,
    pub state_reads: BTreeSet<String>,
    pub state_writes: BTreeSet<String>,
    pub indirect_branches: usize,
    pub intents: usize,
    pub intent_mismatches: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub app: String,
    pub version: String,
    pub isa: String,
    pub tier: Tier,
    pub exposure: Exposure,
    pub size: u64,
    pub sha256: String,
    pub reference: Option<String>,
    pub findings: Vec<Finding>,
    pub slots: Vec<SlotReport>,
    pub syscall_sites: Vec<SyscallSite>,
    pub syscall_set: BTreeSet<String>,
}

impl Report {
    #[must_use]
    pub fn count(&self, sev: Severity) -> usize {
        self.findings.iter().filter(|f| f.severity == sev).count()
    }
}

struct Ctx<'a> {
    app: &'a AppDir,
    findings: Vec<Finding>,
}

impl Ctx<'_> {
    fn push(
        &mut self,
        severity: Severity,
        code: &'static str,
        slot: Option<&str>,
        addr: Option<u64>,
        message: impl Into<String>,
    ) {
        self.findings.push(Finding {
            severity,
            code,
            slot: slot.map(str::to_owned),
            addr,
            message: message.into(),
        });
    }

    fn exposed(&self) -> bool {
        self.app.manifest.app.exposure != Exposure::Local
    }
}

/// Decoded code of one slot.
struct Decoded {
    index: usize,
    insns: Vec<Insn>,
    coverage: Coverage,
}

/// Runs every check. `reference` is an existing binary that must equal the built image.
pub fn run(app: &AppDir, image: &Image, reference: Option<(&str, &[u8])>) -> Report {
    let m = &app.manifest;
    let mut cx = Ctx {
        app,
        findings: Vec::new(),
    };

    if let Some((label, bytes)) = reference {
        compare_reference(&mut cx, image, label, bytes);
    }
    check_format(&mut cx, &image.bytes);
    check_header_intents(&mut cx, image);
    check_policy(&mut cx);
    check_gaps(&mut cx, image);

    let mut decoded = Vec::new();
    for ls in &image.slots {
        let slot = &m.slots[ls.index];
        if slot.kind != SlotKind::Code {
            continue;
        }
        let opts = DecodeOpts {
            state_register: m.target.state_register.clone(),
            bitness: slot.bits,
        };
        let (mut insns, coverage) = isa::decode(m.target.isa, &ls.bytes, slot.vaddr, &opts);
        isa::annotate_external(m.target.isa, &mut insns, &ls.bytes, slot.vaddr);
        decoded.push(Decoded {
            index: ls.index,
            insns,
            coverage,
        });
    }

    // Every direct branch target in the program, to bound backward syscall resolution.
    let mut targets: BTreeSet<u64> = decoded
        .iter()
        .flat_map(|d| d.insns.iter().filter_map(|i| i.flow.direct_target()))
        .collect();
    targets.extend(m.entry_points().into_iter().map(|(a, _)| a));

    let mut slot_reports: Vec<SlotReport> = m
        .slots
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let ls = image
                .slots
                .iter()
                .find(|l| l.index == i)
                .expect("every slot is loaded");
            SlotReport {
                name: s.name.clone(),
                kind: format!("{:?}", s.kind).to_lowercase(),
                vaddr: format!("{:#x}", s.vaddr),
                size: s.size,
                used: ls.bytes.len() as u64,
                sha256: sha256_hex(image.padded(app, i)),
                purpose: s.purpose.clone(),
                ..SlotReport::default()
            }
        })
        .collect();

    let mut sites = Vec::new();
    for d in &decoded {
        let slot = &m.slots[d.index];
        let used = image
            .slots
            .iter()
            .find(|l| l.index == d.index)
            .map_or(0, |l| l.bytes.len() as u64);
        slot_reports[d.index].coverage = Some(d.coverage);
        slot_reports[d.index].instructions = d.insns.len();
        check_code_slot(
            &mut cx,
            slot,
            used,
            d,
            &image
                .slots
                .iter()
                .find(|l| l.index == d.index)
                .expect("slot loaded")
                .intents,
            &targets,
            &mut slot_reports,
            &mut sites,
        );
    }
    for (i, slot) in m.slots.iter().enumerate() {
        if slot.kind == SlotKind::Code && !decoded.iter().any(|d| d.index == i) {
            slot_reports[i].coverage = Some(Coverage::None);
        }
    }
    // Back-references.
    let snapshot: Vec<(String, BTreeSet<String>, BTreeSet<String>)> = slot_reports
        .iter()
        .map(|r| (r.name.clone(), r.calls.clone(), r.data_refs.clone()))
        .collect();
    for (caller, calls, refs) in snapshot {
        for c in calls {
            if let Some(r) = slot_reports
                .iter_mut()
                .find(|r| owner_of(m, &c).is_some_and(|o| o == r.name))
            {
                r.called_by.insert(caller.clone());
            }
        }
        for d in refs {
            if let Some(r) = slot_reports.iter_mut().find(|r| r.name == d) {
                r.referenced_by.insert(caller.clone());
            }
        }
    }
    check_contract_calls(&mut cx, &slot_reports);

    let entry_owner = owner_of(m, &m.target.entry);
    for r in &slot_reports {
        if r.kind == "code"
            && r.called_by.is_empty()
            && r.referenced_by.is_empty()
            && Some(r.name.as_str()) != entry_owner.as_deref()
        {
            let jumped = slot_reports.iter().any(|o| {
                o.jumps_to
                    .iter()
                    .any(|j| owner_of(m, j).as_deref() == Some(r.name.as_str()))
            });
            if !jumped {
                cx.push(
                    Severity::Info,
                    "D001",
                    Some(&r.name),
                    None,
                    "no caller, jump or address reference found (dead code, or reached indirectly)",
                );
            }
        }
    }

    let syscall_set = sites
        .iter()
        .filter_map(|s: &SyscallSite| s.name.clone())
        .collect();
    let mut findings = cx.findings;
    findings.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.addr.cmp(&b.addr)));
    Report {
        app: m.app.name.clone(),
        version: m.app.version.clone(),
        isa: m.target.isa.to_string(),
        tier: m.app.tier,
        exposure: m.app.exposure,
        size: image.bytes.len() as u64,
        sha256: sha256_hex(&image.bytes),
        reference: reference.map(|(l, _)| l.to_owned()),
        findings,
        slots: slot_reports,
        syscall_sites: sites,
        syscall_set,
    }
}

/// Slot that owns a symbol (slot name or label).
fn owner_of(m: &crate::manifest::Manifest, symbol: &str) -> Option<String> {
    m.slots
        .iter()
        .find(|s| s.name == symbol || s.labels.iter().any(|l| l.name == symbol))
        .map(|s| s.name.clone())
}

fn compare_reference(cx: &mut Ctx<'_>, image: &Image, label: &str, bytes: &[u8]) {
    if bytes == image.bytes.as_slice() {
        cx.push(
            Severity::Info,
            "R000",
            None,
            None,
            format!(
                "{label} is byte-identical to the map ({} bytes)",
                bytes.len()
            ),
        );
        return;
    }
    if bytes.len() != image.bytes.len() {
        cx.push(
            Severity::Error,
            "R001",
            None,
            None,
            format!(
                "{label} is {} bytes, the map builds {} bytes",
                bytes.len(),
                image.bytes.len()
            ),
        );
    }
    if let Some(off) = bytes.iter().zip(&image.bytes).position(|(a, b)| a != b) {
        let m = &cx.app.manifest;
        let owner = image
            .slots
            .iter()
            .find(|l| {
                (l.file_offset..l.file_offset + m.slots[l.index].size).contains(&(off as u64))
            })
            .map(|l| m.slots[l.index].name.clone());
        let diffs = bytes
            .iter()
            .zip(&image.bytes)
            .filter(|(a, b)| a != b)
            .count();
        cx.push(
            Severity::Error,
            "R001",
            owner.as_deref(),
            None,
            format!("{label} differs from the map: {diffs} byte(s) differ, first at file offset {off:#x}"),
        );
    }
}

fn check_format(cx: &mut Ctx<'_>, bytes: &[u8]) {
    let m = &cx.app.manifest;
    match m.target.format {
        Format::Raw => {
            let entry = m.symbol(&m.target.entry).unwrap_or(0);
            if m.slot_at(entry).is_none_or(|s| s.kind != SlotKind::Code) {
                cx.push(
                    Severity::Error,
                    "E006",
                    None,
                    Some(entry),
                    "entry point is not inside a code slot",
                );
            }
        }
        Format::Elf64 | Format::Elf32 => match elf::parse(bytes) {
            Err(e) => cx.push(
                Severity::Error,
                "E001",
                None,
                None,
                format!("ELF header unreadable: {e:#}"),
            ),
            Ok(h) => check_elf(cx, &h, bytes.len() as u64),
        },
    }
}

fn check_elf(cx: &mut Ctx<'_>, h: &elf::Elf, file_len: u64) {
    let m = &cx.app.manifest;
    let want64 = m.target.format == Format::Elf64;
    if h.class64 != want64 {
        cx.push(
            Severity::Error,
            "E001",
            None,
            None,
            "ELF class does not match target.format",
        );
    }
    let multiboot_x86_64 = m.target.isa == Isa::X86_64
        && m.target.os == Os::BareMetal
        && m.target.format == Format::Elf32
        && h.machine == 3;
    if h.machine != m.target.isa.elf_machine() && !multiboot_x86_64 {
        cx.push(
            Severity::Error,
            "E002",
            None,
            None,
            format!(
                "e_machine is {} but target.isa {} needs {}",
                h.machine,
                m.target.isa,
                m.target.isa.elf_machine()
            ),
        );
    }
    if h.e_type != 2 {
        cx.push(
            Severity::Warning,
            "E003",
            None,
            None,
            format!(
                "e_type is {} (expected 2 = ET_EXEC for fixed-slot images)",
                h.e_type
            ),
        );
    }
    let entry = m.symbol(&m.target.entry).unwrap_or(0);
    if h.entry != entry {
        cx.push(
            Severity::Error,
            "E004",
            None,
            Some(h.entry),
            format!(
                "e_entry {:#x} does not match target.entry `{}` at {entry:#x}",
                h.entry, m.target.entry
            ),
        );
    }
    if m.slot_at(entry).is_none_or(|s| s.kind != SlotKind::Code) {
        cx.push(
            Severity::Error,
            "E006",
            None,
            Some(entry),
            "entry point is not inside a code slot",
        );
    }
    for ph in &h.phdrs {
        match ph.p_type {
            elf::PT_INTERP => cx.push(
                Severity::Error,
                "E005",
                None,
                None,
                "PT_INTERP present: the binary asks for a dynamic loader",
            ),
            elf::PT_DYNAMIC => cx.push(
                Severity::Error,
                "E005",
                None,
                None,
                "PT_DYNAMIC present: the binary links shared libraries",
            ),
            elf::PT_GNU_STACK if ph.flags & 1 != 0 => cx.push(
                Severity::Error,
                "S006",
                None,
                None,
                "PT_GNU_STACK requests an executable stack",
            ),
            elf::PT_GNU_STACK if ph.offset != 0 || ph.vaddr != 0 || ph.filesz != 0 || ph.memsz != 0 => cx.push(
                Severity::Warning,
                "E013",
                None,
                None,
                format!("PT_GNU_STACK has non-zero offset/vaddr/filesz/memsz (memsz {:#x}): a field is probably misplaced", ph.memsz),
            ),
            elf::PT_LOAD | elf::PT_GNU_STACK => {}
            other => cx.push(
                Severity::Warning,
                "E007",
                None,
                None,
                format!("unexpected program header type {other:#x}"),
            ),
        }
    }
    if h.shoff != 0 || h.shnum != 0 {
        cx.push(Severity::Warning, "E012", None, None, format!("section header table present (e_shoff {:#x}, {} sections): typical of compiler/linker output, not of a hand-authored image", h.shoff, h.shnum));
    }
    if m.target.os == Os::Linux && !h.phdrs.iter().any(|p| p.p_type == elf::PT_GNU_STACK) {
        cx.push(
            Severity::Warning,
            "S003",
            None,
            None,
            "no PT_GNU_STACK header: declare the stack non-executable explicitly (rw-)",
        );
    }
    let loads: Vec<&elf::Phdr> = h
        .phdrs
        .iter()
        .filter(|p| p.p_type == elf::PT_LOAD)
        .collect();
    let mut segs: Vec<&crate::manifest::Segment> = m.segments.iter().collect();
    segs.sort_by_key(|s| s.vaddr);
    if loads.len() != segs.len() {
        cx.push(
            Severity::Error,
            "E008",
            None,
            None,
            format!(
                "{} PT_LOAD headers but {} segments in the map",
                loads.len(),
                segs.len()
            ),
        );
    }
    for (ph, seg) in loads.iter().zip(&segs) {
        let mut diffs = Vec::new();
        if ph.vaddr != seg.vaddr {
            diffs.push(format!("vaddr {:#x}≠{:#x}", ph.vaddr, seg.vaddr));
        }
        if ph.offset != seg.offset {
            diffs.push(format!("offset {:#x}≠{:#x}", ph.offset, seg.offset));
        }
        if ph.filesz != seg.file_size {
            diffs.push(format!("filesz {:#x}≠{:#x}", ph.filesz, seg.file_size));
        }
        if ph.memsz != seg.mem_size {
            diffs.push(format!("memsz {:#x}≠{:#x}", ph.memsz, seg.mem_size));
        }
        if ph.flags != seg.elf_flags() {
            diffs.push(format!("flags {}≠{}", elf::perms(ph.flags), seg.perms));
        }
        if seg.align.is_some_and(|a| a != ph.align) {
            diffs.push(format!(
                "align {:#x}≠{:#x}",
                ph.align,
                seg.align.unwrap_or(0)
            ));
        }
        if !diffs.is_empty() {
            cx.push(
                Severity::Error,
                "E009",
                None,
                Some(ph.vaddr),
                format!(
                    "PT_LOAD for segment `{}` disagrees with the map: {}",
                    seg.name,
                    diffs.join(", ")
                ),
            );
        }
        if ph.align > 1 && ph.vaddr % ph.align != ph.offset % ph.align {
            cx.push(
                Severity::Error,
                "E010",
                None,
                Some(ph.vaddr),
                format!(
                    "segment `{}`: p_vaddr and p_offset are not congruent modulo p_align",
                    seg.name
                ),
            );
        }
        if m.target.isa == crate::manifest::Isa::Aarch64
            && m.target.os == Os::Linux
            && (ph.align < 0x1_0000 || ph.vaddr % 0x1_0000 != ph.offset % 0x1_0000)
        {
            cx.push(
                Severity::Warning,
                "S004",
                None,
                Some(ph.vaddr),
                format!("segment `{}` is not 64 KiB-aligned: it may fail on 16K/64K-page kernels (Raspberry Pi 5 uses 16K pages)", seg.name),
            );
        }
    }
    let loaded_end = loads.iter().map(|p| p.offset + p.filesz).max().unwrap_or(0);
    if file_len > loaded_end {
        cx.push(
            Severity::Warning,
            "E011",
            None,
            None,
            format!(
                "{} trailing bytes after the last loaded byte ({loaded_end:#x}) are never mapped",
                file_len - loaded_end
            ),
        );
    } else if file_len < loaded_end {
        cx.push(
            Severity::Error,
            "E011",
            None,
            None,
            format!("file is {file_len} bytes but PT_LOAD expects {loaded_end}"),
        );
    }
}

fn check_policy(cx: &mut Ctx<'_>) {
    let m = &cx.app.manifest;
    let exposed = cx.exposed();
    for seg in &m.segments {
        if seg.writable() && seg.executable() {
            let sev = if m.app.tier >= Tier::L2 || exposed {
                Severity::Error
            } else {
                Severity::Warning
            };
            cx.push(sev, "S001", None, Some(seg.vaddr), format!("segment `{}` is writable and executable (rwx); tier L2+ and exposed programs require W^X", seg.name));
        }
    }
    if exposed && m.app.tier < Tier::L2 {
        cx.push(
            Severity::Error,
            "S005",
            None,
            None,
            format!(
                "exposure `{:?}` requires tier L2 or higher (W^X), declared {:?}",
                m.app.exposure, m.app.tier
            ),
        );
    }
    if exposed && m.target.os == Os::Linux && m.syscalls.is_none() {
        cx.push(
            Severity::Error,
            "S002",
            None,
            None,
            "exposed Linux program without a [syscalls] allow-list",
        );
    }
    if let Some(sc) = &m.syscalls {
        for name in &sc.allow {
            if syscalls::number(m.target.isa, name).is_none() {
                cx.push(
                    Severity::Warning,
                    "S007",
                    None,
                    None,
                    format!(
                        "allow-listed syscall `{name}` is unknown for {} (extend syscalls.rs)",
                        m.target.isa
                    ),
                );
            }
        }
    }
    for slot in &m.slots {
        let Some(seg) = m.segment_at(slot.vaddr) else {
            if !m.segments.is_empty() {
                cx.push(
                    Severity::Error,
                    "K001",
                    Some(&slot.name),
                    Some(slot.vaddr),
                    "slot lies outside every segment",
                );
            }
            continue;
        };
        match slot.kind {
            SlotKind::Code if !seg.executable() => {
                cx.push(
                    Severity::Error,
                    "K001",
                    Some(&slot.name),
                    Some(slot.vaddr),
                    format!("code slot in non-executable segment `{}`", seg.name),
                );
            }
            SlotKind::Data if !seg.writable() => {
                cx.push(
                    Severity::Error,
                    "K001",
                    Some(&slot.name),
                    Some(slot.vaddr),
                    format!(
                        "data slot in read-only segment `{}` (use kind = \"rodata\")",
                        seg.name
                    ),
                );
            }
            _ => {}
        }
    }
}

fn check_gaps(cx: &mut Ctx<'_>, image: &Image) {
    let m = &cx.app.manifest;
    let mut covered: Vec<(u64, u64)> = image
        .slots
        .iter()
        .map(|l| (l.file_offset, l.file_offset + m.slots[l.index].size))
        .collect();
    covered.sort_unstable();
    let mut cursor = 0u64;
    let mut gaps = Vec::new();
    for (start, end) in covered {
        if start > cursor {
            gaps.push((cursor, start));
        }
        cursor = cursor.max(end);
    }
    if (image.bytes.len() as u64) > cursor {
        gaps.push((cursor, image.bytes.len() as u64));
    }
    for (s, e) in gaps {
        let fill = m.parse_data_fill().unwrap_or(0);
        let nonfill = image.bytes[s as usize..e as usize]
            .iter()
            .filter(|b| **b != fill)
            .count();
        let sev = if nonfill > 0 {
            Severity::Error
        } else {
            Severity::Warning
        };
        cx.push(sev, "G001", None, None, format!("file bytes {s:#x}..{e:#x} belong to no slot ({nonfill} non-fill bytes); declare them so every byte has an owner"));
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "one pass over a slot updates several shared tables"
)]
fn check_code_slot(
    cx: &mut Ctx<'_>,
    slot: &Slot,
    used: u64,
    d: &Decoded,
    intents: &[(u64, String)],
    targets: &BTreeSet<u64>,
    reports: &mut [SlotReport],
    sites: &mut Vec<SyscallSite>,
) {
    let m = &cx.app.manifest;
    let exposed = cx.exposed();
    let me = m
        .slots
        .iter()
        .position(|s| s.name == slot.name)
        .expect("slot exists");
    if d.coverage == Coverage::None {
        let sev = if exposed {
            Severity::Error
        } else {
            Severity::Warning
        };
        cx.push(sev, "X001", Some(&slot.name), Some(slot.vaddr), format!("no built-in decoder for {}: control flow and syscalls are unaudited, attach external evidence", m.target.isa));
        return;
    }
    let used_end = slot.vaddr + used;
    let starts: BTreeSet<u64> = d.insns.iter().map(|i| i.addr).collect();
    let entries: BTreeMap<u64, String> = m.entry_points().into_iter().collect();
    let state_base = m.target.state_base;
    let mut written: BTreeSet<String> = BTreeSet::new();
    let mut saved: BTreeSet<String> = BTreeSet::new();

    for (idx, insn) in d.insns.iter().enumerate() {
        let here = Some(insn.addr);
        if insn.flow == Flow::Invalid {
            cx.push(
                Severity::Error,
                "C001",
                Some(&slot.name),
                here,
                format!(
                    "undecodable bytes {}: {}",
                    hex::encode(&insn.bytes),
                    insn.text
                ),
            );
            continue;
        }
        if insn.end() > used_end {
            cx.push(
                Severity::Error,
                "C002",
                Some(&slot.name),
                here,
                "instruction runs past the authored bytes of the slot",
            );
        }
        written.extend(insn.writes.iter().cloned());
        saved.extend(insn.saves.iter().cloned());

        if let Some(t) = insn.flow.direct_target() {
            branch_target(cx, slot, me, insn, t, used_end, &starts, &entries, reports);
        }
        match insn.flow {
            Flow::IndirectJump | Flow::IndirectCall => {
                reports[me].indirect_branches += 1;
                if slot.indirect.is_none() {
                    let sev = if m.app.exposure == Exposure::Internet {
                        Severity::Error
                    } else {
                        Severity::Warning
                    };
                    cx.push(
                        sev,
                        "C006",
                        Some(&slot.name),
                        here,
                        format!(
                            "indirect branch `{}` without a justification (slot.indirect)",
                            insn.text
                        ),
                    );
                }
            }
            Flow::Syscall if m.target.os == Os::Linux => {
                let nr = resolve_syscall(&d.insns, idx, targets);
                let name = nr
                    .and_then(|n| syscalls::name(m.target.isa, n))
                    .map(str::to_owned);
                syscall_site(cx, slot, insn.addr, nr, name.as_deref());
                if let Some(n) = &name {
                    reports[me].syscalls.insert(n.clone());
                }
                sites.push(SyscallSite {
                    slot: slot.name.clone(),
                    addr: format!("{:#x}", insn.addr),
                    number: nr,
                    name,
                });
            }
            Flow::Trap => {
                cx.push(
                    Severity::Info,
                    "C013",
                    Some(&slot.name),
                    here,
                    format!("{} ({})", insn.text, insn.note.unwrap_or("trap")),
                );
            }
            _ => {}
        }
        for mr in &insn.mem {
            memory_access(cx, slot, insn, mr.addr, mr.write);
        }
        let mut seen_refs = BTreeSet::new();
        for a in &insn.addr_refs {
            if let Some(target) = m.slot_at(*a)
                && target.name != slot.name
            {
                reports[me].data_refs.insert(target.name.clone());
                if seen_refs.insert(*a) {
                    check_data_target(cx, slot, insn, target, *a);
                }
            }
        }
        for mr in &insn.mem {
            if let Some(target) = m.slot_at(mr.addr)
                && target.name != slot.name
            {
                reports[me].data_refs.insert(target.name.clone());
            }
        }
        for sr in &insn.state {
            let Some(base) = state_base else { continue };
            let addr = base.wrapping_add(sr.offset as u64);
            let field = m
                .state
                .iter()
                .find(|f| addr >= f.vaddr && addr < f.vaddr + f.size)
                .map(|f| f.name.clone());
            let label = field.unwrap_or_else(|| format!("+{:#x}", sr.offset));
            if sr.write {
                reports[me].state_writes.insert(label);
            } else {
                reports[me].state_reads.insert(label);
            }
            if sr.write && m.segment_at(addr).is_some_and(|s| !s.writable()) {
                cx.push(
                    Severity::Error,
                    "C010",
                    Some(&slot.name),
                    Some(insn.addr),
                    format!("state write to {addr:#x} hits a read-only segment"),
                );
            }
        }
    }

    check_intents(cx, slot, d, intents, &mut reports[me]);

    // Falling off the end slides through NOP padding into the next slot.
    if let Some(last) = d.insns.last() {
        let ends = last.flow.ends_block()
            || (last.flow == Flow::Syscall
                && resolve_syscall(&d.insns, d.insns.len() - 1, targets)
                    .and_then(|n| syscalls::name(m.target.isa, n))
                    .is_some_and(syscalls::is_noreturn))
            || (matches!(last.flow, Flow::Call(_)) && call_is_noreturn(m, last))
            || slot
                .contract
                .as_ref()
                .is_some_and(|c| c.noreturn && last.flow == Flow::Trap);
        if !ends {
            let next = m
                .slots
                .iter()
                .filter(|s| s.vaddr >= slot.end())
                .min_by_key(|s| s.vaddr);
            let into = next.map_or_else(
                || "past the end of the image".to_owned(),
                |n| format!("into `{}`", n.name),
            );
            if let Some(n) = next.filter(|n| n.kind == SlotKind::Code) {
                reports[me].jumps_to.insert(n.name.clone());
            }
            cx.push(
                Severity::Warning,
                "C015",
                Some(&slot.name),
                Some(last.addr),
                format!("execution falls through the slot end and slides over its NOP padding {into}: an implicit coupling; end with an explicit jmp/ret/exit"),
            );
        }
    }

    reports[me].writes.clone_from(&written);
    if d.coverage == Coverage::Full
        && let Some(c) = slot.contract.as_ref().filter(|c| !c.noreturn)
    {
        let declared: BTreeSet<String> = c
            .clobbers
            .iter()
            .chain(&c.outputs)
            .filter_map(|s| reg_token(s))
            .map(|r| canonical_contract_reg(m.target.isa, &r))
            .collect();
        let ignore = ["rsp", "rflags", "eflags"];
        let undeclared: Vec<&String> = written
            .iter()
            .filter(|r| {
                !declared.contains(*r) && !saved.contains(*r) && !ignore.contains(&r.as_str())
            })
            .collect();
        if !undeclared.is_empty() {
            cx.push(
                Severity::Warning,
                "C011",
                Some(&slot.name),
                Some(slot.vaddr),
                format!(
                    "writes {undeclared:?} which the contract neither clobbers, outputs nor saves"
                ),
            );
        }
    }
}

fn check_intents(
    cx: &mut Ctx<'_>,
    slot: &Slot,
    d: &Decoded,
    intents: &[(u64, String)],
    report: &mut SlotReport,
) {
    let m = &cx.app.manifest;
    let resolve = |name: &str| m.intent_symbol(name);
    report.intents = intents.len();
    for (off, text) in intents {
        let addr = slot.vaddr + off;
        let Some(insn) = d.insns.iter().find(|i| i.addr == addr) else {
            cx.push(
                Severity::Error,
                "I002",
                Some(&slot.name),
                Some(addr),
                format!("intent `{text}` sits at {addr:#x}, which does not start an instruction: the byte boundaries are not what the author meant"),
            );
            report.intent_mismatches += 1;
            continue;
        };
        if insn.text.is_empty() {
            continue;
        }
        if !intent::matches(&insn.text, text, &resolve) {
            cx.push(
                Severity::Warning,
                "I001",
                Some(&slot.name),
                Some(addr),
                format!(
                    "bytes decode as `{}` but the intent says `{text}`",
                    insn.text
                ),
            );
            report.intent_mismatches += 1;
        }
    }
}

/// Intents in the ELF header slot must sit on the field they name.
fn check_header_intents(cx: &mut Ctx<'_>, image: &Image) {
    let m = &cx.app.manifest;
    if !matches!(m.target.format, Format::Elf64 | Format::Elf32) {
        return;
    }
    let fields = elf::fields(&image.bytes);
    for ls in &image.slots {
        let slot = &m.slots[ls.index];
        if slot.kind != SlotKind::Header || ls.file_offset != 0 {
            continue;
        }
        for (off, text) in &ls.intents {
            let Some(field) = fields.iter().find(|f| f.offset == *off) else {
                if let Some(f) = fields
                    .iter()
                    .find(|f| *off > f.offset && *off < f.offset + f.len)
                {
                    cx.push(
                        Severity::Error,
                        "I002",
                        Some(&slot.name),
                        Some(slot.vaddr + off),
                        format!("intent `{text}` starts in the middle of `{}`: the header layout is not what the author meant", f.display()),
                    );
                }
                continue;
            };
            let named = text
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .find(|w| w.starts_with("e_") || w.starts_with("p_"));
            if let Some(w) = named
                && w != field.name
            {
                cx.push(
                    Severity::Error,
                    "I003",
                    Some(&slot.name),
                    Some(slot.vaddr + off),
                    format!(
                        "intent names `{w}` but these bytes are `{}`",
                        field.display()
                    ),
                );
            }
        }
    }
}

/// Declared `contract.calls` must match the calls found in the bytes.
fn check_contract_calls(cx: &mut Ctx<'_>, reports: &[SlotReport]) {
    let m = &cx.app.manifest;
    for slot in &m.slots {
        let Some(c) = &slot.contract else { continue };
        let Some(r) = reports.iter().find(|r| r.name == slot.name) else {
            continue;
        };
        let declared: BTreeSet<String> = c.calls.iter().cloned().collect();
        let missing: Vec<&String> = declared.difference(&r.calls).collect();
        let extra: Vec<&String> = r.calls.difference(&declared).collect();
        if !missing.is_empty() || !extra.is_empty() {
            cx.push(
                Severity::Warning,
                "C012",
                Some(&slot.name),
                Some(slot.vaddr),
                format!("contract.calls disagrees with the bytes: declared but not called {missing:?}, called but not declared {extra:?}"),
            );
        }
    }
}

fn call_is_noreturn(m: &crate::manifest::Manifest, insn: &Insn) -> bool {
    let Flow::Call(t) = insn.flow else {
        return false;
    };
    m.slots
        .iter()
        .any(|s| s.vaddr == t && s.contract.as_ref().is_some_and(|c| c.noreturn))
}

/// Leading register name of a contract entry such as `rdi: buffer`.
fn reg_token(s: &str) -> Option<String> {
    let tok = s.split([':', ' ', '=']).next()?.trim().to_lowercase();
    (!tok.is_empty()).then_some(tok)
}

/// Maps sub-register and narrower-vector contract names to the full register name reported by the
/// ISA decoder (for example `eax` -> `rax` and `xmm0`/`ymm0` -> `zmm0` on x86-64).
fn canonical_contract_reg(isa: Isa, tok: &str) -> String {
    if isa != Isa::X86_64 {
        return tok.to_owned();
    }
    match tok {
        "eax" | "ax" | "al" | "ah" => "rax".to_owned(),
        "ebx" | "bx" | "bl" | "bh" => "rbx".to_owned(),
        "ecx" | "cx" | "cl" | "ch" => "rcx".to_owned(),
        "edx" | "dx" | "dl" | "dh" => "rdx".to_owned(),
        "esi" | "si" | "sil" => "rsi".to_owned(),
        "edi" | "di" | "dil" => "rdi".to_owned(),
        "ebp" | "bp" | "bpl" => "rbp".to_owned(),
        "esp" | "sp" | "spl" => "rsp".to_owned(),
        _ => {
            if let Some(n) = tok.strip_prefix("xmm").or_else(|| tok.strip_prefix("ymm"))
                && n.parse::<u8>().is_ok_and(|idx| idx < 32)
            {
                return format!("zmm{n}");
            }
            if let Some(rest) = tok.strip_prefix('r')
                && let Some(num) = rest
                    .strip_suffix('d')
                    .or_else(|| rest.strip_suffix('w'))
                    .or_else(|| rest.strip_suffix('b'))
                && num.parse::<u8>().is_ok_and(|idx| (8..=15).contains(&idx))
            {
                return format!("r{num}");
            }
            tok.to_owned()
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "branch classification needs the slot context"
)]
fn branch_target(
    cx: &mut Ctx<'_>,
    slot: &Slot,
    me: usize,
    insn: &Insn,
    t: u64,
    used_end: u64,
    starts: &BTreeSet<u64>,
    entries: &BTreeMap<u64, String>,
    reports: &mut [SlotReport],
) {
    let m = &cx.app.manifest;
    let here = Some(insn.addr);
    if slot.contains(t) {
        if t >= used_end {
            cx.push(
                Severity::Error,
                "C003",
                Some(&slot.name),
                here,
                format!("branch to {t:#x} lands in the slot's padding"),
            );
        } else if !starts.contains(&t) {
            cx.push(
                Severity::Error,
                "C004",
                Some(&slot.name),
                here,
                format!("branch to {t:#x} lands inside another instruction (overlapping code)"),
            );
        }
        if matches!(insn.flow, Flow::Call(_)) && t != slot.vaddr && !entries.contains_key(&t) {
            cx.push(
                Severity::Info,
                "C016",
                Some(&slot.name),
                here,
                format!("local call to {t:#x}; consider a label for the map"),
            );
        }
        return;
    }
    let Some(target_slot) = m.slot_at(t) else {
        cx.push(
            Severity::Error,
            "C003",
            Some(&slot.name),
            here,
            format!("branch to {t:#x} leaves every declared slot"),
        );
        return;
    };
    if target_slot.kind != SlotKind::Code {
        cx.push(
            Severity::Error,
            "C003",
            Some(&slot.name),
            here,
            format!(
                "branch to {t:#x} lands in {} slot `{}`",
                format!("{:?}", target_slot.kind).to_lowercase(),
                target_slot.name
            ),
        );
        return;
    }
    let Some(name) = entries.get(&t) else {
        cx.push(
            Severity::Error,
            "C005",
            Some(&slot.name),
            here,
            format!("cross-slot branch to {t:#x} inside `{}` is not a declared entry (add a label with entry = true)", target_slot.name),
        );
        return;
    };
    if matches!(insn.flow, Flow::Call(_)) {
        reports[me].calls.insert(name.clone());
    } else {
        reports[me].jumps_to.insert(name.clone());
    }
}

fn syscall_site(cx: &mut Ctx<'_>, slot: &Slot, addr: u64, nr: Option<u64>, name: Option<&str>) {
    let m = &cx.app.manifest;
    let allow = m.syscalls.as_ref().map(|s| &s.allow);
    match (nr, name) {
        (Some(n), Some(name)) => {
            if allow.is_some_and(|a| !a.iter().any(|x| x == name)) {
                cx.push(
                    Severity::Error,
                    "C008",
                    Some(&slot.name),
                    Some(addr),
                    format!("syscall `{name}` ({n}) is not in the [syscalls] allow-list"),
                );
            }
        }
        (Some(n), None) => {
            let sev = if allow.is_some() {
                Severity::Error
            } else {
                Severity::Warning
            };
            cx.push(
                sev,
                "C008",
                Some(&slot.name),
                Some(addr),
                format!(
                    "syscall number {n} is unknown to the {} table",
                    m.target.isa
                ),
            );
        }
        (None, _) => {
            let sev = if slot.dynamic_syscalls.is_some() {
                Severity::Warning
            } else if allow.is_some() {
                Severity::Error
            } else {
                Severity::Warning
            };
            cx.push(
                sev,
                "C007",
                Some(&slot.name),
                Some(addr),
                "syscall number cannot be resolved statically in this basic block",
            );
        }
    }
}

/// When a `rodata`/`data` slot declares labels, an address pointing inside it must match the slot
/// start or a declared label; catches off-by-N string/table pointer mistakes that `I001` cannot see.
fn check_data_target(cx: &mut Ctx<'_>, slot: &Slot, insn: &Insn, target: &Slot, addr: u64) {
    if !matches!(target.kind, SlotKind::Rodata | SlotKind::Data) || target.labels.is_empty() {
        return;
    }
    if addr == target.vaddr
        || target
            .labels
            .iter()
            .any(|l| target.vaddr + l.offset == addr)
    {
        return;
    }
    let nearest = target
        .labels
        .iter()
        .map(|l| (target.vaddr + l.offset, l.name.as_str()))
        .chain(std::iter::once((target.vaddr, target.name.as_str())))
        .min_by_key(|(a, _)| a.abs_diff(addr));
    let hint = nearest.map_or_else(String::new, |(na, name)| {
        let diff = i128::from(na) - i128::from(addr);
        format!("; nearest label `{name}` is at {na:#x} ({diff:+} byte(s))")
    });
    cx.push(
        Severity::Warning,
        "C017",
        Some(&slot.name),
        Some(insn.addr),
        format!(
            "address {addr:#x} lands inside labelled {} slot `{}` without matching any declared label{hint}",
            format!("{:?}", target.kind).to_lowercase(),
            target.name
        ),
    );
}

fn memory_access(cx: &mut Ctx<'_>, slot: &Slot, insn: &Insn, addr: u64, write: bool) {
    let m = &cx.app.manifest;
    let in_mmio = m
        .mmio
        .iter()
        .any(|w| addr >= w.base && addr < w.base + w.size);
    if in_mmio {
        return;
    }
    let Some(seg) = m.segment_at(addr) else {
        if !m.segments.is_empty() || m.target.os == Os::BareMetal {
            let sev = if m.target.os == Os::BareMetal {
                Severity::Error
            } else {
                Severity::Warning
            };
            cx.push(
                sev,
                "C009",
                Some(&slot.name),
                Some(insn.addr),
                format!(
                    "`{}` accesses {addr:#x}, outside every segment and MMIO window",
                    insn.text
                ),
            );
        }
        return;
    };
    if write && !seg.writable() {
        cx.push(
            Severity::Error,
            "C010",
            Some(&slot.name),
            Some(insn.addr),
            format!(
                "`{}` writes {addr:#x} in read-only segment `{}` (faults at run time)",
                insn.text, seg.name
            ),
        );
    }
    if write && let Some(target) = m.slot_at(addr).filter(|s| s.kind == SlotKind::Code) {
        let allowed = target.self_modifying.is_some() && m.app.tier < Tier::L2;
        let sev = if allowed {
            Severity::Warning
        } else {
            Severity::Error
        };
        cx.push(
            sev,
            "C010",
            Some(&slot.name),
            Some(insn.addr),
            format!("self-modifying write into code slot `{}`", target.name),
        );
    }
}

/// Walks back from a syscall to the instruction that set the syscall-number register.
pub(crate) fn resolve_syscall(insns: &[Insn], idx: usize, targets: &BTreeSet<u64>) -> Option<u64> {
    if targets.contains(&insns[idx].addr) {
        return None;
    }
    let mut i = idx;
    while i > 0 {
        i -= 1;
        let insn = &insns[i];
        if insn.flow.ends_block()
            || matches!(
                insn.flow,
                Flow::Call(_) | Flow::IndirectCall | Flow::Syscall
            )
        {
            return None;
        }
        match insn.syscall_reg {
            SyscallReg::Const(n) => return Some(n),
            SyscallReg::Unknown => return None,
            SyscallReg::Untouched => {}
        }
        if targets.contains(&insn.addr) {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_x86_64_contract_registers() {
        assert_eq!(canonical_contract_reg(Isa::X86_64, "eax"), "rax");
        assert_eq!(canonical_contract_reg(Isa::X86_64, "r11d"), "r11");
        assert_eq!(canonical_contract_reg(Isa::X86_64, "xmm0"), "zmm0");
        assert_eq!(canonical_contract_reg(Isa::X86_64, "ymm15"), "zmm15");
        assert_eq!(canonical_contract_reg(Isa::X86_64, "st0"), "st0");
        assert_eq!(canonical_contract_reg(Isa::Aarch64, "x0"), "x0");
    }
}
