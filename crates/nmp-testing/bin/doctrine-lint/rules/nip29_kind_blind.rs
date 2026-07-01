//! nip29_kind_blind rule — `nmp-nip29` is a kind-blind transport (#2509 / #2513).
//!
//! `nmp-nip29` owns exactly two things: the group-routing envelope (the `h` /
//! `previous` / host-pin tags) and the NIP-29 kind namespace itself (9000–9022
//! moderation/user-management + 39000–39003 metadata). It MUST NOT name,
//! classify, or own any other event kind. Foreign kinds are authored by their
//! owning NIP — `nmp-nip25` builds the kind:7 reaction, `nmp-nip18` builds the
//! kind:16 repost, the app layer builds kind:11/other content — and routed into
//! a group through the single generic write surface
//! `nmp.nip29.publish_group_event` (`PublishGroupEventAction`), which injects
//! only the envelope. `nmp-nip29` never names a kind.
//!
//! ## What this catches (an ALLOWLIST, not a denylist)
//!
//! 1. A `const NAMESPACE: &str = "nmp.nip29.<verb>"` whose `<verb>` is NOT one
//!    of the legitimate lifecycle / admin / envelope verbs in [`ALLOWED_VERBS`].
//!    This catches a reintroduced `react_in_group` / `repost_in_group` /
//!    `share_event_in_group` AND any future renamed per-kind action
//!    (`like_in_group`, `pin_in_group`, …) without a brittle per-name denylist.
//! 2. The authoring constants `REACTION_KIND` / `REPOST_KIND` — the exact
//!    file-private kind:7 / kind:16 constants the deleted compound actions
//!    declared. Their reappearance in production source means a foreign kind is
//!    being named inside the kind-blind transport again.
//!
//! It deliberately does NOT scan for bare numeric `7` / `16` / `11` literals —
//! that is a false-positive generator (loop counters, lengths, indices) and
//! would itself be debt. The namespace allowlist plus the banned
//! authoring-constant identifiers are the principled, robust backstop.
//!
//! ## Scope
//!
//! Only `crates/nmp-nip29/src/`. App crates under `apps/<app>/` and the
//! `nmp-testing` fixtures/harness (which host negative examples) are exempt.
//!
//! ## Allowed exemptions
//!
//! - Comment lines (any of `//`, `///`, `//!`, inside `/* */`).
//! - Per-line `// doctrine-allow: nip29_kind_blind — reason` opt-out (reason
//!   REQUIRED, like the other ownership ratchets).

use std::path::Path;

pub const ID: &str = "nip29_kind_blind";

/// The legitimate `nmp.nip29.<verb>` action namespaces. Each maps to a real
/// lifecycle / admin / envelope-routing action that NIP-29 genuinely owns —
/// never a per-kind authoring verb for a foreign event kind:
///
/// - `create_group` — kind:9007 + kind:9002 group creation (public or private).
/// - `create_invite` — kind:9009 admin invite.
/// - `discover` — pushes a metadata interest (no publish).
/// - `edit_metadata` — kind:9002 admin metadata edit.
/// - `join` — kind:9021 user-management request.
/// - `leave` — kind:9022 user-management request.
/// - `publish_group_event` — the SOLE generic group-event write surface; injects
///   only the `h` / `previous` / host-pin envelope around any caller-built event.
/// - `put_user` — kind:9000 admin moderation.
/// - `set_parent` — kind:9002 subgroups edit (NIP-29 subgroups).
pub const ALLOWED_VERBS: &[&str] = &[
    "create_group",
    "create_invite",
    "discover",
    "edit_metadata",
    "join",
    "leave",
    "publish_group_event",
    "put_user",
    "set_parent",
];

/// The file-private kind-authoring constants the deleted compound actions used.
/// Their reappearance in `nmp-nip29` production source re-asserts ownership of a
/// foreign kind (kind:7 reaction / kind:16 repost).
const BANNED_AUTHORING_CONSTS: &[&str] = &["REACTION_KIND", "REPOST_KIND"];

