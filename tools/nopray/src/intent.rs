//! Authoring intents: `#= <instruction>` comments that state what the author meant to encode.
//!
//! An intent survives `nopray annotate` and is compared with the decoded instruction at the same
//! address. A mismatch is the typical symptom of a hand-encoding mistake (wrong `ModRM`, wrong
//! register field, wrong displacement), caught before the program ever runs.

/// Memory operand size keywords. Decoders omit them when a register operand already fixes the
/// size, so they are only compared when the decoded text itself shows one.
const SIZE_WORDS: [&str; 10] = [
    "byte", "word", "dword", "fword", "qword", "tbyte", "oword", "xmmword", "ymmword", "zmmword",
];

/// Canonical form used for comparison: lowercase, punctuation-insensitive spacing, numbers in
/// decimal, `ptr`/`short`/`near`/`#` dropped, symbols resolved to addresses.
/// Size keywords are dropped when `keep_sizes` is false.
fn normalize(text: &str, resolve: &dyn Fn(&str) -> Option<u64>, keep_sizes: bool) -> String {
    let lower = text.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.';
    while i < chars.len() {
        let c = chars[i];
        if word(c) {
            let start = i;
            while i < chars.len() && word(chars[i]) {
                i += 1;
            }
            let tok: String = chars[start..i].iter().collect();
            if matches!(tok.as_str(), "ptr" | "short" | "near")
                || (!keep_sizes && SIZE_WORDS.contains(&tok.as_str()))
            {
                continue;
            }
            // AArch64 decoders print MOVZ with a small immediate as its preferred alias MOV.
            let tok = if tok == "movz" { "mov".to_owned() } else { tok };
            match parse_num(&tok).or_else(|| resolve(&tok)) {
                Some(v) => out.push_str(&v.to_string()),
                None => out.push_str(&tok),
            }
            out.push(' ');
        } else {
            if !(c.is_whitespace() || c == ',' || c == '#') {
                out.push(c);
                if c == ']' {
                    out.push(' ');
                }
            }
            i += 1;
        }
    }
    collapse_aliases(out.trim_end())
}

/// Normalize well-known x86-64 / x87 decoder vs Intel-manual equivalent forms:
/// - `imul r, r, imm` <-> `imul r, imm`
/// - x86 string ops with implicit `[rdi]`/`[rsi]` (`stosd [rdi]` <-> `stosd`, `rep movsb [rdi],[rsi]` <-> `rep movsb`)
/// - x87 memory loads/stores with implicit `st` (`fld st, [mem]` <-> `fld [mem]`, `fstp [mem], st` <-> `fstp [mem]`)
fn collapse_aliases(norm: &str) -> String {
    let toks: Vec<&str> = norm.split(' ').collect();
    if toks.len() == 4 && toks[0] == "imul" && toks[1] == toks[2] {
        return [toks[0], toks[1], toks[3]].join(" ");
    }
    // Strip implicit [rdi]/[rsi] from x86 string instructions (with optional rep/repe/repne prefix)
    let (prefix_len, op) = match toks.as_slice() {
        ["rep" | "repe" | "repz" | "repne" | "repnz", op, ..] => (2, *op),
        [op, ..] => (1, *op),
        [] => return norm.to_owned(),
    };
    let is_str_op = matches!(
        op,
        "stosb"
            | "stosw"
            | "stosd"
            | "stosq"
            | "movsb"
            | "movsw"
            | "movsd"
            | "movsq"
            | "lodsb"
            | "lodsw"
            | "lodsd"
            | "lodsq"
            | "scasb"
            | "scasw"
            | "scasd"
            | "scasq"
            | "cmpsb"
            | "cmpsw"
            | "cmpsd"
            | "cmpsq"
    );
    if is_str_op && toks.len() > prefix_len {
        let rest = toks[prefix_len..].join(" ");
        if matches!(
            rest.as_str(),
            "[rdi ]" | "[rsi ]" | "[rdi ] [rsi ]" | "[rsi ] [rdi ]"
        ) {
            return toks[..prefix_len].join(" ");
        }
    }
    // Strip implicit `st` / `st0` operand on x87 memory load/store mnemonics (fld, fst, fstp, fild, fistp)
    if matches!(
        toks.first().copied(),
        Some("fld" | "fst" | "fstp" | "fild" | "fistp")
    ) && toks.len() >= 3
        && toks.iter().any(|t| t.starts_with('['))
    {
        let filtered: Vec<&str> = toks
            .iter()
            .copied()
            .enumerate()
            .filter(|(i, t)| *i == 0 || !matches!(*t, "st" | "st0"))
            .map(|(_, t)| t)
            .collect();
        return filtered.join(" ");
    }
    norm.to_owned()
}

fn parse_num(tok: &str) -> Option<u64> {
    if let Some(h) = tok.strip_prefix("0x") {
        return u64::from_str_radix(h, 16).ok();
    }
    if tok.bytes().all(|b| b.is_ascii_digit()) {
        return tok.parse().ok();
    }
    None
}

/// Whether `intent` describes the same instruction as the decoded `text`.
#[must_use]
pub fn matches(decoded: &str, intent: &str, resolve: &dyn Fn(&str) -> Option<u64>) -> bool {
    let lower = decoded.to_lowercase();
    let decoded_sized = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| SIZE_WORDS.contains(&w));
    normalize(decoded, resolve, decoded_sized) == normalize(intent, resolve, decoded_sized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(name: &str) -> Option<u64> {
        (name == "write_all").then_some(0x0040_00b0)
    }

    #[test]
    fn formatting_differences_do_not_matter() {
        assert!(matches(
            "mov dword ptr [r15+0x40],1",
            "mov dword [r15 + 64], 0x1",
            &sym
        ));
        assert!(matches("call 0x4000b0", "call write_all", &sym));
        assert!(matches("movz x8, #0x40", "movz x8, #64", &sym));
        assert!(matches("mov rcx,[rsp]", "mov rcx, qword ptr [rsp]", &sym));
        assert!(matches("imul rax,0xa", "imul rax, rax, 10", &sym));
        assert!(matches("stosd [rdi]", "stosd", &sym));
        assert!(matches("rep movsb [rdi],[rsi]", "rep movsb", &sym));
        assert!(matches("rep stosq [rdi]", "rep stosq", &sym));
        assert!(matches("fld st,qword ptr [r15+0x10]", "fld qword ptr [r15+0x10]", &sym));
        assert!(matches("fstp qword ptr [r15+0x18],st", "fstp qword ptr [r15+0x18]", &sym));
        assert!(!matches("fstp qword ptr [r15+0x20],st", "fstp qword ptr [r15+0x18]", &sym));
    }

    #[test]
    fn encoding_mistakes_are_detected() {
        assert!(
            !matches("mov eax,1", "mov edi,1", &sym),
            "wrong register field"
        );
        assert!(
            !matches("mov dword ptr [r15+0x48],1", "mov dword [r15+0x40],1", &sym),
            "wrong displacement"
        );
        assert!(
            !matches("mov ah,1", "mov ax,1", &sym),
            "register names are not numbers"
        );
        assert!(
            !matches("mov dword ptr [rdi],0xa", "mov byte ptr [rdi], 0xa", &sym),
            "operand size is compared when the decoder shows it"
        );
        assert!(
            !matches("imul rax,rcx,0xa", "imul rax, rax, 0xa", &sym),
            "imul source register is compared"
        );
    }
}
