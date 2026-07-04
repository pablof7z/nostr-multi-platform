---
title: "Chirp UI: Navigation, Social Bar, and Zap Removal"
slug: chirp-ui
topic: ui-components
summary: The Android nav bar has 5 tabs plus a More screen instead of 8 cramped tabs with mid-word wrapping.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# Chirp UI: Navigation, Social Bar, and Zap Removal

## Android Navigation

The Android nav bar has 5 tabs plus a More screen instead of 8 cramped tabs with mid-word wrapping.

<!-- citations: [^dcc80-1cb4d] [^dcc80-7e62d] -->
## Social Bar

The social bar shows reply, reaction, and repost counts from live relay data with no Zap button on both iOS and Android. On Android, the social bar row displays the format `Reply N · React N · Repost N`.

<!-- citations: [^dcc80-e3fc6] [^dcc80-8c479] [^dcc80-56e1a] [^dcc80-2111a] -->
## Zap Removal

Zap is entirely removed (not stubbed) from Chirp. On iOS, `ZapAmountSheet.swift` and its tests were deleted. On Android, the `zapNote()` dispatch in `SocialActions` fails closed with a clear 'unavailable' message and log line rather than referencing a builder that no longer exists. All `onZap`/`PendingZap`/`zapNote` call sites were removed through `HomeFeedView`/`ModularBlockView`/`NoteRowView`/`NoteActions`. Dead `nip57_zap_*` error-toast mappings and the permanently-nil `NoteRelationCounts`/`RelationCount` decode path were removed on both platforms.

<!-- citations: [^dcc80-67ebe] [^dcc80-66957] [^dcc80-0e2d2] [^dcc80-5eb19] [^dcc80-38fdd] -->
## iOS Signer-Relay Health

The iOS signer-relay health section in `AccountsView.swift` only renders when the active account's `signerIsRemote` field is true (gated on `AccountSummary.signer_is_remote`), not merely on `model.signerState != nil`. <!-- [^dcc80-4b0fb] -->

## iOS Content Rendering

iOS content rendering requires tappable links, hashtag chips (not plain text), resolved quote cards (not raw stubs), resolved NIP-23 article cards (not raw URLs), and accessibility identifiers for VoiceOver navigation.

<!-- citations: [^dcc80-1c6c9] [^dcc80-123ce] -->

## iOS Pull-to-Refresh

Pull-to-refresh on the iOS feed must show a visual refresh indicator throughout the pull→fetch cycle. <!-- [^dcc80-43711] -->

## iOS Search

The iOS top-right toolbar includes a search button providing full-text NIP-50 search and entity navigation: `nevent` resolves to the thread, `npub` resolves to the profile, and NIP-AD entities are resolved. <!-- [^dcc80-58d97] -->