/// True iff the file lives under `crates/nmp-nip29/src/`. App crates under
/// `apps/<app>/` and the `nmp-testing` crate (this rule's host + fixtures) are
/// exempt.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.contains("/apps/") || s.starts_with("apps/") {
        return false;
    }
    if s.contains("/crates/nmp-testing/") || s.starts_with("crates/nmp-testing/") {
        return false;
    }
    s.contains("/crates/nmp-nip29/src/") || s.starts_with("crates/nmp-nip29/src/")
}

/// Per-line check. Fires on (1) a non-allowlisted `nmp.nip29.<verb>` namespace
/// and (2) a banned kind-authoring constant identifier.
pub fn check(line: &str, is_comment: bool) -> Vec<(usize, String, String)> {
    if is_comment {
        return Vec::new();
    }
    let mut hits = Vec::new();

    if let Some((value_start, verb)) = parse_nip29_namespace_verb(line) {
        if !ALLOWED_VERBS.contains(&verb.as_str()) {
            hits.push((
                value_start + 1, // 1-indexed column at the opening quote
                format!(
                    "`nmp.nip29.{verb}` is a per-kind named group action — nmp-nip29 is a \
                     kind-blind transport that owns only the `h`/`previous`/host-pin envelope \
                     and the 9xxx/3900x kind namespace, never a foreign event kind. Author the \
                     kind via its owning NIP (kind:7 = nmp-nip25 reaction, kind:16 = nmp-nip18 \
                     repost, kind:11/other = app layer) and route it through the single generic \
                     `nmp.nip29.publish_group_event` write surface"
                ),
                "delete the per-kind action; build the event (correct kind + e/p tags) and \
                 dispatch `nmp.nip29.publish_group_event`, which injects only the envelope"
                    .to_string(),
            ));
        }
    }

    for ident in BANNED_AUTHORING_CONSTS {
        if let Some(col) = find_ident(line, ident) {
            hits.push((
                col + 1,
                format!(
                    "`{ident}` names a foreign event kind inside nmp-nip29 — a kind-blind \
                     transport must not declare authoring constants for kinds it does not own \
                     (kind:7 is nmp-nip25's reaction, kind:16 is nmp-nip18's repost)"
                ),
                "remove the constant; the owning NIP builds the event and routes it through \
                 `nmp.nip29.publish_group_event`"
                    .to_string(),
            ));
        }
    }

    hits
}

/// If `line` declares `const NAMESPACE: &str = "nmp.nip29.<verb>";` (or the
/// `&'static str` trait-associated form), return the opening-quote byte offset
/// and the `<verb>` segment. Returns `None` for any non-`nmp.nip29.*` namespace
/// or any non-NAMESPACE line.
fn parse_nip29_namespace_verb(line: &str) -> Option<(usize, String)> {
    let (value_start, value) = parse_namespace_literal(line)?;
    let verb = value.strip_prefix("nmp.nip29.")?;
    Some((value_start, verb.to_string()))
}

/// Mirror of `action_namespace`'s NAMESPACE-const detector: returns the opening
/// quote byte offset and the literal value for a `const NAMESPACE: &str = "…"` /
/// `const NAMESPACE: &'static str = "…"` declaration (tolerating a visibility
/// modifier). Returns `None` for any other line.
fn parse_namespace_literal(line: &str) -> Option<(usize, String)> {
    if !line.contains("NAMESPACE") || (!line.contains("&'static str") && !line.contains("&str")) {
        return None;
    }
    let ns_pos = line.find("NAMESPACE")?;
    let before = line[..ns_pos].trim_end();
    if !before.ends_with("const") {
        return None;
    }
    let eq_pos = line[ns_pos..].find('=').map(|i| ns_pos + i)?;
    let after_eq = &line[eq_pos + 1..];
    let quote_rel = after_eq.find('"')?;
    let value_start = eq_pos + 1 + quote_rel;
    let after_quote = &line[value_start + 1..];
    let close_rel = after_quote.find('"')?;
    Some((value_start, after_quote[..close_rel].to_string()))
}

