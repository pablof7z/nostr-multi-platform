//! Relay-edit handlers.
//!
//! `add_relay` now returns `Some(canonical_url)` on success so the dispatch
//! layer can call `ensure_relay_worker` and open a live socket for the new
//! entry (T158). The canonical URL is produced by
//! [`crate::relay::canonical_relay_url`] — lowercase scheme+host, empty-path
//! trailing slash stripped. `None` is returned on any validation failure
//! (invalid URL scheme or unrecognised role); the caller MUST NOT spawn a
//! worker in that case.
//!
//! Role semantics: the user-facing NIP-65 role string (`"read"` | `"write"`
//! | `"both"`) is stored in the `AppRelay` projection. For the transport
//! pool, user-added relays are bucketed under `RelayRole::Content` — the
//! diagnostic lane that groups inbox/outbox user-content sockets. The
//! NIP-65 read/write split is handled by the outbox resolver, not by the
//! socket pool key (T105). `ensure_relay_worker` is idempotent on URL, so
//! calling it again for a role-edit of an already-connected relay is a
//! harmless no-op.
//!
//! T-relay-url-normalize: both `add_relay` and `remove_relay` route through
//! `canonical_relay_url` so the `AppRelay.url` field and the pool key in
//! `relay_controls` always agree, regardless of the case/trailing-slash form
//! the caller supplies.

use crate::kernel::{AppRelay, Kernel};
use crate::kinds::KIND_RELAY_LIST;
use crate::relay::canonical_relay_url;
use crate::substrate::UnsignedEvent;

// V-57 P2 (2026-05-27) — the relay-list kind constant lives in the
// workspace-canonical [`crate::kinds`] registry. The wire-shape contract
// with `nmp-router`'s `PublishRelayListAction` is still held by the
// round-trip tests in that crate; the integer is now declared in one
// place across the workspace instead of duplicated here.

fn normalize_role(role: &str) -> Option<String> {
    crate::actor::canonical_relay_role(role)
}

/// Build the NIP-65 third-element marker — if any — for a `AppRelay.role`
/// string.
///
/// * `Some(None)`              — emit `["r", url]` (the "both" / default case).
/// * `Some(Some("read"))`      — emit `["r", url, "read"]`.
/// * `Some(Some("write"))`     — emit `["r", url, "write"]`.
/// * `None`                    — the row has no NIP-65 representation (e.g.
///   pure indexer); the caller drops it.
///
/// Role semantics mirror `nmp-core::actor::relay_roles`:
/// * `read`                     → read-only
/// * `write`                    → write-only
/// * `both` / `""` (empty)      → both (default marker omitted)
/// * `both,indexer`             → both (indexer has no NIP-65 marker; dropped)
/// * `read,indexer`             → read-only
/// * `write,indexer`            → write-only
/// * `indexer` (alone)          → no NIP-65 representation; row is dropped
/// * unrecognised role          → row is dropped (D6 — degrade gracefully)
#[allow(clippy::option_option)] // Outer None = drop row; Some(None) = both-marker; Some(Some(x)) = directional marker
fn nip65_marker_for_role(role: &str) -> Option<Option<&'static str>> {
    let canonical = crate::actor::canonical_relay_role(role)?;
    // `canonical_relay_role` returns one of:
    //   "both" | "read" | "write" | "indexer" | "both,indexer"
    //   | "read,indexer" | "write,indexer"
    // (the role tokens are sorted in a fixed order by that function).
    match canonical.as_str() {
        "both" | "both,indexer" => Some(None),
        "read" | "read,indexer" => Some(Some("read")),
        "write" | "write,indexer" => Some(Some("write")),
        // Pure-indexer rows have no NIP-65 read/write semantics (internal
        // discovery lane) — drop them, along with any unrecognised role.
        _ => None,
    }
}

