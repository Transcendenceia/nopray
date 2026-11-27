//! Slot-level change review between two versions of an application.

use std::fmt::Write as _;

use anyhow::Result;
use similar::TextDiff;

use crate::annotate;
use crate::audit;
use crate::image;
use crate::manifest::AppDir;

/// Strips comments so the diff shows only bytes plus regenerated disassembly text.
fn listing(app: &AppDir, name: &str, img: &image::Image) -> String {
    let Some((i, slot)) = app
        .manifest
        .slots
        .iter()
        .enumerate()
        .find(|(_, s)| s.name == name)
    else {
        return String::new();
    };
    let ls = img
        .slots
        .iter()
        .find(|l| l.index == i)
        .expect("slot loaded");
    annotate::render_slot(app, slot, &ls.bytes, &ls.intents, &ls.fixups)
        .lines()
        .filter(|l| !l.starts_with("# nopray slot") && !l.starts_with("# purpose"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Markdown report of what changed from `old` to `new` and what must be re-verified.
pub fn run(old: &AppDir, new: &AppDir) -> Result<String> {
    let old_img = image::build(old)?;
    let new_img = image::build(new)?;
    let new_report = audit::run(new, &new_img, None);
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# nopray diff · {} {} → {}\n",
        new.manifest.app.name, old.manifest.app.version, new.manifest.app.version
    );
    let _ = writeln!(
        s,
        "image: {} → {} bytes · sha256 {} → {}\n",
        old_img.bytes.len(),
        new_img.bytes.len(),
        &image::sha256_hex(&old_img.bytes)[..16],
        &image::sha256_hex(&new_img.bytes)[..16]
    );
    let mut changed = Vec::new();
    for (i, slot) in new.manifest.slots.iter().enumerate() {
        let new_hash = image::sha256_hex(new_img.padded(new, i));
        match old
            .manifest
            .slots
            .iter()
            .enumerate()
            .find(|(_, o)| o.name == slot.name)
        {
            None => {
                let _ = writeln!(
                    s,
                    "- **added** `{}` at `{:#x}` ({} bytes)",
                    slot.name, slot.vaddr, slot.size
                );
                changed.push(slot.name.clone());
            }
            Some((oi, o)) => {
                if o.vaddr != slot.vaddr || o.size != slot.size {
                    let _ = writeln!(
                        s,
                        "- **moved/resized** `{}`: `{:#x}`+{} → `{:#x}`+{} (every reference to it must be re-checked)",
                        slot.name, o.vaddr, o.size, slot.vaddr, slot.size
                    );
                    changed.push(slot.name.clone());
                } else if image::sha256_hex(old_img.padded(old, oi)) != new_hash {
                    let _ = writeln!(s, "- **changed** `{}`", slot.name);
                    changed.push(slot.name.clone());
                }
            }
        }
    }
    for o in &old.manifest.slots {
        if !new.manifest.slots.iter().any(|n| n.name == o.name) {
            let _ = writeln!(s, "- **removed** `{}`", o.name);
        }
    }
    if changed.is_empty() {
        let _ = writeln!(s, "No slot changed.");
        return Ok(s);
    }
    let mut retest: Vec<String> = Vec::new();
    for name in &changed {
        if let Some(r) = new_report.slots.iter().find(|r| &r.name == name) {
            retest.extend(r.called_by.iter().cloned());
            retest.extend(r.referenced_by.iter().cloned());
        }
    }
    retest.sort();
    retest.dedup();
    retest.retain(|r| !changed.contains(r));
    let _ = writeln!(
        s,
        "\nRe-verify callers/referrers: {}\n",
        if retest.is_empty() {
            "-".into()
        } else {
            retest.join(", ")
        }
    );
    for name in &changed {
        let a = listing(old, name, &old_img);
        let b = listing(new, name, &new_img);
        let diff = TextDiff::from_lines(&a, &b);
        let _ = writeln!(
            s,
            "## `{name}`\n\n```diff\n{}```\n",
            diff.unified_diff().context_radius(3).header("old", "new")
        );
    }
    let _ = writeln!(
        s,
        "New audit: {} errors · {} warnings",
        new_report.count(audit::Severity::Error),
        new_report.count(audit::Severity::Warning)
    );
    Ok(s)
}
