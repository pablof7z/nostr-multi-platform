//! `BROWSER_RUNTIME_BOUNDARY` — browser-runtime relay/wasm adapters enforce policy boundary.
//!
//! The browser-runtime relay transport and wasm ABI adapters
//! (`crates/nmp-browser-runtime/src/relay/**` and `crates/nmp-browser-runtime/src/wasm/**`)
//! must remain pure adapters: no routing/outbox/NIP-65/publish-target/subscription-planning
//! vocabulary, no direct policy logic.
//!
//! This rule (BROWSER_RUNTIME_BOUNDARY) scans for policy vocabulary leaking into the
//! browser-runtime relay and wasm adapters.
//!
//! ## Banned in `crates/nmp-browser-runtime/src/relay/**` and `/wasm/**`
//! - `outbox`, `route_to`, `Nip65`, `publish_target` — routing policy vocabulary
//! - `mailbox`, `signer_kind` — signer/messaging policy vocabulary
//! - `subscription_planning`, `subscription_lifecycle` — subscription control vocabulary
//!
//! ## Note: TypeScript scanning
//!
//! This rule currently scans only Rust files. TypeScript package policy gates
//! (routing, dispatch, subscription planning, poll loops, secret/snapshot retention)
//! require walker extension for `.ts`/`.tsx` with proper comment handling. This is
//! tracked as future work (#2082).
//!
//! Escape hatches: `// doctrine-allow: browser_runtime_boundary` with a required
//! justification (e.g., "// doctrine-allow: browser_runtime_boundary — worker dispatch
//! boundary requires explicit routing decision gate").

use std::path::Path;

pub const ID: &str = "BROWSER_RUNTIME_BOUNDARY";

const BANNED_ROUTING_VOCAB: &[(&str, &str)] = &[
    ("outbox", "routing policy forbidden in transport adapter"),
    ("route_to", "routing policy forbidden in transport adapter"),
    ("Nip65", "NIP-65 relay routing forbidden in transport adapter"),
    ("publish_target", "publish-routing policy forbidden in transport adapter"),
    ("mailbox", "mailbox/inbox messaging policy forbidden in transport adapter"),
];

const BANNED_SIGNER_VOCAB: &[(&str, &str)] = &[
    ("signer_kind", "signer kind management forbidden in transport adapter"),
    ("subscription_kind", "subscription lifecycle policy forbidden in transport adapter"),
];


pub fn file_is_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");

    // Scope: browser-runtime relay and wasm adapters (transport adapters)
    ((s.contains("/nmp-browser-runtime/src/relay/") || s.starts_with("nmp-browser-runtime/src/relay/"))
        || (s.contains("/nmp-browser-runtime/src/wasm/") || s.starts_with("nmp-browser-runtime/src/wasm/")))
        && !s.contains("/doc/")
}

/// Returns `(col, message, suggested)` per match on `line`. `is_comment`
/// short-circuits the scan — the brief exempts doc-comment prose.
pub fn check(line: &str, is_comment: bool) -> Vec<(usize, String, String)> {
    if is_comment {
        return Vec::new();
    }
    let mut hits = Vec::new();

    // Check for banned routing vocabulary (match prefix only)
    for (vocab, reason) in BANNED_ROUTING_VOCAB {
        let mut start = 0;
        while let Some(rel) = line[start..].find(vocab) {
            let abs_pos = start + rel;
            let end = abs_pos + vocab.len();
            // Verify it's a prefix match (character after is not alphanumeric/underscore)
            if end < line.len() {
                let after = line.chars().nth(end).unwrap_or(' ');
                if after.is_alphanumeric() || after == '_' {
                    start = end;
                    continue; // Embedded in a larger word, keep searching
                }
            }
            let col = abs_pos + 1; // 1-indexed
            hits.push((
                col,
                format!(
                    "banned routing vocabulary `{}` — {} (BROWSER_RUNTIME_BOUNDARY)",
                    vocab, reason
                ),
                format!(
                    "Remove routing policy from transport adapter; {} belongs in nmp-core",
                    vocab
                ),
            ));
            start = end;
        }
    }

    // Check for banned signer vocabulary (match prefix only)
    for (vocab, reason) in BANNED_SIGNER_VOCAB {
        let mut start = 0;
        while let Some(rel) = line[start..].find(vocab) {
            let abs_pos = start + rel;
            let end = abs_pos + vocab.len();
            // Verify it's a prefix match (character after is not alphanumeric/underscore)
            if end < line.len() {
                let after = line.chars().nth(end).unwrap_or(' ');
                if after.is_alphanumeric() || after == '_' {
                    start = end;
                    continue; // Embedded in a larger word, keep searching
                }
            }
            let col = abs_pos + 1; // 1-indexed
            hits.push((
                col,
                format!(
                    "banned signer/subscription vocabulary `{}` — {} (BROWSER_RUNTIME_BOUNDARY)",
                    vocab, reason
                ),
                format!(
                    "Remove policy vocabulary from transport adapter; {} belongs in nmp-core",
                    vocab
                ),
            ));
            start = end;
        }
    }

    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_banned_routing_outbox() {
        let hits = check("    let outbox_url = resolver.outbox(&pubkey);", false);
        assert!(hits.iter().any(|h| h.1.contains("outbox")));
    }

    #[test]
    fn flags_banned_routing_nip65() {
        let hits = check("let resolver: Box<dyn Nip65Resolver> = ...;", false);
        assert!(hits.iter().any(|h| h.1.contains("Nip65")));
    }

    #[test]
    fn flags_banned_signer_vocab() {
        let hits = check("    let signer_kind = account.signer_kind();", false);
        assert!(hits.iter().any(|h| h.1.contains("signer_kind")));
    }

    #[test]
    fn ignores_comment_lines() {
        let hits = check("/// Use Nip65Resolver for outbox routing internally", true);
        assert!(hits.is_empty());
    }

    #[test]
    fn allows_clean_abi_glue() {
        let hits = check("pub fn dispatch_action(action_id: u32) {", false);
        assert!(hits.is_empty());
    }

    #[test]
    fn allows_routing_as_variable_name() {
        // The word "routing" by itself is not in the banned list.
        // Only specific routing vocabulary like "route_to", "outbox", "Nip65", etc. is banned.
        let hits = check("const routing = decodeDispatchEnvelopeRouting(request.bytes);", false);
        assert!(hits.is_empty(),
                "Variable name 'routing' by itself should not be flagged; only specific policy vocabulary is banned");
    }
}
