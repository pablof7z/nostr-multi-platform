---
title: Chirp App Overview & Feature Spec
slug: chirp-app
summary: The Chirp app is named 'Chirp' and serves as a reference Nostr implementation targeting feature parity with Amethyst and Primal.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:582fca30-be51-4861-bb16-3788610c6fb7
  - session:e2d58641-a6c3-4f43-94c0-b018c8fbb893
  - session:423f3c56-7275-4e62-998e-e8f37be564da
  - session:d27a4f61-511b-4086-845d-335493f9b464
  - session:cc7dc68a-1fcd-49fe-98be-198f17b6d59e
  - session:fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53
  - session:12b3f443-3c2d-4e47-976a-7f4ceab75343
  - session:156aa64b-42e1-4d3b-96ce-25b31fc06fec
  - session:594b7c34-efd1-4461-81ad-9fa33a6e76f9
  - session:3a906f87-ee2b-4d3a-9d5f-e82ccab29349
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:16ca6097-5734-4d49-8b9a-0a20d42a324b
  - session:38935d82-0cbf-4e85-98d3-a0f056fd450c
---

# Chirp App Overview & Feature Spec

## Overview

The Chirp app is named 'Chirp' and serves as a reference Nostr implementation targeting feature parity with Amethyst and Primal. Chirp retrieves all Nostr data exclusively through nmp; there is zero Swift-side networking. The Chirp data path is Rust nmp-core actor → C FFI → KernelHandle → KernelModel → SwiftUI views. Chirp iOS is a pure renderer with zero display formatting logic in Swift — all display strings (relative time, avatar initials, color hex, abbreviated pubkeys/IDs, formatted numbers) live in Rust and surface through snapshot payloads. Once PR #794 merges, Chirp consistently uses nmp components for content rendering, profile display, avatars with claim/release lifecycle, and name resolution. KeyringResult must conform to Codable (not just Encodable) to support the retrieveSecret decode path in ChirpCapabilities. Chirp defaults to using wss://purplepag.es as the indexer relay and wss://r.f7z.io as the app relay. Chirp provides a settings interface where the user can configure the relays they want, including kind 10050 relay lists. ChirpHandle::snapshot() calls self.engine.snapshot(...) and returns ChirpTimelineSnapshot.

<!-- citations: [^3a906-1] [^582fc-1] [^934-936] [^582fc-2] [^582fc-3] [^d27a4-2] [^cc7dc-1] [^fd809-1] [^12b3f-2] [^156aa-2] [^38935-1] -->
## Milestones

Chirp v1 aligns with the existing NMP ladder milestones M0–M17 without re-sequencing, and adds Chirp-specific milestones CX1–CX5 for features deferred past v1. Migrating iOS and Android Chirp apps from the repo root (ios/, android/) into the apps/chirp/ tree is a priority backlog item. Chirp Android must have iOS feature parity with all 11+ screens: Onboarding, HomeFeed, Profile, Thread, Search, Compose, Notifications, Wallet, Settings, Accounts, and Diagnostics. Chirp ThreadScreen migration and podcast comment surface migration are out of scope for the threading PRs and are designated as M2 follow-ups. No chirp-web features requiring persistence across reloads may be added until IndexedDB lands. V-86 CI coverage is insufficient: the CI glob only checks the gallery app, while the main Android Chirp app has 21 FlatBuffers files that remain completely unchecked.

<!-- citations: [^582fc-4] [^e2d58-5] [^423f3-1] [^594b7-1] [^86] [^42908-3] [^16ca6-2] -->
## See Also

