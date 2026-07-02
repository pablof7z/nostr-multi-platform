//! Doc-citation content-truth gate (#2768).
//!
//! ADR-0073's ref-check only verified that a cited ADR/section *number*
//! resolves to a file. It never checked that the citing prose's section
//! reference actually exists as a heading, which let 11+ doc-comment
//! citations of `crate-boundaries.md §N.M` go dangling after the spec was
//! reduced to flat `## 1`..`## 11` (+ `## 10a`) top-level headings with no
//! numbered `### N.M` subsections. This module closes that class with two
//! gates:
//!
//! 1. [`crate_boundaries_section_citations_resolve_to_real_headings`] —
//!    every `crate-boundaries.md` doc-comment mention followed by a `§<token>`
//!    citation must cite a token that resolves to an actual heading, parsed
//!    fresh from `docs/architecture/crate-boundaries.md` on every run (no
//!    hardcoded section list to drift out of sync with the spec). A `§N.M`
//!    fails unless a `### N.M` heading exists — today none do, so any
//!    `§N.M` citation is a finding. A bare `§N` (or `§10a`) resolves against
//!    the flat top-level headings.
//! 2. [`adr_citations_resolve_to_existing_decision_files`] — every
//!    `ADR-NNNN` token in a Rust source/doc comment must resolve to an
//!    existing `docs/decisions/NNNN-*.md` file. Number-resolves-to-file is
//!    the floor here; this deliberately does not validate ADR *subsection*
//!    citations (e.g. `ADR-0070 §6.1`) — only that the ADR number itself
//!    exists.
//!
//! ## Citation window (deliberately narrow)
//!
//! A citation is recognized when a `§<token>` appears on the same line as
//! the `crate-boundaries.md` mention, OR — only when that mention line has
//! **no** `§` at all — on the immediately following line (doc-comment prose
//! commonly wraps a citation across a `//!`/`///` line break, e.g.
//! `"...see \`crate-boundaries.md\`\n§3 for the trait seam..."`). This is
//! intentionally narrower than "any `§` within N lines": a wider window
//! would misattribute unrelated bare `§` mentions (e.g. NIP spec section
//! numbers, or a separate `spec §N` shorthand a few lines away) to
//! `crate-boundaries.md`. The tradeoff is a known false-negative for a
//! citation that continues past an *already-cited* line (a mention line
//! that cites `§2` and wraps to a second `§N.M` on the next line) — accepted
//! as a floor, not a ceiling, for this gate.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::workspace_root;

// ─── Shared token parsing ─────────────────────────────────────────────────

/// Parse a section token (`"3"`, `"10a"`, or — future-proofing, since none
/// exist today — `"3.2"`) from the start of `s`. Returns `None` if `s` does
/// not start with a digit.
fn parse_number_token(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    let mut end = i;
    if end < bytes.len() && bytes[end].is_ascii_lowercase() {
        end += 1;
    }
    if end < bytes.len()
        && bytes[end] == b'.'
        && end + 1 < bytes.len()
        && bytes[end + 1].is_ascii_digit()
    {
        let mut j = end + 1;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        end = j;
    }
    Some(s[..end].to_string())
}

/// Every `§<token>` occurrence in `text`, in order.
fn extract_section_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut search_from = 0usize;
    while let Some(pos) = text[search_from..].find('§') {
        let abs = search_from + pos;
        let after = &text[abs + '§'.len_utf8()..];
        if let Some(token) = parse_number_token(after) {
            out.push(token);
        }
        search_from = abs + '§'.len_utf8();
    }
    out
}

// ─── crate-boundaries.md heading parsing ──────────────────────────────────

/// Collect every valid section token from `crate-boundaries.md`'s ATX
/// headings (`^#+ <token>...`), e.g. `"## 1. Source Of Authority"` -> `"1"`,
/// `"## 10a. Browser Platform Adapter (...)"` -> `"10a"`.
fn collect_valid_section_tokens(spec_text: &str) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    for line in spec_text.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#') {
            continue;
        }
        let after_hashes = trimmed.trim_start_matches('#');
        if !after_hashes.starts_with(' ') {
            continue;
        }
        let heading_text = after_hashes.trim_start();
        if let Some(token) = parse_number_token(heading_text) {
            tokens.insert(token);
        }
    }
    tokens
}

// ─── Rust-source scanning ──────────────────────────────────────────────────

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(name, "target" | ".git" | "vendor") {
                continue;
            }
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn workspace_rs_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files);
    collect_rs_files(&root.join("apps"), &mut files);
    files
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// `(1-indexed line, cited token)` pairs for every `crate-boundaries.md
/// §<token>` citation found in `text`, per the module-doc citation-window
/// rule.
fn find_section_citations(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if !line.contains("crate-boundaries.md") {
            continue;
        }
        let mut window = (*line).to_string();
        if !line.contains('§') {
            if let Some(next) = lines.get(idx + 1) {
                window.push('\n');
                window.push_str(next);
            }
        }
        for token in extract_section_tokens(&window) {
            out.push((idx + 1, token));
        }
    }
    out
}

