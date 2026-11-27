//! `nopray context`: everything an agent needs to change one slot, without the whole program.
//!
//! The output joins the manifest entry (purpose, contract, labels, origin), the audit facts
//! (callers, callees, data and state touched, syscalls, findings), the room left to grow in
//! place, the neighbouring slots and the symbolic fields that point into or out of the slot.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use anyhow::{Result, bail};
use serde::Serialize;

use crate::audit::{Finding, Report, SlotReport};
use crate::image::Image;
use crate::manifest::{AppDir, Contract, Origin, Slot};

#[derive(Debug, Clone, Serialize)]
pub struct LabelInfo {
    pub name: String,
    pub addr: String,
    pub entry: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Neighbour {
    pub name: String,
    pub vaddr: String,
    pub end: String,
    /// Bytes between this slot and the neighbour (unmapped gap).
    pub gap: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldInfo {
    /// Slot holding the field.
    pub slot: String,
    pub addr: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StateInfo {
    pub name: String,
    pub vaddr: String,
    pub size: u64,
    pub read: bool,
    pub written: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SlotContext {
    pub app: String,
    pub isa: String,
    pub slot: String,
    pub kind: String,
    pub file: String,
    pub vaddr: String,
    pub end: String,
    pub size: u64,
    pub used: u64,
    /// Room to grow in place without moving any other slot.
    pub free: u64,
    pub purpose: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract: Option<Contract>,
    pub labels: Vec<LabelInfo>,
    pub audit: SlotReport,
    /// Slots to re-test after a change here: callers, jumpers and address referrers.
    pub retest: BTreeSet<String>,
    pub state: Vec<StateInfo>,
    /// Symbolic fields inside this slot.
    pub fields_out: Vec<FieldInfo>,
    /// Symbolic fields elsewhere that resolve into this slot (they follow it if it moves).
    pub fields_in: Vec<FieldInfo>,
    pub previous: Option<Neighbour>,
    pub next: Option<Neighbour>,
    pub findings: Vec<Finding>,
}

fn hex(v: u64) -> String {
    format!("{v:#x}")
}

/// Builds the context of the slot that owns `symbol` (a slot or label name).
pub fn build(app: &AppDir, image: &Image, report: &Report, symbol: &str) -> Result<SlotContext> {
    let m = &app.manifest;
    let Some(index) = m
        .slots
        .iter()
        .position(|s| s.name == symbol || s.labels.iter().any(|l| l.name == symbol))
    else {
        bail!("`{symbol}` is neither a slot nor a label in nopray.toml");
    };
    let slot: &Slot = &m.slots[index];
    let r = report.slots[index].clone();
    let names: BTreeSet<&str> = std::iter::once(slot.name.as_str())
        .chain(slot.labels.iter().map(|l| l.name.as_str()))
        .collect();
    let mut retest: BTreeSet<String> = r.called_by.union(&r.referenced_by).cloned().collect();
    for o in &report.slots {
        if o.name != slot.name && o.jumps_to.iter().any(|j| names.contains(j.as_str())) {
            retest.insert(o.name.clone());
        }
    }
    let state = m
        .state
        .iter()
        .filter(|f| r.state_reads.contains(&f.name) || r.state_writes.contains(&f.name))
        .map(|f| StateInfo {
            name: f.name.clone(),
            vaddr: hex(f.vaddr),
            size: f.size,
            read: r.state_reads.contains(&f.name),
            written: r.state_writes.contains(&f.name),
        })
        .collect();
    let mut fields_out = Vec::new();
    let mut fields_in = Vec::new();
    for ls in &image.slots {
        let owner = &m.slots[ls.index];
        for f in &ls.fixups {
            let info = FieldInfo {
                slot: owner.name.clone(),
                addr: hex(owner.vaddr + f.offset),
                token: f.token.clone(),
            };
            if ls.index == index {
                fields_out.push(info);
            } else if names.contains(f.target.symbol.as_str()) {
                fields_in.push(info);
            }
        }
    }
    let mut by_addr: Vec<&Slot> = m.slots.iter().collect();
    by_addr.sort_by_key(|s| s.vaddr);
    let pos = by_addr
        .iter()
        .position(|s| s.name == slot.name)
        .expect("slot is in the manifest");
    let previous = pos.checked_sub(1).map(|p| by_addr[p]).map(|p| Neighbour {
        name: p.name.clone(),
        vaddr: hex(p.vaddr),
        end: hex(p.end()),
        gap: slot.vaddr.saturating_sub(p.end()),
    });
    let next = by_addr.get(pos + 1).map(|n| Neighbour {
        name: n.name.clone(),
        vaddr: hex(n.vaddr),
        end: hex(n.end()),
        gap: n.vaddr.saturating_sub(slot.end()),
    });
    let findings = report
        .findings
        .iter()
        .filter(|f| f.slot.as_deref() == Some(slot.name.as_str()))
        .cloned()
        .collect();
    Ok(SlotContext {
        app: format!("{} {}", m.app.name, m.app.version),
        isa: m.target.isa.to_string(),
        slot: slot.name.clone(),
        kind: r.kind.clone(),
        file: slot.file.clone(),
        vaddr: hex(slot.vaddr),
        end: hex(slot.end()),
        size: slot.size,
        used: r.used,
        free: slot.size - r.used,
        purpose: slot.purpose.clone(),
        origin: slot.origin,
        evidence: slot.evidence.clone(),
        contract: slot.contract.clone(),
        labels: slot
            .labels
            .iter()
            .map(|l| LabelInfo {
                name: l.name.clone(),
                addr: hex(slot.vaddr + l.offset),
                entry: l.entry,
                note: l.note.clone(),
            })
            .collect(),
        audit: r,
        retest,
        state,
        fields_out,
        fields_in,
        previous,
        next,
        findings,
    })
}

fn list<'a>(items: impl IntoIterator<Item = &'a String>) -> String {
    let v: Vec<String> = items.into_iter().map(|x| format!("`{x}`")).collect();
    if v.is_empty() {
        "-".into()
    } else {
        v.join(", ")
    }
}

/// Compact Markdown rendering for agents and humans.
#[must_use]
pub fn render(c: &SlotContext) -> String {
    let mut s = String::new();
    let a = &c.audit;
    let _ = writeln!(s, "# `{}` · {} · {} · {}\n", c.slot, c.kind, c.isa, c.app);
    let _ = writeln!(s, "- File: `{}`", c.file);
    let _ = writeln!(
        s,
        "- Range: `{}`..`{}` · used {} / {} bytes · **{} free to grow in place**",
        c.vaddr, c.end, c.used, c.size, c.free
    );
    let _ = writeln!(s, "- Purpose: {}", c.purpose);
    if c.origin.is_some() || c.evidence.is_some() {
        let _ = writeln!(
            s,
            "- Origin: {} · evidence: {}",
            c.origin
                .map_or_else(|| "undeclared".into(), |o| format!("{o:?}").to_lowercase()),
            c.evidence.as_deref().unwrap_or("-")
        );
    }
    if let Some(k) = &c.contract {
        let _ = writeln!(
            s,
            "- Contract: in {} · out {} · clobbers {} · calls {}{}",
            list(&k.inputs),
            list(&k.outputs),
            list(&k.clobbers),
            list(&k.calls),
            if k.noreturn { " · noreturn" } else { "" }
        );
    }
    for l in &c.labels {
        let _ = writeln!(
            s,
            "- Label `{}` at `{}`{}{}",
            l.name,
            l.addr,
            if l.entry { " (entry)" } else { "" },
            l.note
                .as_deref()
                .map(|n| format!(": {n}"))
                .unwrap_or_default()
        );
    }
    if let Some(cov) = a.coverage {
        let _ = writeln!(
            s,
            "- Decoded: {} instructions · coverage {cov:?} · intents {} ({} mismatched)",
            a.instructions, a.intents, a.intent_mismatches
        );
    }
    let _ = writeln!(
        s,
        "- Calls: {} · jumps to: {}",
        list(&a.calls),
        list(&a.jumps_to)
    );
    let _ = writeln!(
        s,
        "- Data used: {} · syscalls: {}",
        list(&a.data_refs),
        list(&a.syscalls)
    );
    if !a.writes.is_empty() {
        let _ = writeln!(s, "- Registers written: {}", list(&a.writes));
    }
    for f in &c.state {
        let _ = writeln!(
            s,
            "- State `{}` at `{}` ({} bytes): {}",
            f.name,
            f.vaddr,
            f.size,
            match (f.read, f.written) {
                (true, true) => "read + written",
                (true, false) => "read",
                _ => "written",
            }
        );
    }
    let _ = writeln!(s, "- **Re-test after a change:** {}", list(&c.retest));
    for (title, fields) in [
        ("Symbolic fields here", &c.fields_out),
        (
            "Symbolic fields pointing here (follow a move)",
            &c.fields_in,
        ),
    ] {
        if !fields.is_empty() {
            let v: Vec<String> = fields
                .iter()
                .map(|f| format!("`{}` {} {}", f.slot, f.addr, f.token))
                .collect();
            let _ = writeln!(s, "- {title}: {}", v.join("; "));
        }
    }
    for (title, n) in [("Previous", &c.previous), ("Next", &c.next)] {
        if let Some(n) = n {
            let _ = writeln!(
                s,
                "- {title} slot: `{}` `{}`..`{}` (gap {} bytes)",
                n.name, n.vaddr, n.end, n.gap
            );
        }
    }
    if c.findings.is_empty() {
        let _ = writeln!(s, "- Findings: none");
    } else {
        let _ = writeln!(s, "- Findings:");
        for f in &c.findings {
            let _ = writeln!(
                s,
                "  - {} {}{}: {}",
                f.severity,
                f.code,
                f.addr.map(|x| format!(" @{x:#x}")).unwrap_or_default(),
                f.message
            );
        }
    }
    s
}
