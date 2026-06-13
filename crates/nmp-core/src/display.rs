//! Shared display-string helpers for UI surfaces.
//!
//! Pure functions over plain data — no state, no I/O. Every function is
//! infallible (D6): parse / encode failures fall back to the raw input
//! string rather than panicking. The UX degrades gracefully (a hex snippet
//! in an avatar tile instead of an npub abbreviation) but never crashes.
//!
//! # Why these live in `nmp-core`
//!
//! ADR-0032 changed the projection contract: kernel snapshots and Layer-4
//! projection payloads carry raw protocol data, and presentation layers format
//! that data for display. These helpers remain for Rust presentation surfaces
//! (TUI, desktop, CLI/REPL, and tests) that need the same pure primitives:
//! bech32 encoding, npub abbreviation, hex abbreviation, initials, avatar tint,
//! and relative-time bucketing.
//!
//! Do not call these helpers from projection builders, snapshot structs, or
//! FFI serialization paths. Those paths must emit raw pubkeys, timestamps,
//! counts, and optional metadata so each host can choose its own presentation.
//!
//! V-33 — replaced duplicate helpers previously scattered across
//! `nmp-nip17`, `nmp-nip02`, `nmp-nip29`, `nmp-nip01`, `nmp-app-marmot`,
//! and `nmp-core/kernel/nostr.rs`. The cross-surface djb2 vector pinned in
//! [`tests`] (`"abcdef…0123456789" → "08E60C"`) anchors the canonical
//! algorithm; per-crate redundant copies of that pin were removed.

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
/// otherwise it is converted via [`to_npub`] first. Strings short enough to
/// fit without abbreviation (≤ 17 chars) are returned unchanged. Falls back
/// to the raw hex on any error (D6).
#[must_use]
pub fn short_npub(pubkey_hex: &str) -> String {
    let npub = to_npub(pubkey_hex);
    abbreviate(&npub, 10, 6)
}

/// Abbreviated hex form: first 8 chars + `"…"` + last 8 chars.
///
/// For raw hex identifiers (pubkeys, event IDs). Strings shorter than 16
/// characters are returned unchanged. Hex characters are single-byte ASCII,
/// so byte slicing is safe.
///
/// This is the canonical cross-surface algorithm for hex abbreviation — any
/// surface that shortens a hex pubkey or event ID must use this function so
/// abbreviations are consistent across the timeline, DMs, group chat, and
/// diagnostic labels.
#[must_use]
pub fn short_hex(value: &str) -> String {
    if value.len() < 16 {
        return value.to_string();
    }
    format!("{}…{}", &value[..8], &value[value.len() - 8..])
}

/// Two-char uppercase initials for an avatar tile.
///
/// Takes the first 2 characters of the bech32 body — the part after the
/// `"npub1"` prefix — and uppercases them. These are bech32 chars, so
/// always ASCII. Falls back gracefully when the `npub1` prefix is absent
/// (e.g. raw hex fallback from a parse error in [`to_npub`]).
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

/// Two-char uppercase initials from a display name.
///
/// Takes the first character of each of the first two whitespace-split words.
/// Pads with `"."` for single-word or empty names so the result is always
/// exactly two characters. Examples: `"Alice Smith"` → `"AS"`, `"alice"` →
/// `"A."`, `""` → `".."`.
///
/// This is the canonical cross-surface algorithm for name-based avatars — any
/// surface that shows a display name in an avatar tile must use this function
/// so initials are consistent.
#[must_use]
pub fn display_name_initials(name: &str) -> String {
    let chars: Vec<char> = name
        .split_whitespace()
        .take(2)
        .filter_map(|word| word.chars().next())
        .map(|c| c.to_uppercase().next().unwrap_or(c))
        .collect();
    match chars.as_slice() {
        [a, b] => format!("{a}{b}"),
        [a] => format!("{a}."),
        _ => "..".to_string(),
    }
}

/// Deterministic 6-hex avatar background colour from a hex pubkey
/// (uppercase, no `#` prefix).
///
/// Algorithm: djb2 over the **last 6 bytes** of the pubkey hex string in
/// natural order, masked to 24 bits, formatted as 6 uppercase hex chars.
///
/// This is the **canonical cross-surface helper**: every UI that shows an
/// avatar tile for the same author — DMs, NIP-29 group chat, the modular
/// timeline, the Accounts toolbar, Marmot rows — must produce the same
/// tint, so the algorithm is pinned here and verified by the
/// `avatar_color_hex_matches_pinned_djb2_vector` test below.
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

/// Abbreviated `"X ago"` relative-time label for a Unix-seconds timestamp.
///
/// The canonical `"Xs ago"` / `"Xm ago"` / `"Xh ago"` / `"Xd ago"` dialect
/// shared by every surface that renders a relative time. This is a
/// PRESENTATION helper (ADR-0032): projection builders must never call it —
/// they emit raw Unix-epoch timestamps and the shells (iOS / Android / the
/// Rust TUI's `relay_settings::format::format_ms_ago`) format at render time
/// (aim.md §62). The relay-diagnostics projection used to embed pre-formatted
/// labels via a `format_ago_ms` helper; that helper was deleted when the
/// projection switched to raw `*_ms` fields.
///
/// `now_secs` is the wall-clock "now" in Unix seconds — injected so the
/// helper itself does no I/O and stays deterministic in tests.
///
/// When `then_secs == 0` or the message is "in the future" relative to
/// `now_secs` (clock skew, or a sender stamp slightly ahead of the
/// receiver), the label is `"now"`.
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

