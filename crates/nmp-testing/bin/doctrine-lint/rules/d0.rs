//! D0 — kernel never grows app nouns.
//!
//! `nmp-core` is the substrate. Adding domain-specific nouns (NIP-29 group
//! ids, NIP-94 file metadata, etc.) inside it breaks the kernel boundary —
//! such concepts belong in carved-out crates like `nmp-nip29`.
//!
//! d264d9d (T55 cleanup) removed the last NIP-29 nouns from `nmp-core`. This
//! lint keeps them out.
//!
//! ## Banned tokens (in non-comment source under `crates/nmp-core/src/`)
//!
//! - `nip29`, `NIP29`, `nip_29`, `Nip29`
//! - `group_id`, `groupid`, `GroupId`, `group_ids`, `groupIds`
//! - `pin_to` (lower-snake-case; the public type uses `relay_pin`)
//!
//! ## Deliberately omitted
//!
//! - bare `group` (would false-positive on "group by", `GroupedBy`, doc
//!   prose). The `_id`-suffixed variants are the discriminating noun.
//!
//! ## Exemption
//!
//! Doc-comment lines (`///`, `//!`, `//`, inside `/* */`) are skipped; the
//! brief explicitly allows `// Example use case:` and module-level doc
//! prose referencing NIP-29. The exempt file
//! `planner/compiler/partition/case_e_relay_pinned.rs` is also skipped via
//! its path.

use std::path::Path;

pub const ID: &str = "D0";

const EXEMPT_FILE_SUFFIXES: &[&str] = &[
    // The "third routing lane" partition keeps its NIP-29 doc reference
    // intentionally per the brief; the body uses generic `pin_url` / `RelayUrl`.
    "planner/compiler/partition/case_e_relay_pinned.rs",
];

const BANNED_TOKENS: &[(&str, &str)] = &[
    ("nip29", "use carved-out crate `nmp-nip29` instead of inlining the noun"),
    ("NIP29", "use carved-out crate `nmp-nip29` instead of inlining the noun"),
    ("nip_29", "use carved-out crate `nmp-nip29` instead of inlining the noun"),
    ("Nip29", "use carved-out crate `nmp-nip29` instead of inlining the noun"),
    ("group_id", "replace with generic substrate (e.g. `relay_pin`, `h_tag`); domain noun belongs in `nmp-nip29`"),
    ("group_ids", "replace with generic substrate (e.g. `relay_pin`, `h_tag`); domain noun belongs in `nmp-nip29`"),
    ("groupid", "replace with generic substrate (e.g. `relay_pin`, `h_tag`); domain noun belongs in `nmp-nip29`"),
    ("groupIds", "replace with generic substrate (e.g. `relay_pin`, `h_tag`); domain noun belongs in `nmp-nip29`"),
    ("GroupId", "replace with generic substrate (e.g. `RelayUrl`, `String`); domain noun belongs in `nmp-nip29`"),
    ("pin_to", "the public field is `relay_pin`; `pin_to` is a stale NIP-29-flavoured name"),
];

pub fn file_is_exempt(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    // D0's mandate is the kernel substrate crate (`nmp-core`). App-layer
    // crates under `apps/` legitimately use domain nouns — e.g. `nmp-app-chirp`
    // imports `nmp_nip29` types — so the rule does not apply to them.
    // Match both `/apps/` (absolute/relative with leading component) and
    // the case where the path starts directly with `apps/`.
    if s.contains("/apps/") || s.starts_with("apps/") {
        return true;
    }
    EXEMPT_FILE_SUFFIXES.iter().any(|suf| s.ends_with(suf))
}

/// Returns `(col, message, suggested)` per match on `line`. `is_comment`
/// short-circuits the scan — the brief exempts doc-comment prose.
pub fn check(line: &str, is_comment: bool) -> Vec<(usize, String, String)> {
    if is_comment {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (token, suggested) in BANNED_TOKENS {
        let mut start = 0;
        while let Some(rel) = line[start..].find(token) {
            let col = start + rel;
            hits.push((
                col + 1, // 1-indexed columns for clippy compatibility
                format!("banned token `{}` — D0 forbids app nouns in `nmp-core`", token),
                (*suggested).to_string(),
            ));
            start = col + token.len();
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_group_id() {
        let hits = check("    let group_id: String = String::new();", false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("group_id"));
    }

    #[test]
    fn flags_pascal_groupid() {
        let hits = check("pub struct GroupId(String);", false);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn ignores_comment_line() {
        let hits = check("/// Example use case: NIP-29 relay-based groups", true);
        assert!(hits.is_empty());
    }

    #[test]
    fn does_not_flag_bare_group() {
        // `group by` is a verb; `GroupedBy` is a generic name. We avoid the
        // bare-noun trap.
        let hits = check("    let g = items.group_by(|x| x.kind);", false);
        assert!(hits.is_empty());
    }

    #[test]
    fn flags_pin_to() {
        let hits = check("    pub pin_to: Option<RelayUrl>,", false);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn flags_nip29_token() {
        let hits = check("use nip29_things::Whatever;", false);
        assert_eq!(hits.len(), 1);
    }
}
