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
            if matches!(tok.as_str(), "ptr" | "short" | "near" | "far")
                || (!keep_sizes && SIZE_WORDS.contains(&tok.as_str()))
            {
                continue;
            }
            // Canonicalize standard ISA mnemonic aliases:
            // - AArch64: MOVZ with a small immediate is printed as MOV.
            // - x86-64: jz/jnz/jc/jnc are exact opcode synonyms of je/jne/jb/jae.
            let tok = match tok.as_str() {
                "movz" => "mov".to_owned(),
                "jz" => "je".to_owned(),
                "jnz" => "jne".to_owned(),
                "jc" => "jb".to_owned(),
                "jnc" => "jae".to_owned(),
                _ => tok,
            };
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
            | "outsb"
            | "outsw"
            | "outsd"
            | "insb"
            | "insw"
            | "insd"
    );
    if is_str_op && toks.len() > prefix_len {
        let rest = toks[prefix_len..].join(" ");
        if matches!(
            rest.as_str(),
            "[rdi ]"
                | "[rsi ]"
                | "[rdi ] [rsi ]"
                | "[rsi ] [rdi ]"
                | "[edi ]"
                | "[esi ]"
                | "[edi ] [esi ]"
                | "[esi ] [edi ]"
                | "dx [rsi ]"
                | "dx [esi ]"
                | "[rdi ] dx"
                | "[edi ] dx"
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

/// Explains a mismatch token by token: which operands differ and, for numbers, by how much.
///
/// Both sides go through the same normalisation as [`matches`], so the tokens shown are the
/// ones actually compared (registers lowercase, numbers in decimal, symbols resolved).
#[must_use]
pub fn explain(decoded: &str, intent: &str, resolve: &dyn Fn(&str) -> Option<u64>) -> String {
    let lower = decoded.to_lowercase();
    let sized = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| SIZE_WORDS.contains(&w));
    let a = normalize(decoded, resolve, sized);
    let b = normalize(intent, resolve, sized);
    let ta: Vec<&str> = a.split(' ').collect();
    let tb: Vec<&str> = b.split(' ').collect();
    if ta.len() != tb.len() || ta.first() != tb.first() {
        return format!("compared as `{a}` vs `{b}`");
    }
    let bare = |t: &str| {
        t.trim_matches(|c: char| !c.is_ascii_alphanumeric())
            .to_owned()
    };
    let diffs: Vec<String> = ta
        .iter()
        .zip(&tb)
        .enumerate()
        .filter(|(_, (x, y))| x != y)
        .take(3)
        .map(|(i, (x, y))| {
            let (bx, by) = (bare(x), bare(y));
            match (bx.parse::<i128>(), by.parse::<i128>()) {
                (Ok(vx), Ok(vy)) => {
                    let d = vy - vx;
                    let sign = if d < 0 { "-" } else { "+" };
                    format!(
                        "operand {i}: bytes give {vx:#x}, intent {vy:#x} (intent {sign}{:#x})",
                        d.unsigned_abs()
                    )
                }
                _ => format!("operand {i}: bytes give `{bx}`, intent `{by}`"),
            }
        })
        .collect();
    if diffs.is_empty() {
        format!("compared as `{a}` vs `{b}`")
    } else {
        diffs.join("; ")
    }
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
        assert!(matches(
            "fld st,qword ptr [r15+0x10]",
            "fld qword ptr [r15+0x10]",
            &sym
        ));
        assert!(matches(
            "fstp qword ptr [r15+0x18],st",
            "fstp qword ptr [r15+0x18]",
            &sym
        ));
        assert!(!matches(
            "fstp qword ptr [r15+0x20],st",
            "fstp qword ptr [r15+0x18]",
            &sym
        ));
        // Bare-metal x86 32->64, jump aliases, string I/O, and AVX2/FMA/F16C intents:
        assert!(matches("je 0x101096", "jz 0x101096", &sym));
        assert!(matches("jne 0x101017", "jnz 0x101017", &sym));
        assert!(matches("jb 0x1014b1", "jc 0x1014b1", &sym));
        assert!(matches("jae 0x101500", "jnc 0x101500", &sym));
        assert!(matches("jmp 0x8:0x100200", "jmp far 0x8:0x100200", &sym));
        assert!(matches("rep stosd [edi]", "rep stosd", &sym));
        assert!(matches("rep outsb dx,[rsi]", "rep outsb", &sym));
        assert!(matches(
            "vfmadd231ps ymm0,ymm15,[rsi+0x20]",
            "vfmadd231ps ymm0, ymm15, [rsi + 32]",
            &sym
        ));
    }

    #[test]
    fn explanations_name_the_differing_operand() {
        let e = explain("cmp w1, #0x5", "cmp w9, #5", &sym);
        assert!(e.contains("`w1`") && e.contains("`w9`"), "{e}");
        let e = explain("jmp 0x400140", "jmp 0x400144", &sym);
        assert!(e.contains("+0x4"), "{e}");
        let e = explain("mov eax,1", "add eax,1", &sym);
        assert!(e.starts_with("compared as"), "{e}");
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
