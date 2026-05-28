//! OP-feed engine doctrine gate (V-80 rung 3).
//!
//! `nmp-feed` is substrate-generic: the `RootIndexedFeed` engine must name
//! ZERO protocol conventions (D0). A leaked `nip01`, `marmot`, or
//! `ProfileDisplay` token would mean a protocol instance bled into the engine.
//! This grep gate fails the build if any `.rs` under `crates/nmp-feed/src/`
//! contains a banned token, case-insensitively.
//!
//! Why a bespoke gate (not the `doctrine-lint` binary): the binary's banned-
//! token list is keyed per-crate by its own config; this gate is the explicit
//! CI guard the OP-feed design (`docs/perf/op-centric-feed-architecture.md`
//! §3-J, §4) requires for the engine specifically, and it lives with the
//! feature so a reviewer sees the contract next to the code.
//!
//! # Running
//!
//! ```bash
//! cargo test -p nmp-testing --test op_feed_doctrine_lint
//! ```

use std::fs;
use std::path::{Path, PathBuf};

/// Tokens that must never appear in `nmp-feed`'s engine source. Matched
/// case-insensitively as substrings. `nip` followed by a digit catches every
/// `nipNN` form; `marmot` and `profiledisplay` are the other protocol/profile
/// nouns the engine must stay free of.
const BANNED_SUBSTRINGS: &[&str] = &["marmot", "profiledisplay"];

fn nmp_feed_src() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/nmp-testing; the sibling is crates/nmp-feed.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .join("nmp-feed")
        .join("src")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// `true` if `line` contains `nip` immediately followed by an ASCII digit
/// (case-insensitive) — e.g. `nip01`, `Nip10`, `NIP-22` does NOT match (hyphen
/// breaks it) but `nip22` does. Hyphenated forms in prose comments are allowed;
/// only token-shaped `nipNN` is banned, which is what a leaked type/identifier
/// would look like.
fn has_nip_token(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    let needle = b"nip";
    bytes
        .windows(needle.len() + 1)
        .any(|w| &w[..needle.len()] == needle && w[needle.len()].is_ascii_digit())
}

#[test]
fn nmp_feed_engine_names_no_protocol_token() {
    let src = nmp_feed_src();
    assert!(src.is_dir(), "expected nmp-feed src at {src:?}");

    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(
        !files.is_empty(),
        "found no .rs files under {src:?} — gate would be vacuous"
    );

    let mut violations = Vec::new();
    for file in &files {
        let contents = fs::read_to_string(file).unwrap_or_else(|e| panic!("read {file:?}: {e}"));
        for (idx, line) in contents.lines().enumerate() {
            let lower = line.to_ascii_lowercase();
            if has_nip_token(&lower) {
                violations.push(format!("{}:{}: nipNN token — {}", file.display(), idx + 1, line.trim()));
            }
            for banned in BANNED_SUBSTRINGS {
                if lower.contains(banned) {
                    violations.push(format!(
                        "{}:{}: banned `{}` — {}",
                        file.display(),
                        idx + 1,
                        banned,
                        line.trim()
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "nmp-feed must name zero protocol/profile tokens (D0). Violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn nip_token_matcher_is_correct() {
    // Sanity: the matcher catches token-shaped nipNN but not the bare word.
    assert!(has_nip_token("use nmp_nip01::foo"));
    assert!(has_nip_token("nip10reply"));
    assert!(!has_nip_token("nip"));
    assert!(!has_nip_token("a snippet of code"));
    assert!(!has_nip_token("nip-22 in a comment")); // hyphenated prose allowed
}
