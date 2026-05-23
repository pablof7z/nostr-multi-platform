//! Display-string helpers for NIP-17 DM surfaces.
//!
//! Per the thin-shell rule, every UI string shown in the DM UX is computed
//! here in Rust and surfaced through the snapshot payload. Swift renders what
//! it receives — it never encodes pubkeys or derives avatar colours.
//!
//! These are pure functions over hex pubkey strings — no state, no I/O, no
//! fallible operations that cross the FFI.
//!
//! # D6
//!
//! Every code path that could fail (pubkey parse, bech32 encode) falls back
//! to the raw input string rather than panicking. The UX degrades gracefully
//! (a hex snippet in the avatar tile instead of an npub abbreviation) but
//! never crashes.

use nostr::{nips::nip19::ToBech32, PublicKey};

/// Convert a hex pubkey to a bech32 `npub1…` string.
///
/// On any parse or encode error the raw hex is returned verbatim (D6).
#[must_use]
pub fn to_npub(pubkey_hex: &str) -> String {
    match PublicKey::parse(pubkey_hex) {
        Ok(pk) => pk.to_bech32().unwrap_or_else(|_| pubkey_hex.to_string()),
        Err(_) => pubkey_hex.to_string(),
    }
}

/// Abbreviated bech32 form: first 10 chars + `"…"` + last 6 chars of the npub.
///
/// If `pubkey_hex` is already an `npub1…` string it is abbreviated directly;
/// otherwise it is converted first. Falls back to raw hex on any error (D6).
#[must_use]
pub fn short_npub(pubkey_hex: &str) -> String {
    let npub = to_npub(pubkey_hex);
    abbreviate(&npub, 10, 6)
}

/// Two-char uppercase initials for the avatar tile.
///
/// Takes the first 2 characters of the bech32 body — the part after the
/// `"npub1"` prefix — and uppercases them. These are bech32 chars, so always
/// ASCII. Falls back gracefully when the `npub1` prefix is absent (e.g. raw
/// hex fallback from a parse error in `to_npub`).
#[must_use]
pub fn avatar_initials(npub: &str) -> String {
    let body = npub.strip_prefix("npub1").unwrap_or(npub);
    let chars: Vec<char> = body.chars().take(2).collect();
    match chars.as_slice() {
        [a, b] => format!("{a}{b}").to_uppercase(),
        [a] => a.to_uppercase().to_string(),
        _ => "?".to_string(),
    }
}

/// Abbreviated "X ago" relative-time label for a Unix-seconds timestamp.
///
/// Mirrors the bucketing of `kernel/relay_diagnostics.rs::format_ago_ms` so
/// surfaces across the app speak the same dialect ("3s ago" / "12m ago" /
/// "5h ago" / "2d ago"). Replaces the iOS `RelativeDateTimeFormatter` call
/// `dmRelativeTime` in the DM views (V-20 thin-shell fix — aim.md §2:
/// display formatting is Rust-owned).
///
/// `now_secs` is the wall-clock "now" in Unix seconds — injected so the
/// snapshot path stays deterministic in tests and the helper itself does no
/// I/O. The DM projection reads `SystemTime::now()` once per snapshot tick
/// and threads it through; the helper never reaches for a clock.
///
/// When `then_secs == 0` or the message is "in the future" relative to `now`
/// (clock skew, or a sender stamp slightly ahead of the receiver), the label
/// is `"now"` — matching the relay-diagnostics convention.
#[must_use]
pub fn format_ago_secs(now_secs: u64, then_secs: u64) -> String {
    if then_secs == 0 || now_secs <= then_secs {
        return "now".to_string();
    }
    let diff = now_secs - then_secs;
    if diff < 60 {
        format!("{diff}s ago")
    } else if diff < 3_600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86_400 {
        format!("{}h ago", diff / 3_600)
    } else {
        format!("{}d ago", diff / 86_400)
    }
}

/// Deterministic 6-hex avatar background colour from a hex pubkey (uppercase,
/// no `#` prefix).
///
/// Uses the same djb2 algorithm as `nmp-marmot/src/projection/display.rs` so
/// colours are consistent across surfaces: djb2 over the **last 6 bytes** of
/// the pubkey hex string in natural order.
#[must_use]
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

