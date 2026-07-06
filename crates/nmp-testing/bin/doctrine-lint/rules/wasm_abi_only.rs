//! `WASM_ABI_ONLY` — nmp-wasm/src/** must be narrowly ABI glue only.
//!
//! The nmp-wasm crate (the browser Worker ABI entry point, now retired; see
//! wasm_abi_gates.rs) was deleted in #2202. These responsibilities now live in
//! `nmp-browser-runtime::wasm` (the wasm-bindgen Worker export over
//! `NmpRuntimeCore`). This rule ensures the ABI boundary stays clean:
//! no domain business logic, routing, or policy vocabulary leaks into the
//! browser Worker transport adapter.
//!
//! For future nmp-wasm or browser-runtime ABI modules, this rule enforces:
//!
//! ## Banned imports (in non-comment lines)
//! - `nmp_router` (routing policy belongs in core; ABI only calls dispatch)
//! - `nmp_signer_broker` (signer management routing belongs in core)
//! - `nmp_nip*` (any action-module crate — NIP-specific logic belongs in core)
//! - `nmp_defaults` (default registration belongs in app, not ABI)
//! - `apps::` or `app_` prefixed imports (app composition belongs in app shells)
//!
//! Note: `nmp_signers::{LocalKeySigner, Signer}` is ALLOWED. The browser
//! runtime legitimately installs a local-key signer capability (D7/D10 design).
//! Only routing/policy surfaces like `nmp_router`, `nmp_signer_broker`, and
//! NIP action-module crates are banned.
//!
//! ## Banned policy vocabulary (in non-comment code lines)
//! - `outbox`, `route_to`, `Nip65`, `signer_kind`, `mailbox`, `publish_target`,
//!   `retry_policy` — any policy/routing/subscription control vocabulary
//!
//! Escape hatches: `// doctrine-allow: wasm_abi_only` with a required
//! justification (e.g., "// doctrine-allow: wasm_abi_only — FlatBuffers param
//! type requires direct import for codegen").

use std::path::Path;

pub const ID: &str = "WASM_ABI_ONLY";

const BANNED_IMPORTS: &[(&str, &str)] = &[
    ("nmp_router", "routing policy belongs in nmp-core; ABI only calls dispatch"),
    ("nmp_signer_broker", "signer management routing belongs in nmp-core"),
    ("nmp_nip", "NIP-specific logic belongs in carved-out nmp-nip* crates"),
    ("nmp_defaults", "default registration belongs in app composition, not ABI"),
];

const BANNED_POLICY_VOCAB: &[(&str, &str)] = &[
    ("outbox", "routing policy vocabulary forbidden in ABI transport"),
    ("route_to", "routing policy vocabulary forbidden in ABI transport"),
    ("Nip65", "NIP-specific policy forbidden in ABI transport"),
    ("signer_kind", "signer management policy forbidden in ABI transport"),
    ("mailbox", "mailbox/inbox policy forbidden in ABI transport"),
    ("publish_target", "publish-routing policy forbidden in ABI transport"),
    ("retry_policy", "subscription/reliability policy forbidden in ABI transport"),
];

pub fn file_is_in_scope(path: &Path) -> bool {
    // Applies to browser-runtime ABI modules under nmp-browser-runtime/src/wasm/**
    // (the wasm-bindgen Worker export interface).
    let s = path.to_string_lossy().replace('\\', "/");
    (s.contains("/nmp-browser-runtime/src/wasm/") || s.starts_with("nmp-browser-runtime/src/wasm/"))
        && !s.contains("/doc/") // Exempt doc tests from the strict ABI boundary
}

/// Returns `(col, message, suggested)` per match on `line`. `is_comment`
/// short-circuits the scan — the brief exempts doc-comment prose.
pub fn check(line: &str, is_comment: bool) -> Vec<(usize, String, String)> {
    if is_comment {
        return Vec::new();
    }
    let mut hits = Vec::new();

    // Check for banned imports (match prefix only — `nmp_router` in `use nmp_router::..` etc)
    for (import, reason) in BANNED_IMPORTS {
        if let Some(rel) = line.find(import) {
            // Verify it's a prefix match (character after is not alphanumeric/underscore)
            let end = rel + import.len();
            if end < line.len() {
                let after = line.chars().nth(end).unwrap_or(' ');
                if after.is_alphanumeric() || after == '_' {
                    continue; // Embedded in a larger word, skip
                }
            }
            let col = rel + 1; // 1-indexed
            hits.push((
                col,
                format!(
                    "banned import `{}` in ABI — {} (WASM_ABI_ONLY)",
                    import, reason
                ),
                format!("Use a type bridge from nmp-browser-runtime::wasm instead of importing {}", import),
            ));
        }
    }

    // Check for banned policy vocabulary (match prefix only)
    for (vocab, reason) in BANNED_POLICY_VOCAB {
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
                    "banned policy vocabulary `{}` in ABI — {} (WASM_ABI_ONLY)",
                    vocab, reason
                ),
                format!(
                    "Remove policy vocabulary from ABI glue; {} belongs in nmp-core/app composition",
                    vocab
                ),
            ));
            start = end;
        }
    }

    // Also ban direct `apps::` imports
    if line.contains("apps::") {
        if let Some(col) = line.find("apps::") {
            hits.push((
                col + 1,
                format!(
                    "banned app-layer import in ABI — app composition belongs in app shell, not Worker transport (WASM_ABI_ONLY)"
                ),
                "Move app-level logic out of the ABI boundary".to_string(),
            ));
        }
    }

    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_banned_import_nmp_router() {
        let hits = check("use nmp_router::OutboxRouter;", false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("nmp_router"));
    }

    #[test]
    fn flags_banned_import_nmp_nip() {
        let hits = check("use nmp_nip65::resolver::Nip65Resolver;", false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("nmp_nip"));
    }

    #[test]
    fn flags_banned_vocabulary_outbox() {
        let hits = check("    let outbox_url = self.resolver.outbox_for_pubkey(&pubkey);", false);
        assert!(hits.iter().any(|h| h.1.contains("outbox")),
                "Expected outbox to be flagged in: outbox_url");
    }

    #[test]
    fn flags_banned_vocabulary_route_to() {
        let hits = check("        route_to(subscription, relay_url);", false);
        assert!(hits.iter().any(|h| h.1.contains("route_to")));
    }

    #[test]
    fn ignores_comment_line() {
        let hits = check("/// Use nmp_router::OutboxRouter internally", true);
        assert!(hits.is_empty());
    }

    #[test]
    fn ignores_doc_string_mentions() {
        let hits = check("    /// The nmp_router handles outbox logic", true);
        assert!(hits.is_empty());
    }

    #[test]
    fn flags_app_import() {
        let hits = check("use apps::chirp::ChirpConfig;", false);
        assert!(hits.iter().any(|h| h.1.contains("app")));
    }

    #[test]
    fn allows_nmp_signers_import() {
        // nmp_signers is ALLOWED for local-key signer installation (D7/D10 design)
        let hits = check("use nmp_signers::{LocalKeySigner, Signer};", false);
        assert!(hits.is_empty(), "nmp_signers should be allowed in browser runtime");
    }

    #[test]
    fn allows_abi_bridge_types() {
        // FlatBuffers param types are allowed via exemption
        let hits = check("pub struct WorkerMessage {", false);
        assert!(hits.is_empty());
    }
}