/// Abbreviate a string to `head` chars + `"…"` + `tail` chars.
///
/// If the string is short enough to fit without abbreviation
/// (`count <= head + tail + 1`) it is returned unchanged (no trailing
/// ellipsis on short strings).
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

    /// A fresh keypair — the helpers under test only consume the public-key
    /// hex, so any valid secp256k1 pair works as a fixture.
    fn test_keys() -> Keys {
        Keys::generate()
    }

    // ── to_npub ──────────────────────────────────────────────────────────

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

    // ── short_npub ───────────────────────────────────────────────────────

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
    fn short_npub_falls_back_on_garbage_input() {
        let s = short_npub("zz");
        assert_eq!(s, "zz", "short string returned unchanged");
    }

    /// Pins the canonical short-npub form of the NmpGallery demo pubkey
    /// (`apps/nmp-gallery/ios/.../GalleryModel.swift::DEMO_NPUB_SHORT`).
    ///
    /// The Swift `bestEffortProfile` fallback renders this exact literal
    /// before kind:0 arrives so the user-* component screenshots show
    /// real-shape data on the first frame. Per aim.md §6.9 Swift never
    /// reformats npubs — so any drift in [`short_npub`] must show up
    /// here (and require updating the Swift literal alongside).
    #[test]
    fn short_npub_pins_nmp_gallery_demo_account() {
        // pablof7z, the NmpGallery demo account.
        let hex = "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52";
        assert_eq!(
            short_npub(hex),
            "npub1l2vyh…utajft",
            "if this fails, update DEMO_NPUB_SHORT in \
             apps/nmp-gallery/ios/NmpGallery/Bridge/GalleryModel.swift"
        );
    }

    // ── short_hex ────────────────────────────────────────────────────────

    #[test]
    fn short_hex_long_input_first_eight_ellipsis_last_eight() {
        let hex = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        assert_eq!(short_hex(hex), "abcdef01…23456789");
    }

    #[test]
    fn short_hex_short_input_returned_unchanged() {
        assert_eq!(short_hex(""), "");
        assert_eq!(short_hex("abcd"), "abcd");
    }

    #[test]
    fn short_hex_boundary_sixteen_chars_is_abbreviated() {
        assert_eq!(short_hex("0123456789abcdef"), "01234567…89abcdef");
    }

    // ── display_name_initials ────────────────────────────────────────────

    #[test]
    fn display_name_initials_word_based_two_words() {
        assert_eq!(display_name_initials("Alice Smith"), "AS");
        assert_eq!(display_name_initials("alice bob"), "AB");
    }

    #[test]
    fn display_name_initials_single_word_pads_with_dot() {
        assert_eq!(display_name_initials("alice"), "A.");
        assert_eq!(display_name_initials("Bob"), "B.");
    }

    #[test]
    fn display_name_initials_empty_returns_two_dots() {
        assert_eq!(display_name_initials(""), "..");
        assert_eq!(display_name_initials("   "), "..");
    }

    #[test]
    fn display_name_initials_only_first_two_words_count() {
        assert_eq!(display_name_initials("alice bob carol"), "AB");
    }

    // ── avatar_initials ──────────────────────────────────────────────────

    #[test]
    fn avatar_initials_extracts_two_chars_after_npub1_prefix() {
        let npub = "npub1abcdefgh";
        let initials = avatar_initials(npub);
        assert_eq!(
            initials, "AB",
            "initials should be first 2 chars after 'npub1'"
        );
    }

    #[test]
    fn avatar_initials_from_real_pubkey() {
        let keys = test_keys();
        let hex = keys.public_key().to_hex();
        let npub = to_npub(&hex);
        let initials = avatar_initials(&npub);
        assert_eq!(initials.len(), 2, "initials must be 2 chars");
        assert!(
            initials.is_ascii(),
            "initials must be ASCII, got: {initials}"
        );
        assert_eq!(
            initials,
            initials.to_uppercase(),
            "initials must be uppercased, got: {initials}"
        );
    }

    // ── avatar_color_hex ─────────────────────────────────────────────────

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
        // D6: helper accepts any input.
        let _ = avatar_color_hex("zz");
        let _ = avatar_color_hex("");
    }

    #[test]
    fn avatar_color_hex_matches_pinned_djb2_vector() {
        // The load-bearing cross-surface anchor: this is the byte-for-byte
        // output of the djb2 helper for a known 64-char hex input. Every
        // UI surface that renders an avatar tile for this author must
        // produce this tint — DMs, NIP-29 group chat, the modular
        // timeline, the Accounts toolbar, Marmot rows. If this assertion
        // ever flips, the avatar tint for the same author has diverged
        // across surfaces; that is a regression, not a fix.
        let hex = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        assert_eq!(avatar_color_hex(hex), "08E60C");
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
