---
title: Profile Name Flicker — Warm-Reclaim Lifecycle Gap
slug: profile-flicker-warm-reclaim-gap
summary: Names flicker to hex pubkeys during navigation because releaseProfile/re-claimProfile creates a 1–2 tick churn gap even for already-resident profiles that require zero relay REQ.
tags:
  - ios
  - profile
  - lifecycle
  - performance
  - reactivity
volatility: hot
confidence: medium
created: 2026-05-30
updated: 2026-05-25
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:7b4ae585-801c-441f-811d-5308e1002f08
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
  - session:53838558-81bd-433d-a46d-d117ecebb361
---

# Profile Name Flicker — Warm-Reclaim Lifecycle Gap

> Names flicker to hex pubkeys during navigation because releaseProfile/re-claimProfile creates a 1–2 tick churn gap even for already-resident profiles that require zero relay REQ.

## Root Cause

Profile display name flicker is caused by a claim-lifecycle churn gap (250–500ms) where navigating away releases the profile claim and navigating back re-claims it, creating a gap before the kernel re-emits the name. When the user navigates away from a view, ChirpAvatar.onDisappear calls releaseProfile, removing the pubkey from profile_claims. The next snapshot tick drops it from claimed_profiles. When the user navigates back, .task(id: pubkey) calls claimProfile, the kernel re-reads from its resident store with zero new REQ, and the next tick (≤250ms at 4 Hz) re-emits the name. During that 1–2 tick gap, NoteRowView.authorDisplayLabel falls through the chain: claimedProfiles → mentionProfiles → pubkey.shortHex, producing the visible flicker from display name to hex and back. [^4edd4-47]

<!-- citations: [^4edd4-47] [^4edd4-231] -->
## Warm-Reclaim Guarantee

PR #821 confirmed via Rust unit tests that claim_profile short-circuits with zero relay REQ for already-resident pubkeys. The kernel's resident store retains all previously fetched kind:0 metadata. When a profile is released and then re-claimed, the kernel satisfies the re-claim from its resident store without issuing any new REQ to relays. The 1–2 tick gap (250–500ms at 4Hz) is the time between the claim_profile call and the next snapshot tick — there is no network latency involved for cached profiles. For context, a real-world round-trip kind:0 profile fetch via purplepag.es takes approximately 750ms with no signed-in user, making the warm-reclaim path's resident-store short-circuit essential for avoiding that latency on re-claim. When an indexer relay closes, requested-but-unfulfilled profile pubkeys are moved back to pending so they are re-batched on reconnect.

<!-- citations: [^4edd4-48] [^7b4ae-7] [^19e07-16] [^53838-14] -->
## Impact on User Experience

The user sees: display name → scroll away → scroll back → hex pubkey for a fraction of a second → display name returns. This happens every time a view containing profile avatars/names is navigated away from and back to. The behavior is most visible on the home feed timeline where many author names are rendered, and particularly noticeable when rapidly switching between tabs. [^4edd4-49]

## Why the Gap Exists

The kernel emits snapshots at a 4 Hz tick rate (≤250ms between ticks). The claim_profile call and the next tick are not synchronized — the claim is registered immediately in the kernel's internal state, but the snapshot projection that carries claimed_profiles is only rebuilt on the next tick. The SwiftUI view re-renders on snapshot arrival, so there is always a 0–2 tick window where the new claim has not yet been reflected in the emitted snapshot. The fix is not to increase the tick rate, but to ensure the SwiftUI layer does not degrade to shortHex during that window for profiles it has already resolved. [^4edd4-50]

## Affected Code Paths

The flicker originates from two specific hardcoded shortHex fallbacks: NoteRowView.authorDisplayLabel (line 42) hardcodes item.authorPubkey.shortHex, and ModularBlockView.moduleRow (line 141) hardcodes pubkey.shortHex. Both ignore the mentionProfiles dictionary that carries display names for all home-timeline authors. The ModularBlockView path for module rows correctly uses card.authorDisplayName from ChirpEventCard, so the modular block path does not flicker — only the standalone note row path is affected. [^4edd4-51]

## Files Central to the Fix

crates/nmp-core/src/kernel/profile_claim_tests.rs — warm-reclaim Rust tests (landed in PR #821). crates/nmp-core/src/kernel/update/projections.rs:248 — where claimed_profiles is built from the resident store. ios/Chirp/Chirp/Bridge/KernelModel.swift:353 — the profile(forPubkey:) fallback chain. crates/nmp-cli/registry/swiftui/user-avatar/NostrAvatar.swift — the claim lifecycle that is the churn source. ios/Chirp/ChirpUITests/ChirpUITests.swift — UI tests for nav round-trip name persistence. [^4edd4-52]


## Structural Fix — PR #823

PR #823 provides the structural fix: author_display_name is now baked directly into the Rust TimelineItem at snapshot-build time from the resident profile cache. This field is exported via codegen to Swift as item.authorDisplayName. NoteRowView.authorDisplayLabel and ThreadNoteRow use it as a claim-independent fallback, making the flicker structurally impossible — the name is present in the snapshot data regardless of claim lifecycle state. The field is populated from the same resident cache that claim_profile reads with zero relay REQ for already-resident pubkeys. [^4edd4-97]
## See Also
- [[chirp-ios-avatar-profile-lifecycle|Chirp iOS Avatar and Profile Lifecycle — NostrProfileHost Gap]] — related guide
- [[component-owned-reactivity-architecture|Component-Owned Reactivity Architecture]] — related guide
- [[warm-reclaim-rust-tests-pr-821|PR #821 — Warm-Reclaim Rust Unit Tests]] — related guide
- [[profile-display-name-fallback-chain|Profile Display Name Fallback Chain — Resolution Priority]] — related guide
- [[chirp-ios-reliability-metrics-testing-plan|Chirp iOS Reliability Metrics and Testing Plan]] — related guide

