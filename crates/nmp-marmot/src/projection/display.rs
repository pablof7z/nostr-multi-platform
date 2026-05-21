//! Display-string helpers used by the snapshot / message-row projection.
//!
//! Per aim.md §6 anti-pattern #1 ("Duplicated formatting logic across
//! platforms — Rust pre-formats into strings, native renders them") and
//! the `apps/chirp/AGENTS.md` "canonical bad example", every UI string
//! the Marmot surface needs is computed here.
//!
//! These are pure functions over snapshot inputs — no state, no I/O. They
//! exist in the projection module (not at crate root) because they only
//! serve the FFI-payload layer; the substrate `domain` / `view` / `action`
//! modules deliberately stay payload-agnostic.

use nostr::{nips::nip19::ToBech32, PublicKey};

/// First 2 ASCII letters of `name`, uppercased; falls back to `"?"` on
/// empty input. Used for avatar tiles. The 2-char prefix is bytewise, not
/// grapheme-wise — matches the Swift code we are replacing (one
/// observable change is unicode handling, but the Swift `String.prefix`
/// was also bytewise via `Character` truncation for ASCII labels).
pub fn initials(name: &str) -> String {
    let mut chars = name.chars().filter(|c| !c.is_whitespace());
    let a = chars.next();
    let b = chars.next();
    match (a, b) {
        (Some(x), Some(y)) => format!("{}{}", x, y).to_uppercase(),
        (Some(x), None) => x.to_uppercase().to_string(),
        _ => "?".to_string(),
    }
}

/// `"3 members"` / `"1 member"` — Rust-owned pluralisation.
pub fn member_count_display(count: usize) -> String {
    if count == 1 {
        "1 member".to_string()
    } else {
        format!("{count} members")
    }
}

/// `Some("3")` when `count > 0`, else `None`. The UI renders the badge
/// `if let unread = row.unread_display` — no derivation in native.
pub fn unread_display(count: u64) -> Option<String> {
    if count == 0 {
        None
    } else {
        Some(count.to_string())
    }
}

/// `Some("1 invite")` / `Some("3 invites")` / `None`. Drives the
/// top-of-list invite chip with no count branching in Swift.
pub fn invites_chip_label(count: usize) -> Option<String> {
    match count {
        0 => None,
        1 => Some("1 invite".to_string()),
        n => Some(format!("{n} invites")),
    }
}

/// Empty-name fallback. Avoids `name.isEmpty ? "Untitled group" : name`
/// in Swift.
pub fn group_display_name(name: &str) -> String {
    if name.is_empty() {
        "Untitled group".to_string()
    } else {
        name.to_string()
    }
}

/// Empty-name fallback for a welcome / invite row.
pub fn welcome_display_name(name: &str) -> String {
    if name.is_empty() {
        "Group invite".to_string()
    } else {
        name.to_string()
    }
}

/// Compact bech32 form `npub1abcd…wxyz` — 10-char head + 6-char tail. If
/// `pubkey_hex` is already an `npub1…` string, abbreviates that; if it is
/// hex, converts via `nostr::PublicKey` first, falling back to the raw
/// input when the hex cannot be parsed (D6 — render the raw string rather
/// than crash).
pub fn short_npub(pubkey_hex: &str) -> String {
    if pubkey_hex.starts_with("npub1") {
        return abbreviate(pubkey_hex, 10, 6);
    }
    match PublicKey::parse(pubkey_hex) {
        Ok(pk) => match pk.to_bech32() {
            Ok(b) => abbreviate(&b, 10, 6),
            Err(_) => abbreviate(pubkey_hex, 10, 6),
        },
        Err(_) => abbreviate(pubkey_hex, 10, 6),
    }
}

/// `npub1abcd…wxyz` (8 + 4) — used for inline error strings where the
/// shorter form fits better.
pub fn short_npub_compact(pubkey_hex: &str) -> String {
    if pubkey_hex.starts_with("npub1") {
        return abbreviate(pubkey_hex, 8, 4);
    }
    match PublicKey::parse(pubkey_hex) {
        Ok(pk) => match pk.to_bech32() {
            Ok(b) => abbreviate(&b, 8, 4),
            Err(_) => abbreviate(pubkey_hex, 8, 4),
        },
        Err(_) => abbreviate(pubkey_hex, 8, 4),
    }
}

