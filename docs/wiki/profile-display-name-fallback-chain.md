---
title: Profile Display Name Fallback Chain — Resolution Priority
slug: profile-display-name-fallback-chain
summary: "The 4-step fallback chain for profile display names: claimedProfiles → mentionProfiles → eventCards.authorDisplayName → pubkey.shortHex."
tags:
  - ios
  - profile
  - display-name
  - fallback
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-28
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:eb342a0d-84e3-4289-9873-88a947ca8144
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
  - session:45fcf96e-5b37-414f-a080-820b74a4e179
  - session:161ad3af-aeba-42f7-98ab-a71d2fda69a7
  - session:63dfcbb3-3ae0-48bb-9228-a494f85df203
  - session:d98be997-81df-4738-8846-8323d40ab9ff
---

# Profile Display Name Fallback Chain — Resolution Priority

> The 4-step fallback chain for profile display names: claimedProfiles → mentionProfiles → eventCards.authorDisplayName → pubkey.shortHex.

## Fallback Chain Priority

The profile display name for any pubkey is resolved through a strict fallback chain: (1) claimedProfiles — highest priority, the kernel's claimed_profiles projection carrying kind:0 metadata for all currently claimed pubkeys; (2) mentionProfiles — the mention_profiles projection carrying author metadata for all timeline-row authors; (3) eventCards.authorDisplayName — the NOFS event card's inline author display name, serving as a gap-filler when neither projection has the profile; (4) pubkey.shortHex — last resort, the truncated hex pubkey (e.g., 'npub1abc…'). When a profile has not yet been resolved, the mention chip must fall back to a short pubkey display (truncated to first 8 and last 4 characters) rather than showing a hardcoded name. Each step is tried in order; the first non-nil value wins. This chain is implemented in KernelModel.profile(forPubkey:) at line 353 and consumed by NoteRowView.authorDisplayLabel.

After PR #823, the fallback chain is consolidated into the resolveAuthorLabel helper in NoteRowView.swift: resolveAuthorLabel(claimedProfiles:mentionProfiles:itemAuthorName:) with itemAuthorName defaulting to nil. The 4-step chain is now: claimedProfiles → mentionProfiles → itemAuthorName (populated from TimelineItem.authorDisplayName in the FlatBuffers snapshot) → pubkey.shortHex. With the default nil, existing call sites skip the itemAuthorName rung, degrading to a 3-step chain.

The mention_profiles projection, initially scoped to ProfileView's open author-view items, should be adopted by HomeFeedView and ThreadScreen to eliminate their identical Dictionary rebuilds.

The Chats tab displays conversation rows using the peer's profile display name (never raw hex), falling back to a short npub format, and shows a profile picture or name initials as the avatar.

Currently, profile_value() emits a null display_name rather than falling back to the hex pubkey string via display_label(). The fallback chain must ensure that display_label() is consulted so that a null display_name does not result in a missing label when shortHex is available.

<!-- citations: [^4edd4-69] [^4edd4-98] [^eb342-9] [^45fcf-9] [^63dfc-3] [^d98be-7] -->
## Hardcoded shortHex Violations

Two author display locations bypass the fallback chain and hardcode shortHex directly: NoteRowView.authorDisplayLabel (line 42) uses item.authorPubkey.shortHex, and ModularBlockView.moduleRow (line 141) uses pubkey.shortHex. Neither consults mentionProfiles, which is passed to cover all home-timeline authors. The MentionProfile struct carries a display field specifically for this lookup. In contrast, ModularBlockView's displayName() function for module rows correctly uses card.authorDisplayName — so the modular block path already works correctly and only the standalone note row path is broken. [^4edd4-70]

## NOFS eventCards Gap-Filler

The eventCards.authorDisplayName field from the NOFS typed decoder is a load-bearing gap-filler. When both claimedProfiles and mentionProfiles are empty for a given pubkey (e.g., on a cold start before any kind:0 events arrive), the authorDisplayName from the event card is the last human-readable fallback before shortHex. The card author field reads from `author_pubkey` with a fallback to `pubkey` to handle both TimelineEventCard objects and raw events. This field is populated from the note's author data embedded in the event itself, not from a separate kind:0 fetch. Tier 1 unit tests must document this gap-filler as load-bearing: noteRow_authorDisplayLabel_priority verifies that when claimedProfiles and mentionProfiles are both empty, the NOFS authorDisplayName is used rather than shortHex.

ProfileCard has a hasProfile boolean field that gates display of about and nip05, preventing debug text like 'Waiting for selected author kind:0' from appearing as bio.

<!-- citations: [^4edd4-71] [^19e07-15] [^161ad-1] -->
## KernelModel.profile(forPubkey:) Location

The fallback chain lives at KernelModel.swift line 353. It is the single point of truth for profile display name resolution on iOS. All views that display author names must route through this method rather than accessing projections or pubkey directly. The method signature is profile(forPubkey:) → String? and returns nil only when no profile data is available at any layer of the chain. [^4edd4-72]

## See Also
- [[profile-flicker-warm-reclaim-gap|Profile Name Flicker — Warm-Reclaim Lifecycle Gap]] — related guide

