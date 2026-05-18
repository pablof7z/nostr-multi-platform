//! Relay-edit handlers.
//!
//! The kernel's socket layer is a fixed two-role model (Content / Indexer
//! with constant URLs); arbitrary user-added relay sockets are a
//! relay-manager change beyond T66a scope. These handlers maintain the
//! editable relay-row projection (D4: actor is sole writer) so the Accounts
//! screen can render + edit the set; wiring each row to a live socket is a
//! documented follow-up.

use crate::kernel::{Kernel, RelayEditRow};

fn normalize_role(role: &str) -> Option<&'static str> {
    match role.trim().to_ascii_lowercase().as_str() {
        "read" => Some("read"),
        "write" => Some("write"),
        "both" | "" => Some("both"),
        _ => None,
    }
}

pub(crate) fn add_relay(kernel: &mut Kernel, url: &str, role: &str) {
    let url = url.trim();
    if !(url.starts_with("wss://") || url.starts_with("ws://")) {
        kernel.set_last_error_toast(Some(
            "invalid relay URL — expected wss:// or ws://".to_string(),
        ));
        return;
    }
    let Some(role) = normalize_role(role) else {
        kernel.set_last_error_toast(Some(
            "invalid relay role — expected read | write | both".to_string(),
        ));
        return;
    };
    let mut rows = kernel.relay_edit_rows_snapshot().to_vec();
    if let Some(existing) = rows.iter_mut().find(|r| r.url == url) {
        existing.role = role.to_string();
    } else {
        rows.push(RelayEditRow {
            url: url.to_string(),
            role: role.to_string(),
        });
    }
    kernel.set_relay_edit_rows(rows);
    kernel.set_last_error_toast(None);
}

pub(crate) fn remove_relay(kernel: &mut Kernel, url: &str) {
    let url = url.trim();
    let mut rows = kernel.relay_edit_rows_snapshot().to_vec();
    let before = rows.len();
    rows.retain(|r| r.url != url);
    if rows.len() != before {
        kernel.set_relay_edit_rows(rows);
    }
}
