---
title: Account Creation & Autofollow
slug: account-creation-autofollow
summary: Local account creation, seed contacts, and autofollow behavior across all platforms
tags:
  - account-creation
  - autofollow
  - identity
  - contacts
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:855be2a2-4866-4d8d-ad4f-145309da56bc
---

# Account Creation & Autofollow

> Local account creation, seed contacts, and autofollow behavior across all platforms

## DEFAULT_FOLLOWS

When creating a new local account, the system automatically follows two seed contacts: the user's own pubkey (fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52 — npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft) and fiatjaf (3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d). These are hardcoded in identity.rs as the DEFAULT_FOLLOWS array. [^855be-1]

## Cross-platform consistency

All three platforms (iOS, TUI, Android) funnel through the same nmp_app_create_new_account C-ABI symbol, which calls create_account() in identity.rs. That function calls kernel.prepopulate_seed_contacts() with DEFAULT_FOLLOWS. Every platform gets the same seed follows on fresh account creation. [^855be-2]

## Account creation flow

Android triggers account creation via KernelBridge.nativeCreateLocalAccount → nmp_app_create_new_account (nmp-ffi) → ActorCommand::CreateAccount → create_account() in identity.rs. This publishes kind:0 (profile), kind:3 (contacts), and kind:10002 (relay list) events to the bootstrap relay (relay.primal.net). These published events are echoed back by the relay. [^855be-3]


On Android, when no active account exists, the Timeline screen shows the message "cannot open timeline: no active account — sign in first" with a "Create local account" button. Tapping this button triggers the account creation flow. [^855be-22]
## Fresh vs persisted accounts

A fresh account re-publishes kind:3 and kind:10002 events on creation, which the relay echoes back. A persisted account on TUI/iOS does not re-publish these events on each launch, so the relay echo behavior only surfaces during fresh account testing. [^855be-4]

## See Also
- [[nofs-op-centric-feed|NOFS OP-Centric Feed Engine]] — related guide
- [[cross-platform-architecture|Cross-Platform Architecture]] — related guide

