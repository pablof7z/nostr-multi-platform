---
title: Chirp Onboarding Gate
slug: chirp-onboarding-gate
summary: Chirp's onboarding gate uses model.hasActiveAccount (derived from activeAccount != nil) to switch RootShell between OnboardingView and the main tab interface.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-27
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:582fca30-be51-4861-bb16-3788610c6fb7
  - session:f22be978-ccc6-42dd-bad0-2b2d5aba2999
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
  - session:485a5310-d073-41c9-b230-e6e77926a143
---

# Chirp Onboarding Gate

## Onboarding Gate

Chirp's onboarding gate uses model.hasActiveAccount (derived from activeAccount != nil) to switch RootShell between OnboardingView and the main tab interface. Login completion depends on the kernel emitting an updated UpdateFrame with a non-nil activeAccount, which Swift decodes and uses to transition from OnboardingView to mainTabs via hasActiveAccount. When no accounts are configured, the welcome screen renders inline (no standalone module) showing the 'chirp' title, 'the nostr social client' subtitle, and key hints for n, c, and q. An onboarding state machine handles first launch: welcome → create/import/bunker/browse → relay picker → done, replacing the empty feed with a full-screen flow. The onboarding welcome screen shows a staggered fade and slide-up entrance animation using the previously-wired-but-unused `appeared` state. Every new account created on Chirp automatically follows npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft and fiatjaf's key as its initial contact list. A DEFAULT_FOLLOWS constant defines the two hex pubkeys for the default follows. A publish_initial_follows() helper builds a kind:3 contact list event and publishes it, called at the end of create_account().

<!-- citations: [^582fc-13] [^f22be-1] [^19e07-10] [^93c59-6] [^485a5-2] -->
## NIP-46 Signer Discovery

Chirp's onboarding detects installed NIP-46 signer apps on the phone using UIApplication.shared.canOpenURL with LSApplicationQueriesSchemes entries for nostrsigner, primal, and nostrconnect. The nostrsigner:// URL scheme is labeled 'Nostr Signer' (not 'Amber') since multiple NIP-46 apps register that scheme. [^582fc-14]

## Deep-Link Callback Handling

Chirp registers chirp:// as a deep-link callback scheme and handles incoming chirp://nip46?bunker_uri=... callbacks by routing the bunker_uri to model.signInBunker. [^582fc-15]

## Onboarding NIP-46 UI

Chirp's onboarding always displays a NIP-46 section in welcome mode with a collapsible QR code for the nostrconnect:// URI, an 'Open in [App]' button when a signer is detected, and a persistent bunker:// paste field. [^582fc-16]

## Local Signer Discovery Gap

Local signer discovery with wallet auto-linking (the Olas/Primal pattern using nostrnwc://) is a gap not designed in the framework docs and is specced as Chirp milestone CX3 with a new LinkDiscoveryCapability requiring an ADR. [^582fc-17]
## See Also

