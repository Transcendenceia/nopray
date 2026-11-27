//! Slot source files: hexadecimal bytes with `#` comments and optional `{kind @symbol}` fields.
//!
//! Only what precedes a `#` on each line is authoritative: hex digit pairs, and `{...}` tokens
//! that stand for one declared address/displacement field (see [`crate::fixup`]). Comments,
//! whitespace and line structure never change the bytes, so annotations can be regenerated
//! freely.

use std::fmt::Write as _;

use anyhow::{Result, bail};

use crate::fixup::{self, Fixup};

/// A parsed slot file: authored bytes (fields still at zero or at their authored word) and
/// the symbolic fields still to be resolved against the manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub bytes: Vec<u8>,
    pub fixups: Vec<Fixup>,
}

fn flush(digits: &mut String, lineno: usize, out: &mut Parsed) -> Result<()> {
    if digits.len() % 2 != 0 {
        bail!("line {lineno}: odd number of hex digits");
    }
    out.bytes
        .extend(hex::decode(&*digits).expect("validated hex digits"));
    digits.clear();
    Ok(())
}

/// Parses the authoritative part of one line, appending to `out`.
fn parse_line(code: &str, lineno: usize, out: &mut Parsed) -> Result<()> {
    let line_start = out.bytes.len() as u64;
    let first_fixup = out.fixups.len();
    let mut digits = String::new();
    let mut rest = code;
    while let Some(c) = rest.chars().next() {
        if c == '{' {
            flush(&mut digits, lineno, out)?;
            let Some(close) = rest.find('}') else {
                bail!("line {lineno}: `{{` without a closing `}}`");
            };
            let f = fixup::parse_token(&rest[1..close], out.bytes.len() as u64, lineno)?;
            match f.word {
                Some(w) => out.bytes.extend_from_slice(&w),
                None => out
                    .bytes
                    .extend(std::iter::repeat_n(0, f.kind.size() as usize)),
            }
            out.fixups.push(f);
            rest = &rest[close + 1..];
            continue;
        }
        if c.is_ascii_hexdigit() {
            digits.push(c);
        } else if !c.is_whitespace() {
            bail!("line {lineno}: `{c}` is not a hex digit (comments must start with `#`)");
        }
        rest = &rest[c.len_utf8()..];
    }
    flush(&mut digits, lineno, out)?;
    let line_end = out.bytes.len() as u64;
    debug_assert!(line_end >= line_start);
    for f in &mut out.fixups[first_fixup..] {
        f.line_end = line_end;
    }
    Ok(())
}

/// Parses a slot file. Accepts hex digit pairs separated by any whitespace, and `{...}` fields.
pub fn parse_full(text: &str) -> Result<Parsed> {
    let mut out = Parsed::default();
    for (lineno, line) in text.lines().enumerate() {
        let code = line.split('#').next().unwrap_or("");
        parse_line(code, lineno + 1, &mut out)?;
    }
    Ok(out)
}

/// Parses a slot file into bytes, with symbolic fields left at their unresolved placeholders.
#[cfg(test)]
pub fn parse(text: &str) -> Result<Vec<u8>> {
    parse_full(text).map(|p| p.bytes)
}

/// Authoring intents: `(byte offset, text)` for every byte line carrying `#= text`.
///
/// The offset is the slot offset of the first byte on that line. Assumes `text` already
/// passed [`parse_full`].
#[must_use]
pub fn parse_intents(text: &str) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    let mut acc = Parsed::default();
    for (lineno, line) in text.lines().enumerate() {
        let code = line.split('#').next().unwrap_or("");
        let offset = acc.bytes.len() as u64;
        if parse_line(code, lineno + 1, &mut acc).is_err() {
            continue;
        }
        if acc.bytes.len() as u64 > offset
            && let Some(pos) = line.find("#=")
        {
            let intent = line[pos + 2..].trim();
            if !intent.is_empty() {
                out.push((offset, intent.to_owned()));
            }
        }
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

/// A symbolic field re-emitted verbatim by [`render`]: `(absolute address, size, token)`.
pub type FieldToken<'a> = (u64, u64, &'a str);

/// Renders an annotated slot file. `labels` are `(address, name)` pairs printed as headings.
/// `intents` are `(absolute address, text)` pairs re-attached to the line that holds them.
/// `fields` are symbolic fields printed as their `{...}` token instead of the resolved bytes.
#[must_use]
pub fn render(
    header: &[String],
    lines: &[ListingLine],
    labels: &[(u64, String)],
    intents: &[(u64, String)],
    fields: &[FieldToken<'_>],
) -> String {
    let mut s = String::new();
    // Bytes already emitted as part of a `{...}` token (a field may cross a listing line).
    let mut skip_until = 0u64;
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
            let mut items: Vec<String> = Vec::new();
            for a in from..to {
                if a < skip_until {
                    continue;
                }
                if let Some((_, size, token)) = fields.iter().find(|(fa, _, _)| *fa == a) {
                    items.push((*token).to_owned());
                    skip_until = a + size;
                    continue;
                }
                items.push(format!("{:02x}", line.bytes[(a - line.addr) as usize]));
            }
            let hex = items.join(" ");
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
            } else if hex.is_empty() {
                let _ = writeln!(
                    s,
                    "# {from:#010x}  {text}  (bytes in the field token above)"
                );
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
    fn field_tokens_reserve_bytes_and_keep_intent_offsets() {
        let text = "e8 {rel32 @write_all}  #= call write_all\n{a64.imm26 @loop : 00 00 00 14} #= b loop\nc3 #= ret\n";
        let p = parse_full(text).expect("valid");
        assert_eq!(p.bytes, vec![0xe8, 0, 0, 0, 0, 0, 0, 0, 0x14, 0xc3]);
        assert_eq!(p.fixups.len(), 2);
        assert_eq!((p.fixups[0].offset, p.fixups[0].line_end), (1, 5));
        assert_eq!((p.fixups[1].offset, p.fixups[1].line_end), (5, 9));
        assert_eq!(
            parse_intents(text),
            vec![
                (0, "call write_all".to_owned()),
                (5, "b loop".to_owned()),
                (9, "ret".to_owned())
            ],
            "letters inside `{{}}` are not hex digits"
        );
        assert!(parse("e8 {rel32 @x\n").is_err(), "unterminated token");
    }

    #[test]
    fn render_keeps_field_tokens_across_line_breaks() {
        let mut bytes = b"0123456789abcd".to_vec();
        bytes.extend([0x34, 0x12, 0x40, 0, 0, 0, 0, 0]);
        let lines = data_lines(0x1000, &bytes);
        let token = "{abs64 @msg}";
        let text = render(&[], &lines, &[], &[], &[(0x100e, 8, token)]);
        let p = parse_full(&text).expect("rendered listing parses");
        assert_eq!(p.bytes.len(), bytes.len());
        assert_eq!(p.fixups.len(), 1);
        assert_eq!(p.fixups[0].offset, 14);
        assert_eq!(p.fixups[0].token, token);
    }

    #[test]
    fn render_round_trips() {
        let lines = data_lines(0x1000, b"NOPRAY #1\x00\x01");
        let text = render(
            &["slot".into()],
            &lines,
            &[(0x1000, "msg".into())],
            &[(0x1000, "greeting".into())],
            &[],
        );
        assert_eq!(parse_intents(&text), vec![(0, "greeting".to_owned())]);
        let split = render(&[], &lines, &[], &[(0x1003, "inside".into())], &[]);
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
