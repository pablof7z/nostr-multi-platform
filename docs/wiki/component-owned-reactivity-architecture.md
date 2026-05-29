---
title: Component-Owned Reactivity Architecture
slug: component-owned-reactivity-architecture
summary: Every UI component is solely responsible for signaling its own data requirements via claim_profile/claim_event calls; the kernel fetches only in response to those claims, never proactively.
tags:
  - architecture
  - reactivity
  - gallery
  - doctrine
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
  - session:38935d82-0cbf-4e85-98d3-a0f056fd450c
---

# Component-Owned Reactivity Architecture

> Every UI component is solely responsible for signaling its own data requirements via claim_profile/claim_event calls; the kernel fetches only in response to those claims, never proactively.

## Core Principle

Every component is fully responsible for signaling its own data requirements — claim_profile(pubkey), claim_event(uri), etc. — when it mounts. The kernel fetches in response to those claims, never proactively. The gallery shell has no right to pre-warm anything. No component, shell, or host is permitted to pre-fetch kind:0, kind:30023, or any other Nostr events on behalf of a component before the component signals its own requirements. [^6a951-46]

## Registry vs. Live Data Distinction

The registry metadata (component IDs, labels, descriptions, section groupings, and variant specs) is pulled once statically at startup via nmp_app_gallery_registry_json(). This is pure config — it has nothing to do with data fetching. Live data (profiles, events, relay status) remains entirely component-driven: each component calls claim_profile or claim_event when it mounts, and those claims drive all kernel fetching. [^6a951-47]

## DataContract as Wiring Hint, Not Data Fetcher

The DataContract field in ComponentSpec (e.g., Profile { pubkey_ref: 'showcase.profile' }) is a hint to the shell about which showcase reference to pass into the component when rendering it. It tells the shell to wire showcase.profile.pubkey_hex into NostrAvatar, but it does NOT trigger any fetch. The component's own claim_profile call is what drives the fetch. The DataContract is a rendering hint, not a data pre-fetch directive. [^6a951-48]

## Claim/Release Lifecycle

Components claim data when they mount (appear) and release it when they unmount (disappear). On iOS this is implemented via .task / .onDisappear calling claimProfile/releaseProfile. On Android this uses DisposableEffect for the claim/release lifecycle. The kernel tracks which pubkeys and events are currently claimed by any mounted component and only fetches data for active claims. No component means no claim means no fetch. [^6a951-49]


The embed system in Chirp iOS (PR #795) implements this lifecycle for events: EmbeddedEvent calls claimEvent when it appears and releaseEvent when it disappears. EmbedHost reads claimedEvents from the kernel-pushed SnapshotProjections. The kernel fetches embedded events (kind:1, kind:30023, kind:9802) only when they are claimed by a mounted component, never proactively. The claim/release C FFI symbols (nmp_app_claim_event / nmp_app_release_event) were already present in NmpCore.h before the Swift implementation. [^38935-30]
## Why Pre-Warming Is Forbidden

Pre-warming (fetching data before a component signals its requirement) violates two project doctrines: (1) it breaks component-owned reactivity by moving the fetch decision to the shell, and (2) it wastes network and compute resources fetching data that may never be displayed. The gallery shell is a showcase — it demonstrates components behaving correctly. If the shell pre-warms data, it demonstrates the shell's behavior, not the component's. [^6a951-50]


Desktop nmp-gallery has a specific pre-warming violation at gallery.rs:167-173: it claims all profiles on every snapshot tick regardless of which component is currently visible. This means the shell, not the component, drives data fetching — the exact violation of component-owned reactivity. The fix is to move claims into each component's render path, matching the TUI sink pattern where components claim their own data when they mount. The Opus audit identified this as one of the three highest-leverage remaining tasks after PR #787. [^6a951-4]
## See Also
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide
- [[reactive-profile-mentions-architecture|Reactive Profile Mentions — LiveProfileMap Architecture]] — related guide
- [[adr-0025-bespoke-ffi-anti-pattern|ADR-0025 — Bespoke FFI Pull Symbols Are an Anti-Pattern; Use register_snapshot_projection]] — related guide
- [[opus-architect-workflow|Opus Architect Workflow — Plan, Validate, Execute, Audit]] — related guide
- [[chirp-ios-embed-system-implementation|Chirp iOS Embed System — Implementation and Architecture]] — related guide
- [[architectural-compliance-verification-gate|Architectural Compliance Verification Gate — Verify Before Implementing]] — related guide

