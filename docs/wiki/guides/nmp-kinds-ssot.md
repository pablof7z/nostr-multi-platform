---
title: NMP Kinds Single Source of Truth
slug: nmp-kinds-ssot
topic: crate-architecture
summary: KIND_SHORT_NOTE is renamed to KIND_SHORT_TEXT_NOTE throughout nmp-nip01 with no compat alias; all call sites in build.rs, decode.rs, meta_timeline, view, visibl
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-19
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:019edc13-83b1-7143-8631-b0e695ea4afd
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edc3c-53d4-73a0-8c42-a6b88a318e8c
  - session:019edc92-b628-7ce1-be8a-c3d1013f2969
---

# NMP Kinds Single Source of Truth

## Kinds Ssot

KIND_SHORT_NOTE is renamed to KIND_SHORT_TEXT_NOTE throughout nmp-nip01 with no compat alias; all call sites in build.rs, decode.rs, meta_timeline, view, visible_relations, and tests are updated. A stale KIND_SHORT_NOTE reference remains in docs/perf/op-centric-feed-architecture.md after the rename to KIND_SHORT_TEXT_NOTE in code. The WoT crate's KIND_MUTE_LIST (10000) changed from a local u32 literal to a re-export of nmp_core::kinds::KIND_MUTE_LIST, eliminating the last non-canonical kind integer in that crate. nmp-content-fixtures semantically uses nmp_core::kinds::is_parameterized_replaceable, so its event-cycle-key behavior changes for 20000..29999 (ephemeral) kinds from d-tag keyed to event-id keyed — this is an observable behavior change, not a no-op removal. `is_replaceable` has a single canonical definition in nmp-kinds (`matches!(kind, 0|3|41) || (10_000..20_000).contains(&kind)`) that nmp-core re-exports; the buggy nmp-core copy that returned true for all 0..=19999 is removed. `is_parameterized_replaceable` is defined as the range [30000, 40000); the earlier buggy definition that included ephemeral kinds 20000–29999 is removed. nmp-kinds must remain zero-dep (the nostr crate must NOT be added to it); canonical kind predicates are encoded directly. KindDtag.d_tag is Vec<u8>, not String as the issue text states. Per-NIP/per-kind branch tables in generic layers are D0 violations; the router `classify_kind` table and the test_router mirror are removed (PR #1533 merges this removal and fixes the lib.rs doc-lie claiming only 2-6 of 7 lanes are implemented), and `EventClass` threads through `RoutingContext` from the NIP-aware caller instead. nip21.rs and tags.rs are compliant kind-agnostic protocol codecs (their module docs cite D0), not per-NIP branches requiring change. The repost triple-path in nmp-core is test-only (#[cfg(test)]); the canonical decoder is nmp-nip18::try_from_kernel_event, already used by op_feed. nmp-content bare kind literals in sniff_mode_from_kind and resolve_embed_projection are replaced with named constants plus TODO(#1493) annotations (PR #1529 removes these bare literals); longform/mod.rs and embed_registry/view.rs already use named constants (including KIND_LONG_FORM_ARTICLE) and generic coordinate construction. u32 kinds above u16::MAX silently truncate before entering nostr::Kind in encode_nevent/encode_naddr (Some(65536) encodes as kind 0); since the public NMP surface accepts u32, these functions must reject out-of-range kinds with a typed Err. nmp-chirp-config relay-role has a confirmed drift between Rust ("both") and TypeScript ("both,indexer"); the single-source-of-truth fix is to generate the TS list from the Rust source. Presentation formatting (SF Symbol names, English labels, pluralization, bech32-encoded npub, initials, age formatting, emoji) must not live in Rust projections, snapshot types, or FFI paths; aim.md §2 is the authority and the in-code citations to §4.4/V-24/V-115 claiming otherwise are the violations being removed. Relay_diagnostics emits only raw tokens; its `*_tone` hue selectors were removed (#1802, joining the formatting functions short_url/short_id/title_case/format_bytes/compact_count already removed) — shells derive color from the raw tokens. The nip01 attribution slice removes redundant `author_display_name`/`author_picture_url` flat mirrors from `Nip10ReplyAttribution` and `AuthorDisplay.npub` from FFI; shells use the nested `authorDisplay` + `nmp_app_encode_profile` (the existing V-115 path). The nip29 slice emits raw `name`/`group_id`/`public`/`open`/`member_count` from `DiscoveredGroup`; removes `display_name`/`initials`/`subtitle` + `finalize_display_fields`; iOS uses computed-property extensions, 7 forbidden-logic tests deleted. Issue #1557 (ProfileCard raw fields + unknown kind:0 round-trip) is NMP-owned and schema/regen-heavy; its ownership should name nmp-nip01/profile rather than generic nmp. PR #1529 (nmp-content kind-literal cleanup) and PR #1533 (router classify_kind D0 removal + test_router dedup + doc-lie fix) are in CI and cleared to self-merge on green. P4 finding 1 fix: nmp-core stores host-declared follow_feed_kinds unconditionally (even without an active account), and the Android imperative openTimeline call is removed; PR #1545 merges both fixes.

<!-- citations: [^019ed-74] [^129d2-65] [^11850-15] [^019ed-77] [^11850-38] [^11850-57] [^11850-79] [^11850-121] [^019ed-140] [^11850-138] [^11850-192] [^11850-210] [^11850-227] [^11850-239] [^11850-251] -->