fn abbreviate(s: &str, head: usize, tail: usize) -> String {
    if s.chars().count() <= head + tail + 1 {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let head_s: String = chars.iter().take(head).collect();
    let tail_s: String = chars.iter().skip(chars.len() - tail).collect();
    format!("{head_s}…{tail_s}")
}

/// Deterministic 6-hex avatar tint derived from `pubkey_hex` (uppercase,
/// no `#` prefix). Bytewise-equivalent to the previous Swift derivation:
/// djb2 over the **last 6 chars** of the hex string in NATURAL order
/// (`hex.suffix(6).utf8` in Swift → `&hex[hex.len()-6..]` in Rust). The
/// match means existing renders keep their tints across this migration.
pub fn avatar_color_hex(pubkey_hex: &str) -> String {
    let bytes = pubkey_hex.as_bytes();
    let start = bytes.len().saturating_sub(6);
    let tail = &bytes[start..];
    let mut hash: u32 = 5381;
    for b in tail {
        hash = hash.wrapping_mul(33).wrapping_add(u32::from(*b));
    }
    format!("{:06X}", hash & 0x00FF_FFFF)
}

/// Relative-time stamp ("now", "12s", "3m", "5h", "2d", "1w") computed
/// against `now_secs`. Same coarse buckets `RelativeDateTimeFormatter`
/// reaches for at `.abbreviated`. Future timestamps (clock skew) report
/// as `"now"`.
pub fn relative_time(unix_secs: u64, now_secs: u64) -> String {
    if unix_secs >= now_secs {
        return "now".to_string();
    }
    let delta = now_secs - unix_secs;
    const MIN: u64 = 60;
    const HOUR: u64 = 60 * MIN;
    const DAY: u64 = 24 * HOUR;
    const WEEK: u64 = 7 * DAY;
    const YEAR: u64 = 365 * DAY;
    if delta < 5 {
        "now".to_string()
    } else if delta < MIN {
        format!("{delta}s")
    } else if delta < HOUR {
        format!("{}m", delta / MIN)
    } else if delta < DAY {
        format!("{}h", delta / HOUR)
    } else if delta < WEEK {
        format!("{}d", delta / DAY)
    } else if delta < YEAR {
        format!("{}w", delta / WEEK)
    } else {
        format!("{}y", delta / YEAR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_basic() {
        assert_eq!(initials("Trusted Circle"), "TR");
        assert_eq!(initials("a"), "A");
        assert_eq!(initials(""), "?");
        assert_eq!(initials("   spaces"), "SP");
    }

    #[test]
    fn member_count_pluralises() {
        assert_eq!(member_count_display(0), "0 members");
        assert_eq!(member_count_display(1), "1 member");
        assert_eq!(member_count_display(7), "7 members");
    }

    #[test]
    fn invites_chip_label_pluralises() {
        assert_eq!(invites_chip_label(0), None);
        assert_eq!(invites_chip_label(1), Some("1 invite".to_string()));
        assert_eq!(invites_chip_label(4), Some("4 invites".to_string()));
    }

    #[test]
    fn relative_time_buckets() {
        let now: u64 = 100_000_000;
        assert_eq!(relative_time(now, now), "now");
        assert_eq!(relative_time(now + 5, now), "now"); // future
        assert_eq!(relative_time(now - 30, now), "30s");
        assert_eq!(relative_time(now - 5 * 60, now), "5m");
        assert_eq!(relative_time(now - 3 * 3600, now), "3h");
        assert_eq!(relative_time(now - 2 * 86_400, now), "2d");
        assert_eq!(relative_time(now - 3 * 7 * 86_400, now), "3w");
    }

    #[test]
    fn short_npub_falls_back_on_garbage() {
        // Hex that does not decode → still abbreviated, not panicked.
        let s = short_npub("zz");
        assert_eq!(s, "zz");
    }

    #[test]
    fn avatar_color_is_deterministic_and_six_hex() {
        let a = avatar_color_hex("abcdef1234567890");
        assert_eq!(a.len(), 6);
        let b = avatar_color_hex("abcdef1234567890");
        assert_eq!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() && c.is_ascii_uppercase() || c.is_ascii_digit()));
    }
}
