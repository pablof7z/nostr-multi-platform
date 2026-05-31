---
title: Reactive Profile Mentions — LiveProfileMap Architecture
slug: reactive-profile-mentions-architecture
summary: "Inline nostr:npub1 mention chips resolve profiles reactively via LiveProfileMap, which ingests three kernel projections and falls back to truncated npub until kind:0 arrives."
tags:
  - profiles
  - mentions
  - reactivity
  - gallery
  - projections
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-31
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:752b523f-231e-4fca-ab86-748c35b5dd74
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
  - session:9b9db91a-b324-4c11-aacf-421d9aab2819
---

# Reactive Profile Mentions — LiveProfileMap Architecture

> Inline nostr:npub1 mention chips resolve profiles reactively via LiveProfileMap, which ingests three kernel projections and falls back to truncated npub until kind:0 arrives.

## Overview

Inline `nostr:npub1…` / `nostr:nprofile1…` URIs in note content render as interactive mention chips. The chip shows an honest placeholder (`@npub1abc…` truncated npub) immediately on cold start, then upgrades in place to `@DisplayName` once a kind:0 profile event arrives. This reactive promise is fully implemented in master via `LiveProfileMap` in the gallery TUI. [^752b5-1]


On iOS, `MentionProfile` carries a `display` field for exactly this lookup. However, two author display locations ignore it: `NoteRowView.authorDisplayLabel` hardcodes `item.authorPubkey.shortHex` and `ModularBlockView.moduleRow` hardcodes `pubkey.shortHex`, instead of preferring `mentionProfiles[pubkey]?.display`. [^9a2c7-31]
## LiveProfileMap — Three-Projection Design

The canonical implementation lives in `apps/nmp-gallery/src/data.rs` (lines 138–173). `LiveProfileMap` ingests **all three** kernel projections:

- **`claimed_profiles`** — component-owned claims; this is the claim-driven path for inline-content mention resolution
- **`mention_profiles`** — timeline-row author profiles (top-level authors only, not inline mention pubkeys)
- **`author_view.profile`** — full profile card

The `resolve()` method (line 180) returns the kind:0 display name if available, and falls back to the truncated npub (line 184) when no kind:0 has arrived. The fallback is explicit and intentional — it is an honest representation of the unknown state, not an error. Profile resolution for mention profiles goes through `LiveProfileMap` at render time, not embedded upfront into render data. The `content_example` function takes only pubkeys (or URIs) for mention profiles, not `LiveProfile` structs with pre-resolved names.

<!-- citations: [^752b5-2] [^9b9db-1] -->
## Shell-Boundary Wiring

The profile host is wired once at the shell boundary via `profile_host_from_context` → `NostrMentionProfileHost` trait (present in `nostr_mention_chip.rs`). The `NostrMentionProfileHost` trait is the abstraction layer that mention chips use to query display names; the `LiveProfileMap` is its implementation in the gallery shell. Shells must not duplicate profile resolution logic — they implement the trait once and pass it through. [^752b5-3]

## Why mention_profiles Does Not Cover Inline Mentions

`mention_profiles_from_items` maps only `item.author_pubkey` — top-level timeline authors, not pubkeys mentioned inside note bodies. Inline-content mention resolution uses the separate `claimed_profiles` projection, which is keyed by all currently claimed pubkeys (claim-driven). Any architecture that routes inline mention resolution through `mention_profiles` alone will miss content-embedded pubkeys. [^752b5-4]

## Superseded Design

An earlier branch (`fix/reactive-profile-mentions`, base commit 344d7aa7) implemented this promise via `claimed_profiles` mirrored through `embed_host.rs`. That approach is fully superseded by `LiveProfileMap`. The branch was deleted once the overlap was confirmed. Any future branch attempting to re-introduce an `embed_host`-based profile resolution path must first verify that `LiveProfileMap` does not already cover it. [^752b5-5]

## See Also
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide
- [[chirp-ios-nmp-gallery-component-adoption|Chirp iOS NMP Gallery Component Adoption — Gap Audit and Implementation Plan]] — related guide
- [[chirp-ios-avatar-profile-lifecycle|Chirp iOS Avatar and Profile Lifecycle — NostrProfileHost Gap]] — related guide
- [[component-owned-reactivity-architecture|Component-Owned Reactivity Architecture]] — related guide
- [[resolved-profiles-kernel-projection|resolved_profiles — Kernel-Level Profile Merge Projection]] — related guide

