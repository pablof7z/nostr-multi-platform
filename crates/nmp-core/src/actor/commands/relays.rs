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
use crate::relay::canonical_relay_url;

fn normalize_role(role: &str) -> Option<String> {
    crate::actor::canonical_relay_role(role)
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
        kernel.set_last_error_token(&crate::ui_token::UiToken::error(
            crate::ui_token::codes::RELAY_INVALID_URL,
            "invalid relay URL — expected wss:// or ws://",
        ));
        return None;
    };
    let Some(role) = normalize_role(role) else {
        kernel.set_last_error_token(&crate::ui_token::UiToken::error(
            crate::ui_token::codes::RELAY_INVALID_ROLE,
            "invalid relay role — expected read | write | both | indexer",
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
    // #114: a removed relay has zero diagnostic value — evict its transport
    // row (and any wire-subs still open on it) synchronously, in the same
    // dispatch as the `configured_relays` edit, rather than depend on the
    // socket's own async teardown to ever clear it (it never fully did — see
    // `relay_removed`'s doc comment).
    kernel.relay_removed(&canonical);
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

    /// chirp#114 — the load-bearing regression proof: a relay that failed to
    /// connect repeatedly (mirroring the field repro's climbing reconnect
    /// count / "Backing_off" row) must leave NO trace in Diagnostics once the
    /// user removes it — not a frozen "closed" row, no row at all. Before this
    /// fix `remove_relay` only ever touched `configured_relays`; the
    /// transport-diagnostics row had no eviction path whatsoever, so it
    /// outlived the relay it described.
    #[test]
    fn t_remove_relay_evicts_phantom_diagnostics_row() {
        use nmp_network::role::RelayRole;

        let mut kernel = fresh_kernel();
        add_relay(&mut kernel, "ws://127.0.0.1:19999", "read");
        // Simulate the repro: several failed connection attempts against a
        // dead relay before the user gives up and removes it.
        for _ in 0..14 {
            kernel.relay_failed(
                RelayRole::Content,
                "ws://127.0.0.1:19999",
                "tcp connect: Connection refused".to_string(),
            );
        }
        assert!(
            kernel.has_relay_diagnostics_row_for_test("ws://127.0.0.1:19999"),
            "sanity: the failed dial must have left a transport row"
        );

        remove_relay(&mut kernel, "ws://127.0.0.1:19999");

        assert!(kernel.configured_relays_snapshot().is_empty());
        assert!(
            !kernel.has_relay_diagnostics_row_for_test("ws://127.0.0.1:19999"),
            "#114: a removed relay must vanish from Diagnostics, not linger as \
             a phantom 'Backing_off' row"
        );
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
}
