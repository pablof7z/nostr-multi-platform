//! Display-only relay diagnostics formatters.
//!
//! Float casts and count truncations are acceptable for metrics labels that
//! are never used in arithmetic.
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]

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
    } else if lower == "unknown" || lower == "idle" || lower == "—" {
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

// ── String formatters ────────────────────────────────────────────────────

pub(super) fn role_label(role: &str) -> String {
    if role.is_empty() {
        "—".to_string()
    } else {
        title_case(role)
    }
}

pub(super) fn auth_label(auth: &str) -> String {
    if auth == "—" {
        auth.to_string()
    } else {
        title_case(auth)
    }
}

pub(super) fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut first = true;
    for c in s.chars() {
        if first {
            for u in c.to_uppercase() {
                out.push(u);
            }
            first = false;
        } else {
            out.push(c);
        }
    }
    out
}

pub(super) fn short_relay_url(url: &str) -> String {
    let stripped = url
        .strip_prefix("wss://")
        .or_else(|| url.strip_prefix("ws://"))
        .unwrap_or(url);
    stripped.trim_end_matches('/').to_string()
}

pub(super) fn short_id(id: &str) -> String {
    if id.chars().count() <= 12 {
        id.to_string()
    } else {
        let head: String = id.chars().take(8).collect();
        format!("{head}…")
    }
}

pub(super) fn format_bytes(bytes: u64) -> String {
    let kb = bytes as f64 / 1024.0;
    if kb < 1.0 {
        format!("{bytes} B")
    } else if kb < 1024.0 {
        format!("{kb:.1} KB")
    } else {
        format!("{:.1} MB", kb / 1024.0)
    }
}

pub(super) fn compact_count(n: u64) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        let v = n as f64 / 1_000.0;
        if v.fract() == 0.0 {
            format!("{}K", v as u64)
        } else {
            format!("{v:.1}K")
        }
    } else if n < 1_000_000_000 {
        let v = n as f64 / 1_000_000.0;
        if v.fract() == 0.0 {
            format!("{}M", v as u64)
        } else {
            format!("{v:.1}M")
        }
    } else {
        let v = n as f64 / 1_000_000_000.0;
        format!("{v:.1}B")
    }
}

