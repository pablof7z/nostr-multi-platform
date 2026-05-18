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
//! | `"both"`) is stored in the `RelayEditRow` projection. For the transport
//! pool, user-added relays are bucketed under `RelayRole::Content` — the
//! diagnostic lane that groups inbox/outbox user-content sockets. The
//! NIP-65 read/write split is handled by the outbox resolver, not by the
//! socket pool key (T105). `ensure_relay_worker` is idempotent on URL, so
//! calling it again for a role-edit of an already-connected relay is a
//! harmless no-op.
//!
//! T-relay-url-normalize: both `add_relay` and `remove_relay` route through
//! `canonical_relay_url` so the `RelayEditRow.url` field and the pool key in
//! `relay_controls` always agree, regardless of the case/trailing-slash form
//! the caller supplies.

use crate::kernel::{Kernel, RelayEditRow};
use crate::relay::canonical_relay_url;

fn normalize_role(role: &str) -> Option<&'static str> {
    match role.trim().to_ascii_lowercase().as_str() {
        "read" => Some("read"),
        "write" => Some("write"),
        "both" | "" => Some("both"),
        _ => None,
    }
}

/// Validate `url` and `role`, update the relay-edit projection, and return
/// the canonical URL so the caller can open a socket.
///
/// Canonicalization (T-relay-url-normalize): the URL is passed through
/// [`canonical_relay_url`] — lowercase scheme+host, empty-path trailing slash
/// stripped. The stored `RelayEditRow.url` is always the canonical form so
/// it matches the pool key `ensure_relay_worker` / `shutdown_relay_worker` use.
///
/// Returns `Some(canonical_url)` on success, `None` on any validation error
/// (an error toast is set on the kernel in that case).
pub(crate) fn add_relay(kernel: &mut Kernel, url: &str, role: &str) -> Option<String> {
    let canonical = match canonical_relay_url(url) {
        Some(u) => u,
        None => {
            kernel.set_last_error_toast(Some(
                "invalid relay URL — expected wss:// or ws://".to_string(),
            ));
            return None;
        }
    };
    let Some(role) = normalize_role(role) else {
        kernel.set_last_error_toast(Some(
            "invalid relay role — expected read | write | both".to_string(),
        ));
        return None;
    };
    let mut rows = kernel.relay_edit_rows_snapshot().to_vec();
    if let Some(existing) = rows.iter_mut().find(|r| r.url == canonical) {
        existing.role = role.to_string();
    } else {
        rows.push(RelayEditRow {
            url: canonical.clone(),
            role: role.to_string(),
        });
    }
    kernel.set_relay_edit_rows(rows);
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
    let mut rows = kernel.relay_edit_rows_snapshot().to_vec();
    let before = rows.len();
    rows.retain(|r| r.url != canonical);
    if rows.len() != before {
        kernel.set_relay_edit_rows(rows);
    }
}
