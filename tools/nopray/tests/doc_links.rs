//! The skill is a book: its chapters cross-reference each other, the references and the
//! examples. A broken relative link or anchor sends the agent nowhere, so every relative
//! link in the skill's Markdown must resolve to a file and, when it names one, a heading.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Markdown files that make up the skill: the entry point, the book, the references and the
/// READMEs of every example.
fn skill_docs(root: &Path) -> Vec<PathBuf> {
    let mut docs = vec![
        root.join("SKILL.md"),
        root.join("README.md"),
        root.join("README.es.md"),
    ];
    for dir in ["book", "references"] {
        docs.extend(markdown_in(&root.join(dir), false));
    }
    docs.extend(markdown_in(&root.join("examples"), true));
    docs.sort();
    docs
}

fn markdown_in(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let generated = path.file_name().is_some_and(|n| n == "build");
            if recursive && !generated {
                out.extend(markdown_in(&path, true));
            }
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
    out
}

/// Text outside fenced code blocks and inline code spans.
fn prose(text: &str) -> String {
    let mut out = String::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut in_code = false;
        for c in line.chars() {
            if c == '`' {
                in_code = !in_code;
            } else if !in_code {
                out.push(c);
            }
        }
        out.push('\n');
    }
    out
}

/// Targets of inline links `[text](target)` in prose.
fn link_targets(text: &str) -> Vec<String> {
    let prose = prose(text);
    let mut targets = Vec::new();
    let mut rest = prose.as_str();
    while let Some(at) = rest.find("](") {
        rest = &rest[at + 2..];
        if let Some(end) = rest.find(')') {
            let target = rest[..end].split_whitespace().next().unwrap_or("");
            targets.push(target.to_string());
            rest = &rest[end..];
        }
    }
    targets
}

/// GitHub-style heading anchor: lower case, punctuation dropped, spaces to hyphens.
fn slug(heading: &str) -> String {
    heading
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            '-' | '_' => Some(c),
            c if c.is_alphanumeric() => Some(c),
            _ => None,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

fn anchors(text: &str) -> HashSet<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || !line.starts_with('#') {
            continue;
        }
        let heading = line.trim_start_matches('#').trim().replace('`', "");
        let base = slug(&heading);
        let mut anchor = base.clone();
        let mut n = 1;
        while seen.contains(&anchor) {
            anchor = format!("{base}-{n}");
            n += 1;
        }
        seen.insert(anchor);
    }
    seen
}

#[test]
fn slug_matches_github_for_the_headings_we_link() {
    assert_eq!(slug("Live node · 2026-11-27"), "live-node--2026-11-27");
    assert_eq!(
        slug("5. Linux deployment checklist"),
        "5-linux-deployment-checklist"
    );
}

#[test]
fn every_relative_link_in_the_skill_resolves() {
    let root = repo_root();
    let mut broken = Vec::new();
    for doc in skill_docs(&root) {
        let text = fs::read_to_string(&doc).expect("read markdown");
        for target in link_targets(&text) {
            let external = target.contains("://") || target.starts_with("mailto:");
            if target.is_empty() || external {
                continue;
            }
            let (path, anchor) = match target.split_once('#') {
                Some((p, a)) => (p, Some(a)),
                None => (target.as_str(), None),
            };
            let file = if path.is_empty() {
                doc.clone()
            } else {
                doc.parent().expect("parent").join(path)
            };
            let shown = doc
                .strip_prefix(&root)
                .unwrap_or(&doc)
                .display()
                .to_string();
            if !file.exists() {
                broken.push(format!("{shown}: {target} (no such file)"));
                continue;
            }
            let is_markdown = file.extension().is_some_and(|e| e == "md");
            if let (Some(anchor), true) = (anchor, is_markdown) {
                let target_text = fs::read_to_string(&file).expect("read link target");
                if !anchors(&target_text).contains(anchor) {
                    broken.push(format!("{shown}: {target} (no such heading)"));
                }
            }
        }
    }
    assert!(broken.is_empty(), "broken links:\n{}", broken.join("\n"));
}
