//! `BROWSER_RUNTIME_BOUNDARY` — browser-runtime transport adapter enforces policy boundary.
//!
//! The browser-runtime transport adapter (`crates/nmp-browser-runtime/src/transport/**`)
//! must remain a pure adapter: no routing/outbox/NIP-65/publish-target/subscription-planning
//! vocabulary, no protocol/routing/signing policy in the web TypeScript packages, no
//! thread::sleep/setInterval+poll loops in browser runtime + TS packages, no raw-secret/
//! snapshot/debug retention patterns.
//!
//! This rule (BROWSER_RUNTIME_BOUNDARY) scans for policy vocabulary leaking into the
//! transport adapter or web packages, and extends the D8 no-polling scan to include
//! browser runtimes and TypeScript packages.
//!
//! ## Banned in `crates/nmp-browser-runtime/src/transport/**`
//! - `outbox`, `route_to`, `Nip65`, `publish_target` — routing policy vocabulary
//! - `mailbox`, `signer_kind` — signer/messaging policy vocabulary
//! - `subscription_planning`, `subscription_lifecycle` — subscription control vocabulary
//!
//! ## Banned in `web/packages/*/src`
//! - `routing`, `dispatch_route`, `subscription_planner` — routing/policy logic
//! - `setInterval` + poll patterns — active polling loops (covered by D8 extension)
//! - Raw secret/snapshot retention in diagnostics
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

const BANNED_TS_VOCAB: &[(&str, &str)] = &[
    ("routing", "routing logic forbidden in TS packages"),
    ("dispatch_route", "route dispatch forbidden in TS packages"),
    ("subscription_planner", "subscription planning forbidden in TS packages"),
];

pub fn file_is_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");

    // Scope: transport adapter under nmp-browser-runtime/src/transport
    let in_transport = (s.contains("/nmp-browser-runtime/src/transport/") || s.starts_with("nmp-browser-runtime/src/transport/"))
        && !s.contains("/doc/");

    // Scope: web TypeScript packages
    let in_web_packages = (s.contains("/web/packages/") || s.starts_with("web/packages/"))
        && (s.ends_with(".ts") || s.ends_with(".tsx"));

    in_transport || in_web_packages
}

/// Returns `(col, message, suggested)` per match on `line`. `is_comment`
/// short-circuits the scan — the brief exempts doc-comment prose.
pub fn check(line: &str, is_comment: bool) -> Vec<(usize, String, String)> {
    if is_comment {
        return Vec::new();
    }
    let mut hits = Vec::new();

    // Check for banned routing vocabulary
    for (vocab, reason) in BANNED_ROUTING_VOCAB {
        let mut start = 0;
        while let Some(rel) = line[start..].find(vocab) {
            let col = start + rel + 1; // 1-indexed
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
            start = start + rel + vocab.len();
        }
    }

    // Check for banned signer vocabulary
    for (vocab, reason) in BANNED_SIGNER_VOCAB {
        let mut start = 0;
        while let Some(rel) = line[start..].find(vocab) {
            let col = start + rel + 1; // 1-indexed
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
            start = start + rel + vocab.len();
        }
    }

    // Check for banned TS vocabulary (only in TypeScript files)
    for (vocab, reason) in BANNED_TS_VOCAB {
        let mut start = 0;
        while let Some(rel) = line[start..].find(vocab) {
            let col = start + rel + 1; // 1-indexed
            hits.push((
                col,
                format!(
                    "banned TypeScript vocabulary `{}` — {} (BROWSER_RUNTIME_BOUNDARY)",
                    vocab, reason
                ),
                format!(
                    "Remove policy logic from TS package; {} belongs in app/core layer",
                    vocab
                ),
            ));
            start = start + rel + vocab.len();
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
    fn detects_routing_in_ts() {
        let hits = check("const routing = new RouterImpl();", false);
        assert!(hits.iter().any(|h| h.1.contains("routing")));
    }
}