/// Abbreviate a string to `head` chars + `"…"` + `tail` chars.
///
/// If the string is short enough to fit without abbreviation it is returned
/// unchanged (no trailing ellipsis on short strings).
fn abbreviate(s: &str, head: usize, tail: usize) -> String {
    if s.chars().count() <= head + tail + 1 {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let head_s: String = chars.iter().take(head).collect();
    let tail_s: String = chars.iter().skip(chars.len() - tail).collect();
    format!("{head_s}…{tail_s}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{FromBech32, Keys};

    /// A known secp256k1 keypair — deterministic test vector.
    fn test_keys() -> Keys {
        Keys::generate()
    }

    #[test]
    fn to_npub_produces_bech32_for_valid_hex() {
        let keys = test_keys();
        let hex = keys.public_key().to_hex();
        let npub = to_npub(&hex);
        assert!(
            npub.starts_with("npub1"),
            "to_npub must produce an npub1… string, got: {npub}"
        );
        let pk = nostr::PublicKey::from_bech32(&npub).expect("round-trip");
        assert_eq!(pk.to_hex(), hex);
    }

    #[test]
    fn to_npub_falls_back_to_raw_on_garbage_input() {
        let garbage = "not-a-valid-hex-pubkey";
        assert_eq!(to_npub(garbage), garbage);
    }

    #[test]
    fn short_npub_abbreviates_to_ten_plus_six() {
        let keys = test_keys();
        let hex = keys.public_key().to_hex();
        let short = short_npub(&hex);
        assert!(
            short.starts_with("npub1"),
            "short npub must start with npub1, got: {short}"
        );
        assert!(
            short.contains('…'),
            "short npub must contain ellipsis, got: {short}"
        );
        let visible: Vec<char> = short.chars().collect();
        assert_eq!(
            visible.len(),
            17,
            "short_npub must be exactly 10 + 1 + 6 chars, got: {short}"
        );
    }

    #[test]
    fn avatar_initials_extracts_two_chars_after_npub1_prefix() {
        let npub = "npub1abcdefgh";
        let initials = avatar_initials(npub);
        assert_eq!(initials, "AB", "initials should be first 2 chars after 'npub1'");
    }

    #[test]
    fn avatar_initials_from_real_pubkey() {
        let keys = test_keys();
        let hex = keys.public_key().to_hex();
        let npub = to_npub(&hex);
        let initials = avatar_initials(&npub);
        assert_eq!(initials.len(), 2, "initials must be 2 chars");
        assert!(
            initials.chars().all(|c| c.is_ascii()),
            "initials must be ASCII, got: {initials}"
        );
        assert_eq!(
            initials,
            initials.to_uppercase(),
            "initials must be uppercased, got: {initials}"
        );
    }

    #[test]
    fn avatar_color_hex_is_deterministic_and_six_uppercase_hex() {
        let keys = test_keys();
        let hex = keys.public_key().to_hex();

        let color_a = avatar_color_hex(&hex);
        let color_b = avatar_color_hex(&hex);
        assert_eq!(color_a, color_b, "avatar_color_hex must be deterministic");
        assert_eq!(color_a.len(), 6, "must be exactly 6 chars");
        assert!(
            color_a.chars().all(|c| c.is_ascii_hexdigit()),
            "must be hex chars, got: {color_a}"
        );
        assert_eq!(
            color_a,
            color_a.to_uppercase(),
            "must be uppercase, got: {color_a}"
        );
    }

    #[test]
    fn avatar_color_hex_differs_between_distinct_pubkeys() {
        let k1 = Keys::generate();
        let k2 = Keys::generate();
        assert_ne!(
            avatar_color_hex(&k1.public_key().to_hex()),
            avatar_color_hex(&k2.public_key().to_hex()),
            "distinct pubkeys should (almost always) produce distinct colours"
        );
    }

    #[test]
    fn avatar_color_hex_on_garbage_does_not_panic() {
        let _ = avatar_color_hex("zz");
        let _ = avatar_color_hex("");
    }

    #[test]
    fn short_npub_falls_back_on_garbage_input() {
        let s = short_npub("zz");
        assert_eq!(s, "zz", "short string returned unchanged");
    }

    // ── format_ago_secs ──────────────────────────────────────────────────

    #[test]
    fn format_ago_secs_zero_then_is_now() {
        assert_eq!(format_ago_secs(1_000_000_000, 0), "now");
    }

    #[test]
    fn format_ago_secs_future_then_is_now() {
        assert_eq!(format_ago_secs(100, 200), "now");
        assert_eq!(format_ago_secs(100, 100), "now");
    }

    #[test]
    fn format_ago_secs_seconds_bucket() {
        assert_eq!(format_ago_secs(105, 100), "5s ago");
        assert_eq!(format_ago_secs(159, 100), "59s ago");
    }

    #[test]
    fn format_ago_secs_minutes_bucket() {
        assert_eq!(format_ago_secs(160, 100), "1m ago");
        assert_eq!(format_ago_secs(100 + 59 * 60, 100), "59m ago");
    }

    #[test]
    fn format_ago_secs_hours_bucket() {
        assert_eq!(format_ago_secs(100 + 3_600, 100), "1h ago");
        assert_eq!(format_ago_secs(100 + 23 * 3_600, 100), "23h ago");
    }

    #[test]
    fn format_ago_secs_days_bucket() {
        assert_eq!(format_ago_secs(100 + 86_400, 100), "1d ago");
        assert_eq!(format_ago_secs(100 + 7 * 86_400, 100), "7d ago");
    }
}
