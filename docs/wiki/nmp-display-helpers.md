---
title: NMP Display Helpers & Cross-Surface Formatting
slug: nmp-display-helpers
summary: All display helper primitives (`to_npub`, `short_npub`, `short_hex`, `display_name_initials`, `avatar_color_hex`, `format_ago_secs`) live exclusively in `nmp-co
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-26
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:12b3f443-3c2d-4e47-976a-7f4ceab75343
  - session:eb342a0d-84e3-4289-9873-88a947ca8144
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
---

# NMP Display Helpers & Cross-Surface Formatting

## Canonical Location

All display helper primitives (`to_npub`, `short_npub`, `short_hex`, `display_name_initials`, `avatar_color_hex`, `format_ago_secs`) live exclusively in `nmp-core::display` as the canonical cross-surface source of truth. Per D6 doctrine, display separation is mandatory: backend projections must emit raw data, and `display::` helpers are banned from kernel, projection, and FFI code. [^12b3f-8]

<!-- citations: [^12b3f-8] [^f2605-8] -->
## Avatar Color

The djb2 canonical color algorithm produces avatar colors as `format!("{:06X}", hash & 0x00FF_FFFF)` over the last 6 bytes of the pubkey hex string, with no `#` prefix and 6 uppercase hex characters. [^12b3f-9]

## short_npub

`short_npub` is the canonical user-facing pubkey display function, producing bech32-encoded abbreviated pubkeys in the format `npub1<first10>…<last6>` (17 chars total). The `npub` and `npubShort` fields carried in `ProfileWire` must be Rust-formatted; no Swift-side or Kotlin-side reformatting is performed.

<!-- citations: [^12b3f-10] [^53838-6] -->
## short_hex

`short_hex` is the canonical hex abbreviation function for technical IDs (event IDs, secondary identifier slots), producing `8…8` format with Unicode ellipsis and a `<16` threshold (abbreviates at 16+ chars). The local `short_hex` function in `kernel/nostr.rs` (6..6 format, threshold 12, ASCII `..` separator) is intentionally kept separate from the canonical `short_hex` because it serves internal subscription status labels with a different contract. [^12b3f-11]

## display_name_initials

`display_name_initials` produces word-based initials (first character of each whitespace-split word, up to 2 words) — this is the canonical algorithm used across all surfaces for avatar initials derived from display names. [^12b3f-12]

## Abbreviation Thresholds

The `abbreviate` helper in `display.rs` uses a `≤17` threshold (preserves 17-char strings unchanged), while `short_hex` uses `<16` (abbreviates at 16+ chars) — the two cannot share the same threshold. [^12b3f-13]

## DM Conversation Display

DM conversation rows display the peer's name from their NIP-01 profile, falling back to short npub form, never raw hex. DM conversation avatars use profile pictures or name initials, never first hex characters. [^eb342-9]
## See Also