/// Find `ident` in `line` as a whole identifier token (boundaries are non-`[A-Za-z0-9_]`).
fn find_ident(line: &str, ident: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let ilen = ident.len();
    let mut start = 0;
    while let Some(rel) = line[start..].find(ident) {
        let pos = start + rel;
        let before_ok = pos == 0 || !is_ident_char(bytes[pos - 1]);
        let after_idx = pos + ilen;
        let after_ok = after_idx >= bytes.len() || !is_ident_char(bytes[after_idx]);
        if before_ok && after_ok {
            return Some(pos);
        }
        start = pos + ilen;
    }
    None
}

fn is_ident_char(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn fires_on_reintroduced_react_in_group_namespace() {
        let hits = check(
            "    const NAMESPACE: &'static str = \"nmp.nip29.react_in_group\";",
            false,
        );
        assert_eq!(hits.len(), 1, "a per-kind named action must fire");
        assert!(
            hits[0].1.contains("kind-blind transport"),
            "message must state the kind-blind doctrine; got: {}",
            hits[0].1
        );
        assert!(
            hits[0].1.contains("publish_group_event"),
            "message must point at the generic write surface; got: {}",
            hits[0].1
        );
    }

    #[test]
    fn fires_on_future_renamed_per_kind_action() {
        // A future `like_in_group` is not on the allowlist — the allowlist
        // design catches it even though its name is brand new.
        let hits = check(
            "    const NAMESPACE: &'static str = \"nmp.nip29.like_in_group\";",
            false,
        );
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn silent_on_every_allowlisted_verb() {
        for verb in ALLOWED_VERBS {
            let line = format!("    const NAMESPACE: &'static str = \"nmp.nip29.{verb}\";");
            let hits = check(&line, false);
            assert!(
                hits.is_empty(),
                "allowlisted verb `{verb}` must not fire; got: {hits:?}"
            );
        }
    }

    #[test]
    fn silent_on_module_level_str_namespace_with_allowlisted_verb() {
        let hits = check(
            "pub const NAMESPACE: &str = \"nmp.nip29.publish_group_event\";",
            false,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn ignores_non_nip29_namespace() {
        // Out-of-NIP namespaces are action_namespace's concern, not this rule's.
        let hits = check(
            "    const NAMESPACE: &'static str = \"nmp.nip17.send\";",
            false,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn fires_on_reaction_kind_constant() {
        let hits = check("const REACTION_KIND: u32 = 7;", false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("REACTION_KIND"));
    }

    #[test]
    fn fires_on_repost_kind_constant() {
        let hits = check("    const REPOST_KIND: u32 = 16;", false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("REPOST_KIND"));
    }

    #[test]
    fn ignores_identifier_substring() {
        // `MY_REACTION_KINDNESS` is not the banned `REACTION_KIND` token.
        let hits = check("const MY_REACTION_KINDNESS: u32 = 1;", false);
        assert!(hits.is_empty(), "substring must not trip the ident match");
    }

    #[test]
    fn ignores_comment_line() {
        let hits = check(
            "    // const NAMESPACE: &'static str = \"nmp.nip29.react_in_group\"; REACTION_KIND",
            true,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn file_in_scope_targets_only_nip29_src() {
        assert!(file_in_scope(&PathBuf::from(
            "crates/nmp-nip29/src/action/mod.rs"
        )));
        assert!(file_in_scope(&PathBuf::from(
            "/abs/crates/nmp-nip29/src/kinds.rs"
        )));
        // Other protocol crates are out of scope (this is a nip29-specific rule).
        assert!(!file_in_scope(&PathBuf::from(
            "crates/nmp-nip25/src/lib.rs"
        )));
        // Apps and the testing crate are exempt.
        assert!(!file_in_scope(&PathBuf::from(
            "apps/chirp/crates/nmp-app-chirp/src/lib.rs"
        )));
        assert!(!file_in_scope(&PathBuf::from(
            "crates/nmp-testing/bin/doctrine-lint/fixtures/nip29_kind_blind/pos.rs"
        )));
    }
}
