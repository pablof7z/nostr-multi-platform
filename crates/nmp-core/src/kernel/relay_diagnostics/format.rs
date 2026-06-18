//! Semantic tone selectors for relay diagnostics.
//!
//! These functions decide which semantic class a row belongs to — the shell
//! maps each tone token to a platform Color. String formatting (labels,
//! compact counts, byte sizes) belongs in the shell, not here.

// ── Hue selectors (semantic tone, not a Color value) ─────────────────────

pub(super) fn role_tone(role: &str) -> &'static str {
    match role {
        "write" => "write",
        _ => "accent",
    }
}

pub(super) fn connection_tone(connection: &str) -> &'static str {
    let lower = connection.to_ascii_lowercase();
    if lower == "connected" {
        "ok"
    } else if lower.starts_with("disconnect") || lower == "failed" {
        "error"
    } else if lower.contains("connect") {
        // "reconnecting", "connecting", "auth_paused_will_reconnect", etc.
        "warn"
    } else if lower == "unknown" || lower == "idle" || lower == "—" || lower == "blocked" {
        "muted"
    } else {
        "error"
    }
}

pub(super) fn auth_tone(auth: &str) -> &'static str {
    let lower = auth.to_ascii_lowercase();
    if lower == "ok" || lower == "authenticated" {
        "ok"
    } else if lower == "pending" {
        "warn"
    } else {
        "muted"
    }
}

pub(super) fn state_tone(state: &str) -> &'static str {
    match state.to_ascii_lowercase().as_str() {
        "open" | "active" | "live" => "ok",
        "pending" | "warming" | "opening" | "auth_paused" => "warn",
        _ => "muted",
    }
}

pub(super) fn interest_state_tone(state: &str) -> &'static str {
    match state {
        "active" | "warming" | "tailing" | "complete" => "ok",
        "idle" => "muted",
        _ => "warn",
    }
}