/// `(1-indexed line, ADR number string, e.g. "0070")` pairs for every
/// `ADR-NNNN` token found in `text`.
fn find_adr_citations(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let mut offset = 0usize;
        while let Some(rel) = line[offset..].find("ADR-") {
            let start = offset + rel + 4;
            let digits: String = line[start..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if digits.len() == 4 {
                out.push((idx + 1, digits.clone()));
            }
            // Advance to a guaranteed-valid char boundary past what we just
            // inspected, so the scan always makes forward progress whether
            // or not digits followed "ADR-".
            let consumed = if digits.is_empty() {
                line[start..]
                    .chars()
                    .next()
                    .map(char::len_utf8)
                    .unwrap_or(1)
            } else {
                digits.len()
            };
            offset = start + consumed;
            if offset >= line.len() {
                break;
            }
        }
    }
    out
}

// ─── Gate 1: crate-boundaries.md §N[.M] citations resolve to real headings ─

#[test]
fn crate_boundaries_section_citations_resolve_to_real_headings() {
    let root = workspace_root();
    let spec_path = root.join("docs/architecture/crate-boundaries.md");
    let spec_text = std::fs::read_to_string(&spec_path)
        .unwrap_or_else(|e| panic!("must read {}: {e}", spec_path.display()));
    let valid = collect_valid_section_tokens(&spec_text);
    assert!(
        !valid.is_empty(),
        "sanity check failed: crate-boundaries.md must have at least one numbered \
         top-level heading (## N) for this gate to mean anything"
    );

    let mut violations = Vec::new();
    for path in workspace_rs_files(&root) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (line, token) in find_section_citations(&text) {
            if !valid.contains(&token) {
                violations.push(format!(
                    "{}:{line}: cites `crate-boundaries.md` §{token} — no such heading exists \
                     (valid section tokens: {})",
                    display_path(&root, &path),
                    valid.iter().cloned().collect::<Vec<_>>().join(", "),
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "doc-comment citations of `crate-boundaries.md §N[.M]` must resolve to an actual \
         heading in the spec — content-truth, not just number-resolves-to-file. \
         `docs/architecture/crate-boundaries.md` has ONLY flat `## 1`..`## 11` (+ `## 10a`) \
         headings; no numbered `### N.M` subsections exist, so any `§N.M` citation is \
         dangling until a matching subsection heading is added. Fix the citing doc comment \
         to name the top-level section that actually states the rule, or quote the rule \
         inline and drop the false section number. Violations:\n{}",
        violations.join("\n")
    );
}

// ─── Gate 2: ADR-NNNN citations resolve to an existing decision file ──────

#[test]
fn adr_citations_resolve_to_existing_decision_files() {
    let root = workspace_root();
    let decisions_dir = root.join("docs/decisions");
    let mut existing_numbers = BTreeSet::new();
    for entry in std::fs::read_dir(&decisions_dir)
        .unwrap_or_else(|e| panic!("must read {}: {e}", decisions_dir.display()))
        .flatten()
    {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.len() >= 5 && name.as_bytes()[..4].iter().all(u8::is_ascii_digit) {
            existing_numbers.insert(name[..4].to_string());
        }
    }
    assert!(
        !existing_numbers.is_empty(),
        "sanity check failed: docs/decisions must contain at least one numbered ADR file"
    );

    let mut violations = Vec::new();
    for path in workspace_rs_files(&root) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (line, number) in find_adr_citations(&text) {
            if !existing_numbers.contains(&number) {
                violations.push(format!(
                    "{}:{line}: cites ADR-{number} — no docs/decisions/{number}-*.md file exists",
                    display_path(&root, &path),
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "doc-comment `ADR-NNNN` citations must resolve to an existing \
         docs/decisions/NNNN-*.md file. Number-resolves-to-file is the floor here; ADR \
         subsection citations (e.g. `ADR-0070 §6.1`) are NOT validated by this gate. \
         Violations:\n{}",
        violations.join("\n")
    );
}

// ─── Self-tests for the parsing helpers ────────────────────────────────────

#[test]
fn parse_number_token_handles_bare_letter_and_subsection_forms() {
    assert_eq!(parse_number_token("3)"), Some("3".to_string()));
    assert_eq!(parse_number_token("10a)"), Some("10a".to_string()));
    assert_eq!(parse_number_token("3.2)"), Some("3.2".to_string()));
    assert_eq!(parse_number_token("display-separation)"), None);
}

#[test]
fn find_section_citations_follows_the_next_line_only_when_mention_line_has_no_section() {
    let same_line = "//! see `docs/architecture/crate-boundaries.md` §5 for the contract";
    assert_eq!(
        find_section_citations(same_line),
        vec![(1, "5".to_string())]
    );

    let wrapped = "//! see `docs/architecture/crate-boundaries.md`\n//! §3 for the seam";
    assert_eq!(find_section_citations(wrapped), vec![(1, "3".to_string())]);

    // A mention line that already cites a section does NOT pull in a
    // trailing section on the next line (documented tradeoff above).
    //
    // Built via `concat!` rather than one literal so this fixture's
    // deliberately-dangling `§3.7` does not itself trip the gate defined in
    // this very file when it scans its own source (mirrors the
    // `["Nmp", "Wasm", "Runtime"].concat()` self-reference dodge in
    // `wasm_abi_gates.rs`).
    let spec_name = ["crate-boundaries", ".md"].concat();
    let already_cited_then_more = format!("//! see {spec_name} §2 and\n//! §3.7 too");
    assert_eq!(
        find_section_citations(&already_cited_then_more),
        vec![(1, "2".to_string())]
    );
}

#[test]
fn find_adr_citations_extracts_every_four_digit_number() {
    let text = "//! See ADR-0071 and ADR-0076 for the split.";
    assert_eq!(
        find_adr_citations(text),
        vec![(1, "0071".to_string()), (1, "0076".to_string())]
    );
}
