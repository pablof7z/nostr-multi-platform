---
title: Shell Formatting Boundary
slug: shell-formatting-boundary
topic: ffi-runtime
summary: Presentation formatting (SF Symbol names, English title/label/prose, avatar_initials, short_npub, bucket_age, emoji, pluralization) must not live in Rust projec
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-19
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edcf5-0586-7960-ba68-0b4e9fb81117
---

# Shell Formatting Boundary

## Formatting Boundary

Presentation formatting (SF Symbol names, English title/label/prose, avatar_initials, short_npub, bucket_age, emoji, pluralization) must not live in Rust projection builders, snapshot types, or FFI serialization paths; shells own all formatting and render from raw data tokens emitted by Rust. The canonical source is aim.md §2 #4 + §2 anti-patterns (lines 60-68): Rust sends raw data (pubkeys as hex, timestamps as Unix integers, display names verbatim), and Rust display helpers (short_npub, avatar_initials, etc.) are legitimate ONLY in TUI/CLI/test fixtures — NEVER inside projection builders, snapshot types, or FFI serialization paths. Note: aim.md §4.4 governs outbox/smart-relay routing (NIP-65), NOT presentation formatting — in-code comments citing "doctrine §4.4" or "aim.md §4.4" for this formatting rule are miscitations; the real source is aim.md §2 #4. aim.md §2 overrides contradictory in-code citations to §4.4 and ADR-0032. (Previously: ADR-0032/#1099 directed precomputed labels in Rust.) ADR-0032 is amended with a dated note recording that #1099's status_label/status_tone was a regression (not sanctioned by the ADR) and its removal in #1580, plus the miscitation correction.

The Rust kernel emits raw values at the formatting boundary:
- publish_outbox emits raw kind/content/status/attempt/counts; iOS NotificationsView+OutboxRow and the TUI shell own title/icon/preview/label/summary formatting. SF Symbol names ('person.crop.circle', 'heart', 'text.bubble') must be removed from nmp-core.
- relay_diagnostics emits raw values (role, connection, auth as lowercase strings; bytesRx/bytesTx as u64 counters; discoveryKinds as Vec<u64>; consumerCount as u32; eventsRx as u64; state as lowercase string) and never pre-formats display strings; shells derive all display formatting (title-cased labels, compact counts, formatted byte sizes, short URLs, short wire IDs, discovery-kind labels) at render time.
- The discovery module (relay_diagnostics/discovery.rs) returns raw Vec<u64> kind numbers instead of a pre-formatted label string; the label-to-kind mapping (0→"profile", 3→"follows", 10002→"relay-list") is removed from the kernel and must be replicated by each shell.
- signer_state emits raw semantic tokens (connection_state, signer_kind, stage); display labels (status_label, status_tone, stage_label) are removed from Rust FlatBuffers. Shells render display labels from raw semantic tokens (signer_kind, state, stage) via shared parity-consistent helper mappings, reversing ADR-0032/#1099 for display labels per aim.md §2. P4 F3 (SignInScreen signerKind label switch) collapses into P9 PR3's labels-to-shells change: the shell renders the raw signer_kind token through a shared mapping helper, not a Rust precompute.
- NIP-29 discovered groups emit raw name/group_id/public/open/member_count; display_name, initials, subtitle and finalize_display_fields are removed from Rust.
- KeyPackageStatus emits raw published/age_secs/stale + is_registered:bool; bucket_age, render_subtitle and action_label are removed from Rust.
- NIP-01 Nip10ReplyAttribution removes redundant flat author_display_name/author_picture_url mirrors; AuthorDisplay.npub is removed (shells use nmp_app_encode_profile).
- format.rs retains only tone-selector functions (role_tone, connection_tone, auth_tone, state_tone, interest_state_tone); all display string formatters (title_case, short_relay_url, short_id, format_bytes, compact_count, role_label, auth_label) are removed from the kernel.

The *_tone semantic-hue tokens (role_tone, connection_tone, auth_tone, state_tone, interest_state_tone) are retained as raw tokens on the wire and are not reformatted by shells; they emit raw tokens, not colors/prose.

Android WalletScreen must bind the Rust-computed WalletStatus.is_connected bool verbatim, not derive connection state from the tone discriminant.

Known per-shell divergences in display formatting:
- Discovery-kind labels: TUI renders empty as "none", maps 10002→"relay-list", unknowns→"list"; iOS renders empty as "", maps 10002→"relay list", 10003→"bookmarks", unknowns→"kind:N"; Android has no discovery-kind label computed property at all.
- Byte-size formatting: Android uses binary KiB/MiB/GiB; TUI uses decimal KB/MB/GB; iOS delegates to locale-dependent ByteCountFormatter with .binary style; the old Rust kernel used mixed KB/MB notation.
- TUI connection-classification logic (status_dot, zero_count_label, relay_is_connected) switched to raw lowercase connection tokens but still uses display-ish substring matching (contains("disconnected"), contains("connected")), which is fragile and should ideally use the retained connection_tone or an exact raw-token table.

<!-- citations: [^11850-173] [^11850-143] [^11850-144] [^11850-145] [^11850-146] [^11850-147] [^11850-148] [^11850-168] [^11850-169] [^11850-170] [^11850-171] [^11850-172] [^019ed-145] [^11850-219] [^11850-231] [^11850-245] -->