/// Build a NIP-65 kind:10002 **unsigned** event from the current
/// `AppRelay` projection — the active account's intended outbox/inbox
/// set.
///
/// Used by the `AddRelay` / `RemoveRelay` dispatch arms to re-publish the
/// user's NIP-65 metadata whenever the local relay set changes, so other
/// clients reading the relay graph see the same set the user just edited.
///
/// Row → tag mapping is the [`nip65_marker_for_role`] table. Pure-indexer
/// rows are dropped (NIP-65 has no indexer concept); the indexer suffix on
/// composite roles is also dropped. URLs are NOT re-canonicalised here —
/// `AppRelay.url` is already the canonical form (every `add_relay`
/// caller routes through `canonical_relay_url`).
///
/// The returned event:
/// * has `kind = 10002`,
/// * has `created_at = 0` — the D7 sentinel; the actor re-stamps it,
/// * has an empty `pubkey` — the active signer fills it at sign time.
///
/// Returns `None` when the projection would produce zero `r` tags — the
/// caller MUST NOT publish in that case, because an empty kind:10002 is
/// the "clear my NIP-65 metadata" signal (see
/// `kernel::ingest::relay_list::ingest_relay_list`), and we never want a
/// `RemoveRelay` that leaves indexer-only rows behind to accidentally wipe
/// the cache for the user. Concretely the caller (`AddRelay` / `RemoveRelay`
/// arms) skips the publish-piggyback step in that branch — the local
/// projection mutation still stands.
pub(crate) fn build_relay_list_event(rows: &[AppRelay]) -> Option<UnsignedEvent> {
    let mut tags: Vec<Vec<String>> = Vec::with_capacity(rows.len());
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        let Some(marker_opt) = nip65_marker_for_role(&row.role) else {
            continue;
        };
        // Defensive dedup: the projection should already be url-unique
        // (add_relay updates rather than appending), but a guard here means
        // a future projection change can't silently emit a kind:10002 with
        // duplicate `r` tags.
        if !seen.insert(row.url.clone()) {
            continue;
        }
        let tag = match marker_opt {
            None => vec!["r".to_string(), row.url.clone()],
            Some(marker) => vec!["r".to_string(), row.url.clone(), marker.to_string()],
        };
        tags.push(tag);
    }
    if tags.is_empty() {
        return None;
    }
    Some(UnsignedEvent {
        // Empty placeholder — the actor re-derives the pubkey from the
        // signing key at sign time (see `publish_unsigned_event`).
        pubkey: String::new(),
        kind: KIND_RELAY_LIST,
        tags,
        content: String::new(),
        // D7 sentinel — the actor re-stamps from `kernel.now_secs()`.
        created_at: 0,
    })
}

/// Validate `url` and `role`, update the relay-edit projection, and return
/// the canonical URL so the caller can open a socket.
///
/// Canonicalization (T-relay-url-normalize): the URL is passed through
/// [`canonical_relay_url`] — lowercase scheme+host, empty-path trailing slash
/// stripped. The stored `AppRelay.url` is always the canonical form so
/// it matches the pool key `ensure_relay_worker` / `shutdown_relay_worker` use.
///
/// Returns `Some(canonical_url)` on success, `None` on any validation error
/// (an error toast is set on the kernel in that case).
pub(crate) fn add_relay(kernel: &mut Kernel, url: &str, role: &str) -> Option<String> {
    let Some(canonical) = canonical_relay_url(url) else {
        kernel.set_last_error_toast(Some(
            "invalid relay URL — expected wss:// or ws://".to_string(),
        ));
        return None;
    };
    let Some(role) = normalize_role(role) else {
        kernel.set_last_error_toast(Some(
            "invalid relay role — expected read | write | both | indexer".to_string(),
        ));
        return None;
    };
    let mut rows = kernel.configured_relays_snapshot().to_vec();
    if let Some(existing) = rows.iter_mut().find(|r| r.url == canonical) {
        *existing = AppRelay::new(existing.url.clone(), role);
    } else {
        rows.push(AppRelay::new(canonical.clone(), role));
    }
    kernel.set_configured_relays(rows);
    kernel.set_last_error_toast(None);
    Some(canonical)
}

