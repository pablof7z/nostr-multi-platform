---
title: Chirp iOS Avatar and Profile Lifecycle — NostrProfileHost Gap
slug: chirp-ios-avatar-profile-lifecycle
summary: ChirpAvatar is static with no profile lifecycle; NostrAvatar from nmp-gallery uses NostrProfileHost with claimProfile/releaseProfile on appear/disappear to tell the kernel which profiles to fetch.
tags:
  - ios
  - avatar
  - profile
  - nostrprofilehost
  - chirp
volatility: hot
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
  - session:38935d82-0cbf-4e85-98d3-a0f056fd450c
---

# Chirp iOS Avatar and Profile Lifecycle — NostrProfileHost Gap

> ChirpAvatar is static with no profile lifecycle; NostrAvatar from nmp-gallery uses NostrProfileHost with claimProfile/releaseProfile on appear/disappear to tell the kernel which profiles to fetch.

## Profile Host System

nmp-gallery defines a `NostrProfileHost` protocol with `claimProfile`/`releaseProfile` methods that reactively tell the kernel which profiles to fetch. `NostrAvatar` from nmp-gallery uses this system: it calls `profileHost?.claimProfile(pubkey:)` on appear and `releaseProfile` on disappear. This is how the kernel knows which profiles are on screen and need kind:0 metadata fetched. [^9a2c7-26]

## ChirpAvatar — Current State

ChirpAvatar was originally a static avatar component with no lifecycle. It did not call claimProfile or releaseProfile. PR #789 updated ChirpAvatar.init to require a new first parameter pubkey: String for wiring the claim/release lifecycle. However, PR #789 left all 11 call sites unpatched, making the iOS project uncompilable — every avatar call produced a missing argument for parameter 'pubkey' in call error. PR #794 fixed all call sites — each with the contextually correct pubkey (item.authorPubkey, conversation.peerPubkey, message.senderPubkeyHex, etc.). The pubkey: parameter is now wired on every ChirpAvatar call. The .task { profileHost?.claimProfile(pubkey:) } and .onDisappear { profileHost?.releaseProfile(pubkey:) } lifecycle hooks are the remaining gap to fully close the profile fetching loop.

Even after PR #789 (pubkey: wiring), PR #794 (build fix), and PR #795 (embed system), ChirpAvatar still uses raw AsyncImage for image loading instead of NostrImageCache (32MB memory / 256MB disk cache from nmp-gallery). Images load correctly but without the caching layer that gallery components use. This is a known remaining gap — functional but suboptimal. [^38935-34]

<!-- citations: [^9a2c7-27] [^38935-4] [^38935-24] -->
## KernelModel Readiness

KernelModel already has claimProfile/releaseProfile implemented (KernelBridge.swift:137,145 and KernelModel.swift:329,332). These call the same C FFI symbols as nmp-gallery. The kernel side is fully ready — the gap is only in the Swift UI layer. PR #789 inadvertently introduced a duplicate NostrProfileHost protocol declaration: NostrProfileHost.swift was added as a new file containing the protocol and EnvironmentValues extension, but the same definitions were not removed from ProfileWire.swift. PR #794 removed the duplicate from ProfileWire.swift, resolving the invalid redeclaration compile error. Both regressions in PR #789 (duplicate protocol and missing pubkey: arguments) match the known red CI merges to master pattern.

<!-- citations: [^9a2c7-28] [^38935-5] [^38935-29] [^38935-32] -->
## Required Fix

To wire NostrProfileHost: (1) Make `KernelModel` conform to `NostrProfileHost`, (2) Add `profile(forPubkey:)` method reading from `claimed_profiles` projection, (3) Inject `.environment(\.nostrProfileHost, model)` at the `ChirpApp.swift` root, (4) Add `.task { profileHost?.claimProfile(pubkey:) }` and `.onDisappear { profileHost?.releaseProfile(pubkey:) }` to `ChirpAvatar`. [^9a2c7-29]


ChirpAvatar — Current State

The name display bug manifests in two specific locations: `NoteRowView.authorDisplayLabel` (line 42) hardcodes `item.authorPubkey.shortHex`, and `ModularBlockView.moduleRow` (line 141) hardcodes `pubkey.shortHex`. Both ignore the `mentionProfiles` dictionary that is passed to cover all home-timeline authors (V-31). The `MentionProfile` struct already carries a `display` field for this lookup. In contrast, `ModularBlockView`'s `displayName()` function for module rows correctly uses `card.authorDisplayName` from `ChirpEventCard` — so the modular block path already works and the standalone note row path is the one broken. [^9a2c7-42]
## See Also
- [[chirp-ios-nmp-gallery-component-adoption|Chirp iOS NMP Gallery Component Adoption — Gap Audit and Implementation Plan]] — related guide
- [[reactive-profile-mentions-architecture|Reactive Profile Mentions — LiveProfileMap Architecture]] — related guide
- [[red-ci-merges-to-master|Red CI Merges to Master — Pattern and Prevention]] — related guide

