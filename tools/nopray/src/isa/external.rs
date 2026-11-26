//! Optional `objdump` text for annotation. Never used to decide audit results.

use std::collections::BTreeMap;
use std::process::Command;

use crate::manifest::Isa;

fn run(isa: Isa, bytes: &[u8], addr: u64) -> Option<String> {
    let dir = std::env::temp_dir().join(format!("nopray-objdump-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("slot-{addr:x}.bin"));
    std::fs::write(&path, bytes).ok()?;
    let mut cmd = Command::new("objdump");
    cmd.args([
        "-D",
        "-b",
        "binary",
        "-m",
        isa.objdump_arch(),
        "-M",
        "no-aliases",
    ]);
    if isa == Isa::Thumb {
        cmd.args(["-M", "force-thumb"]);
    }
    cmd.arg(format!("--adjust-vma={addr:#x}")).arg(&path);
    let output = cmd.output().ok();
    let _ = std::fs::remove_file(&path);
    let output = output?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_lines(text: &str) -> Vec<(u64, Vec<u8>, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(a), Some(b), Some(t)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let Some(a) = a.trim().strip_suffix(':') else {
            continue;
        };
        let Ok(addr) = u64::from_str_radix(a, 16) else {
            continue;
        };
        let mut bytes = Vec::new();
        for tok in b.split_whitespace() {
            let Ok(v) = hex::decode(tok) else { continue };
            // objdump prints multi-byte words for fixed-width ISAs in instruction order (big-endian text)
            bytes.extend(v.iter().rev());
        }
        out.push((addr, bytes, clean(t)));
    }
    out
}

/// Drops objdump's own trailing comments (`# 0x400140`, `// #1`, `<sym>`) so the text is the
/// bare instruction, comparable with authoring intents.
fn clean(t: &str) -> String {
    let joined = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut cut = joined.len();
    for pat in [" # ", " //", " <"] {
        if let Some(p) = joined.find(pat) {
            cut = cut.min(p);
        }
    }
    joined[..cut].trim_end().to_owned()
}

pub(super) fn objdump_text(isa: Isa, bytes: &[u8], addr: u64) -> Option<BTreeMap<u64, String>> {
    let text = run(isa, bytes, addr)?;
    Some(
        parse_lines(&text)
            .into_iter()
            .map(|(a, _, t)| (a, t))
            .collect(),
    )
}

pub(super) fn objdump_listing(
    isa: Isa,
    bytes: &[u8],
    addr: u64,
) -> Option<Vec<(u64, Vec<u8>, String)>> {
    let text = run(isa, bytes, addr)?;
    let lines = parse_lines(&text);
    (!lines.is_empty()).then_some(lines)
}
