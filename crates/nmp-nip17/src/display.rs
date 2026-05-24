//! Display-string helpers for NIP-17 DM surfaces.
//!
//! Per the thin-shell rule (aim.md §2), every UI string shown in the DM UX
//! is computed here in Rust and surfaced through the snapshot payload.
//! Swift renders what it receives — it never encodes pubkeys or derives
//! avatar colours.
//!
//! # V-33 — canonical implementation lives in `nmp-core::display`
//!
//! All five helpers (`to_npub`, `short_npub`, `avatar_initials`,
//! `avatar_color_hex`, `format_ago_secs`) are re-exported from
//! [`nmp_core::display`]. This module is the stable NIP-17 facade so
//! existing in-crate call sites (`use crate::display;`) keep compiling
//! unchanged; the algorithms themselves — and the cross-surface pinned
//! djb2 vector that anchors avatar-tint consistency — live in the kernel
//! crate every consumer already depends on.

pub use nmp_core::display::{
    avatar_color_hex, avatar_initials, format_ago_secs, short_npub, to_npub,
};

#[cfg(test)]
mod tests {
    //! Smoke tests asserting the re-exports resolve to the canonical helpers
    //! and behave as the DM ingest / snapshot path expects. The exhaustive
    //! bucket / round-trip / pinned-vector coverage lives in
    //! [`nmp_core::display::tests`].
    use super::*;
    use nostr::Keys;

    #[test]
    fn re_exports_resolve_and_round_trip() {
        let keys = Keys::generate();
        let hex = keys.public_key().to_hex();
        let npub = to_npub(&hex);
        assert!(npub.starts_with("npub1"));
        let short = short_npub(&hex);
        assert!(short.contains('…'));
        let initials = avatar_initials(&npub);
        assert_eq!(initials.len(), 2);
        let color = avatar_color_hex(&hex);
        assert_eq!(color.len(), 6);
    }

    #[test]
    fn format_ago_secs_smoke() {
        assert_eq!(format_ago_secs(1_000_000_000, 0), "now");
        assert_eq!(format_ago_secs(105, 100), "5s ago");
        assert_eq!(format_ago_secs(160, 100), "1m ago");
    }
}
