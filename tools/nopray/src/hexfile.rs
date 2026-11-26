//! Slot source files: hexadecimal bytes with `#` comments.
//!
//! Only the hex digits before a `#` on each line are authoritative. Comments, whitespace and
//! line structure never change the bytes, so annotations can be regenerated freely.

use std::fmt::Write as _;

use anyhow::{Result, bail};

/// Parses a slot file into bytes. Accepts hex digit pairs separated by any whitespace.
pub fn parse(text: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for (lineno, line) in text.lines().enumerate() {
        let code = line.split('#').next().unwrap_or("");
        let digits: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        if digits.is_empty() {
            continue;
        }
        if let Some(bad) = digits.chars().find(|c| !c.is_ascii_hexdigit()) {
            bail!(
                "line {}: `{bad}` is not a hex digit (comments must start with `#`)",
                lineno + 1
            );
        }
        if digits.len() % 2 != 0 {
            bail!("line {}: odd number of hex digits", lineno + 1);
        }
        out.extend(hex::decode(&digits).expect("validated hex digits"));
    }
    Ok(out)
}

/// Authoring intents: `(byte offset, text)` for every byte line carrying `#= text`.
///
/// The offset is the slot offset of the first byte on that line. Assumes `text` already
/// passed [`parse`].
#[must_use]
pub fn parse_intents(text: &str) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    let mut offset = 0u64;
    for line in text.lines() {
        let code = line.split('#').next().unwrap_or("");
        let n = code.chars().filter(char::is_ascii_hexdigit).count() as u64 / 2;
        if n > 0
            && let Some(pos) = line.find("#=")
        {
            let intent = line[pos + 2..].trim();
            if !intent.is_empty() {
                out.push((offset, intent.to_owned()));
            }
        }
        offset += n;
    }
    out
}

/// One rendered line of an annotated slot listing.
#[derive(Debug, Clone)]
pub struct ListingLine {
    pub addr: u64,
    pub bytes: Vec<u8>,
    pub text: String,
}

/// Renders an annotated slot file. `labels` are `(address, name)` pairs printed as headings.
#[must_use]
/// `intents` are `(absolute address, text)` pairs re-attached to the line that holds them.
pub fn render(
    header: &[String],
    lines: &[ListingLine],
    labels: &[(u64, String)],
    intents: &[(u64, String)],
) -> String {
    let mut s = String::new();
    for h in header {
        let _ = writeln!(s, "# {h}");
    }
    let width = lines
        .iter()
        .map(|l| l.bytes.len() * 3)
        .max()
        .unwrap_or(0)
        .clamp(12, 48);
    for line in lines {
        for (_, name) in labels.iter().filter(|(a, _)| *a == line.addr) {
            let _ = writeln!(s, "#\n# {name}:");
        }
        let end = line.addr + line.bytes.len() as u64;
        // Split the line wherever an intent starts, so every intent keeps its exact offset.
        let mut cuts: Vec<u64> = intents
            .iter()
            .map(|(a, _)| *a)
            .filter(|a| *a > line.addr && *a < end)
            .collect();
        cuts.sort_unstable();
        cuts.dedup();
        let mut bounds = vec![line.addr];
        bounds.extend(cuts);
        bounds.push(end);
        for (k, pair) in bounds.windows(2).enumerate() {
            let (from, to) = (pair[0], pair[1]);
            let chunk = &line.bytes[(from - line.addr) as usize..(to - line.addr) as usize];
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            let hex = hex.join(" ");
            let notes: Vec<&str> = intents
                .iter()
                .filter(|(a, _)| *a == from)
                .map(|(_, t)| t.as_str())
                .collect();
            let intent = if notes.is_empty() {
                String::new()
            } else {
                format!("  #= {}", notes.join(" ; "))
            };
            let text = if k == 0 {
                line.text.as_str()
            } else {
                "(continued: an intent starts inside this item)"
            };
            if text.is_empty() && intent.is_empty() {
                let _ = writeln!(s, "{hex}");
            } else {
                let _ = writeln!(s, "{hex:<width$} # {from:#010x}  {text}{intent}");
            }
        }
    }
    let covered = |a: u64| {
        lines
            .iter()
            .any(|l| a >= l.addr && a < l.addr + l.bytes.len() as u64)
    };
    for (a, t) in intents.iter().filter(|(a, _)| !covered(*a)) {
        let _ = writeln!(s, "# ORPHAN INTENT at {a:#x} (no byte there any more): {t}");
    }
    s
}

/// Splits data bytes into 16-byte lines with an ASCII preview.
#[must_use]
pub fn data_lines(base: u64, bytes: &[u8]) -> Vec<ListingLine> {
    bytes
        .chunks(16)
        .enumerate()
        .map(|(i, chunk)| {
            let ascii: String = chunk
                .iter()
                .map(|&b| {
                    if (0x20..0x7f).contains(&b) && b != b'#' {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            ListingLine {
                addr: base + (i as u64) * 16,
                bytes: chunk.to_vec(),
                text: format!("|{ascii}|"),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_whitespace_do_not_change_bytes() {
        let a = parse("b8 01 00 00 00 # mov eax,1\n0f05\n# full comment\n").expect("valid");
        assert_eq!(a, vec![0xb8, 1, 0, 0, 0, 0x0f, 0x05]);
    }

    #[test]
    fn rejects_text_outside_comments() {
        assert!(
            parse("b8 01 add\n").is_err(),
            "letters outside comments must be rejected"
        );
        assert!(parse("b8 0\n").is_err(), "odd digit count must be rejected");
    }

    #[test]
    fn render_round_trips() {
        let lines = data_lines(0x1000, b"NOPRAY #1\x00\x01");
        let text = render(
            &["slot".into()],
            &lines,
            &[(0x1000, "msg".into())],
            &[(0x1000, "greeting".into())],
        );
        assert_eq!(parse_intents(&text), vec![(0, "greeting".to_owned())]);
        let split = render(&[], &lines, &[], &[(0x1003, "inside".into())]);
        assert_eq!(
            parse_intents(&split),
            vec![(3, "inside".to_owned())],
            "mid-line intents keep their offset"
        );
        assert_eq!(parse(&split).expect("parses"), b"NOPRAY #1\x00\x01");
        assert_eq!(
            parse(&text).expect("rendered listing parses"),
            b"NOPRAY #1\x00\x01"
        );
    }
}