pub(crate) fn remove_relay(kernel: &mut Kernel, url: &str) {
    // Canonicalize so that removing "wss://r.ex/" finds the row stored as
    // "wss://r.ex" (T-relay-url-normalize).
    let canonical = match canonical_relay_url(url) {
        Some(u) => u,
        None => url.trim().to_string(), // best-effort for non-ws URLs (no-op in practice)
    };
    let mut rows = kernel.configured_relays_snapshot().to_vec();
    let before = rows.len();
    rows.retain(|r| r.url != canonical);
    if rows.len() != before {
        kernel.set_configured_relays(rows);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    fn fresh_kernel() -> Kernel {
        Kernel::new(DEFAULT_VISIBLE_LIMIT)
    }

    // --- normalize_role: pure function, no Kernel needed -------------------

    #[test]
    fn t_normalize_role_read() {
        assert_eq!(normalize_role("read").as_deref(), Some("read"));
    }

    #[test]
    fn t_normalize_role_write() {
        assert_eq!(normalize_role("write").as_deref(), Some("write"));
    }

    #[test]
    fn t_normalize_role_both() {
        assert_eq!(normalize_role("both").as_deref(), Some("both"));
    }

    #[test]
    fn t_normalize_role_indexer() {
        // `indexer` is a real canonical variant (used by the discovery lane).
        assert_eq!(normalize_role("indexer").as_deref(), Some("indexer"));
    }

    #[test]
    fn t_normalize_role_content_and_indexer() {
        assert_eq!(
            normalize_role("write read indexer").as_deref(),
            Some("both,indexer")
        );
        assert_eq!(
            normalize_role("both,indexer").as_deref(),
            Some("both,indexer")
        );
    }

    #[test]
    fn t_normalize_role_unknown_is_none() {
        assert_eq!(normalize_role("unknown"), None);
        // The task description mentions "wallet" — confirm it is NOT accepted
        // by the actual code (the doc/task list was inaccurate).
        assert_eq!(normalize_role("wallet"), None);
    }

    #[test]
    fn t_normalize_role_empty_defaults_to_both() {
        // The `"both" | "" => Some("both")` arm is intentional: an empty role
        // string defaults to "both" rather than being rejected.
        assert_eq!(normalize_role("").as_deref(), Some("both"));
    }

    #[test]
    fn t_normalize_role_is_case_insensitive() {
        // `normalize_role` lowercases via `to_ascii_lowercase()` before matching.
        assert_eq!(normalize_role("READ").as_deref(), Some("read"));
        assert_eq!(normalize_role("Write").as_deref(), Some("write"));
        assert_eq!(normalize_role("BOTH").as_deref(), Some("both"));
    }

    #[test]
    fn t_normalize_role_trims_whitespace() {
        // Leading/trailing whitespace is stripped before matching.
        assert_eq!(normalize_role("  read  ").as_deref(), Some("read"));
    }

    // --- add_relay / remove_relay: need a Kernel --------------------------

    #[test]
    fn t_add_relay_valid_appears_in_state() {
        let mut kernel = fresh_kernel();
        let result = add_relay(&mut kernel, "wss://relay.example", "read");
        assert_eq!(result, Some("wss://relay.example".to_string()));

        let rows = kernel.configured_relays_snapshot();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url, "wss://relay.example");
        assert_eq!(rows[0].role, "read");
        // Success clears any prior error toast.
        assert_eq!(kernel.last_error_toast_snapshot(), None);
    }

    #[test]
    fn t_add_relay_invalid_url_returns_none_and_sets_toast() {
        let mut kernel = fresh_kernel();
        // `http://` is not a ws/wss scheme — canonicalization fails.
        let result = add_relay(&mut kernel, "http://relay.example", "read");
        assert_eq!(result, None);
        assert!(kernel.configured_relays_snapshot().is_empty());
        assert!(kernel.last_error_toast_snapshot().is_some());
    }

    #[test]
    fn t_add_relay_invalid_role_returns_none_and_sets_toast() {
        let mut kernel = fresh_kernel();
        let result = add_relay(&mut kernel, "wss://relay.example", "bogus-role");
        assert_eq!(result, None);
        // No row is added when the role is rejected.
        assert!(kernel.configured_relays_snapshot().is_empty());
        assert!(kernel.last_error_toast_snapshot().is_some());
    }

    #[test]
    fn t_add_relay_duplicate_updates_role_in_place() {
        let mut kernel = fresh_kernel();
        add_relay(&mut kernel, "wss://relay.example", "read");
        // Re-adding the same URL with a different role updates the existing
        // row instead of pushing a second one.
        add_relay(&mut kernel, "wss://relay.example", "write");

        let rows = kernel.configured_relays_snapshot();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].role, "write");
    }

    #[test]
    fn t_add_relay_canonicalizes_url() {
        let mut kernel = fresh_kernel();
        // Mixed-case scheme/host + trailing slash → canonical lowercase form.
        let result = add_relay(&mut kernel, "WSS://Relay.Example/", "read");
        assert_eq!(result, Some("wss://relay.example".to_string()));

        let rows = kernel.configured_relays_snapshot();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url, "wss://relay.example");
    }

    #[test]
    fn t_add_then_remove_relay() {
        let mut kernel = fresh_kernel();
        add_relay(&mut kernel, "wss://relay.example", "read");
        assert_eq!(kernel.configured_relays_snapshot().len(), 1);

        remove_relay(&mut kernel, "wss://relay.example");
        assert!(kernel.configured_relays_snapshot().is_empty());
    }

    #[test]
    fn t_remove_relay_canonicalizes_url() {
        let mut kernel = fresh_kernel();
        // Stored canonical: "wss://relay.example". Remove using a non-canonical
        // form (trailing slash + mixed case) — canonicalization must still match.
        add_relay(&mut kernel, "wss://relay.example", "read");
        remove_relay(&mut kernel, "WSS://Relay.Example/");
        assert!(kernel.configured_relays_snapshot().is_empty());
    }

    #[test]
    fn t_remove_relay_nonexistent_is_noop() {
        let mut kernel = fresh_kernel();
        add_relay(&mut kernel, "wss://relay.example", "read");
        // Removing a URL that was never added leaves existing rows untouched.
        remove_relay(&mut kernel, "wss://other.example");

        let rows = kernel.configured_relays_snapshot();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url, "wss://relay.example");
    }

    // --- build_relay_list_event ----------------------------
    //
    // These tests pin the wire-shape contract for the AddRelay/RemoveRelay
    // auto-trigger path. They cover the four `AppRelay.role` shapes
    // that show up in production projections plus the empty/indexer-only
    // degenerate cases.

    fn row(url: &str, role: &str) -> AppRelay {
        AppRelay::new(url.to_string(), role.to_string())
    }

    #[test]
    fn t_build_relay_list_event_kind_is_10002() {
        let rows = [row("wss://relay.example", "both")];
        let event = build_relay_list_event(&rows).expect("non-empty rows");
        assert_eq!(event.kind, 10002);
    }

    #[test]
    fn t_build_relay_list_event_uses_d7_created_at_sentinel() {
        let rows = [row("wss://relay.example", "both")];
        let event = build_relay_list_event(&rows).expect("non-empty rows");
        assert_eq!(
            event.created_at, 0,
            "D7: created_at is the 0 sentinel — the actor re-stamps it"
        );
    }

    #[test]
    fn t_build_relay_list_event_leaves_pubkey_empty() {
        let rows = [row("wss://relay.example", "both")];
        let event = build_relay_list_event(&rows).expect("non-empty rows");
        assert!(
            event.pubkey.is_empty(),
            "pubkey is filled by the actor from the active signer at sign time"
        );
    }

    #[test]
    fn t_build_relay_list_event_both_omits_marker() {
        // NIP-65: `["r", url]` (no third element) is the canonical
        // read+write shape.
        let rows = [row("wss://relay.example", "both")];
        let event = build_relay_list_event(&rows).expect("non-empty rows");
        assert_eq!(
            event.tags,
            vec![vec!["r".to_string(), "wss://relay.example".to_string()]]
        );
    }

    #[test]
    fn t_build_relay_list_event_read_emits_read_marker() {
        let rows = [row("wss://relay.example", "read")];
        let event = build_relay_list_event(&rows).expect("non-empty rows");
        assert_eq!(
            event.tags,
            vec![vec![
                "r".to_string(),
                "wss://relay.example".to_string(),
                "read".to_string(),
            ]]
        );
    }

    #[test]
    fn t_build_relay_list_event_write_emits_write_marker() {
        let rows = [row("wss://relay.example", "write")];
        let event = build_relay_list_event(&rows).expect("non-empty rows");
        assert_eq!(
            event.tags,
            vec![vec![
                "r".to_string(),
                "wss://relay.example".to_string(),
                "write".to_string(),
            ]]
        );
    }

    #[test]
    fn t_build_relay_list_event_skips_pure_indexer_rows() {
        // Pure-indexer rows are an NMP-internal lane (discovery probes).
        // They have no NIP-65 representation; the row is dropped entirely.
        let rows = [
            row("wss://indexer.example", "indexer"),
            row("wss://relay.example", "both"),
        ];
        let event = build_relay_list_event(&rows).expect("non-empty rows");
        assert_eq!(event.tags.len(), 1);
        assert_eq!(event.tags[0][1], "wss://relay.example");
    }

    #[test]
    fn t_build_relay_list_event_strips_indexer_suffix_on_composite_roles() {
        // `both,indexer` rolls up to NIP-65 "both" (no marker). The indexer
        // half is NMP-internal and has no NIP-65 expression.
        let rows = [row("wss://relay.example", "both,indexer")];
        let event = build_relay_list_event(&rows).expect("non-empty rows");
        assert_eq!(
            event.tags,
            vec![vec!["r".to_string(), "wss://relay.example".to_string()]]
        );
    }

    #[test]
    fn t_build_relay_list_event_preserves_input_order() {
        let rows = [
            row("wss://b.example", "both"),
            row("wss://a.example", "read"),
            row("wss://c.example", "write"),
        ];
        let event = build_relay_list_event(&rows).expect("non-empty rows");
        let urls: Vec<&String> = event.tags.iter().map(|t| &t[1]).collect();
        assert_eq!(
            urls,
            vec!["wss://b.example", "wss://a.example", "wss://c.example"]
        );
    }

    #[test]
    fn t_build_relay_list_event_returns_none_for_empty_rows() {
        let event = build_relay_list_event(&[]);
        assert!(
            event.is_none(),
            "an empty projection MUST NOT produce a kind:10002 — that would \
             clear the author_relay_lists cache on ingest"
        );
    }

    #[test]
    fn t_build_relay_list_event_returns_none_for_indexer_only_projection() {
        // Indexer-only rows produce zero NIP-65 entries — the builder must
        // signal `None` so the caller skips the publish piggyback and does
        // NOT emit a destructive empty kind:10002.
        let rows = [row("wss://indexer.example", "indexer")];
        let event = build_relay_list_event(&rows);
        assert!(
            event.is_none(),
            "an indexer-only projection has no NIP-65 expression — must \
             return None so the dispatch arm skips re-publishing"
        );
    }
}
