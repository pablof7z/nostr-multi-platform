---
title: resolved_profiles — Kernel-Level Profile Merge Projection
slug: resolved-profiles-kernel-projection
summary: resolved_profiles is a kernel-level snapshot projection that applies the canonical profile-merge precedence once in Rust, eliminating triplicated merge logic in Swift/Kotlin/Rust.
tags:
  - kernel
  - projection
  - profile
  - nmp-gallery
  - nmp-core
volatility: hot
confidence: medium
created: 2026-05-29
updated: 2026-05-25
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
  - session:7b4ae585-801c-441f-811d-5308e1002f08
  - session:45fcf96e-5b37-414f-a080-820b74a4e179
  - session:53838558-81bd-433d-a46d-d117ecebb361
---

# resolved_profiles — Kernel-Level Profile Merge Projection

> resolved_profiles is a kernel-level snapshot projection that applies the canonical profile-merge precedence once in Rust, eliminating triplicated merge logic in Swift/Kotlin/Rust.

## Overview

resolved_profiles is a kernel-level snapshot projection that applies the canonical profile-merge precedence (claimed_profiles → author_view → mention_profiles-if-absent) once in Rust, emitting a map of {pubkey: ProfileCard} that every platform consumes as a dumb decode with zero merge logic. It eliminates the triplicated profile-merge implementations currently hand-maintained in tui/src/data.rs (Rust), GalleryModel.swift (Swift), and GalleryModel.kt (Kotlin) — all with divergent precedence. Any derived state, business logic, or view-state derivation in Swift violates the repository rules and must be moved to the Rust kernel as pre-formatted snapshot projections; all violations identified in Chirp's Swift layer must be fixed by moving policy and derived logic into Rust kernel projections, not by extracting a shared Swift package.

After the kernel projection landed (PR #812), all gallery platforms (iOS #813, Android #815, TUI/Desktop #816) and Chirp production apps (iOS #817, Android #818) migrated to consume resolved_profiles. Every layer of the goal is now satisfied: the kernel applies the canonical merge once in projections.rs, nmp-app-gallery has no merge code or bespoke pull symbols, and all Chirp apps decode resolved_profiles as a single map with zero merge logic.

KernelSnapshot exposes a `profiles_by_pubkey` map so that Swift can render kind:0 profile information for mentioned pubkeys and p-tags, not just for timeline item authors.

The Profile struct includes `lud16`, `banner`, and `website` fields.

<!-- citations: [^6a951-9] [^6a951-31] [^7b4ae-9] [^7b4ae-10] [^45fcf-10] -->
## Design and Location

The projection lives directly in projections.rs inside snapshot_projections_with_publish_cluster — it is a kernel-owned projection like claimed_events, not an app-level projection registered via register_snapshot_projection. This is because register_snapshot_projection closures cannot reach kernel &self, and the merge needs access to the full kernel state (claimed_profiles, author_view, mention_profiles). It reuses the existing ProfileCard type and adds a from_mention constructor. [^6a951-10]


The projection lives in projections.rs inside snapshot_projections_with_publish_cluster — a kernel-owned projection, not an app-level projection registered via register_snapshot_projection. This is because register_snapshot_projection closures cannot reach kernel &self, and the merge needs access to the full kernel state (claimed_profiles, author_view, mention_profiles). It reuses the existing ProfileCard type and adds a from_mention constructor. The kernel-first, platform-second shipping order ensures the projection is stable and tested before any consumer depends on it. After the projection landed on master, platform migration PRs (TUI, iOS, Android, Desktop, and Chirp production apps) all ran in parallel. [^6a951-38]
## Canonical Merge Precedence

The three-step precedence, applied in order: (1) claimed_profiles — highest priority, applied first. (2) author_view.profile — applied second, only when has_profile is true. (3) mention_profiles — applied last with an only-if-absent guard (if !profiles.contains_key(pubkey)). This is the same precedence that was fixed in TUI in PR #787 task 5, but the kernel-level implementation corrects a latent TUI bug where apply_profile_card actually inverts precedence (author_view overwrites claimed). ProfileWire.npub and npubShort must always be Rust-formatted with no Swift/Kotlin-side reformatting, per aim.md §6.9.

<!-- citations: [^6a951-11] [^53838-16] -->
## Shipping Strategy

The projection ships additive-first: PR 1 lands the kernel projection in nmp-core with precedence tests mirroring event_claim_tests.rs. PR 1 is a pure addition — no consumers yet, no deletions. Once PR 1 lands on master, platform migration PRs (TUI, iOS, Android, Desktop) can run in parallel, each collapsing their native merge body into a single map decode from the resolved_profiles projection key. [^6a951-12]

## Implementation Status

PR #812 implemented the resolved_profiles kernel projection as a pure additive change to nmp-core. The implementation adds ProfileCard::from_mention to types.rs and the merge block to snapshot_projections_with_publish_cluster in projections.rs, with precedence tests mirroring event_claim_tests.rs. The projection applies the canonical three-step precedence (claimed_profiles → author_view → mention_profiles-if-absent) and emits resolved_profiles: {pubkey: ProfileCard}. PR #812 is merged to master.

After #812 landed, six platform migration PRs followed — three gallery and three Chirp production:
- #813 (iOS gallery): deletes Swift three-step merge in GalleryModel.swift → reads resolved_profiles
- #815 (Android gallery): deletes Kotlin three-step merge in GalleryModel.kt → reads resolved_profiles
- #816 (TUI gallery): deletes LiveProfileMap::update_from_snapshot merge body → reads resolved_profiles. Desktop benefits automatically since it imports from TUI crate.
- #817 (Chirp iOS): ModularBlockView.moduleRow routes through displayName(item:card:) instead of hardcoded shortHex. resolvedProfiles comes from codegen registry via FlatBuffers (Chirp uses FlatBuffers, not JSON, so resolved_profiles goes through swift_projections_registry.rs).
- #818 (Chirp Android): resolved_profiles decoded in both FlatBuffers and JSON paths. ProfileScreen and TimelineScreen merge chains deleted. CompositionLocal wiring updated (also fixes embedded notes in NostrRichText).
- Chirp TUI and Desktop: already correct — profiles come from claimed_events inline author cards, no separate merge needed.

Complete status: every app on every platform now reads resolved_profiles as a single map decode with zero merge logic. The kernel applies the merge once in snapshot_projections_with_publish_cluster. nmp-app-gallery has no business logic, no merge code, no bespoke pull symbols.

<!-- citations: [^6a951-13] [^6a951-19] [^6a951-30] [^6a951-78] -->
## See Also
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide
- [[claimed-events|claimed_events Snapshot Projection]] — related guide
- [[component-owned-reactivity-architecture|Component-Owned Reactivity Architecture]] — related guide
- [[adr-0025-bespoke-ffi-anti-pattern|ADR-0025 — Bespoke FFI Pull Symbols Are an Anti-Pattern; Use register_snapshot_projection]] — related guide
- [[reactive-profile-mentions-architecture|Reactive Profile Mentions — LiveProfileMap Architecture]] — related guide
- [[v-107-bespoke-snapshot-consumer-migration|V-107 — Live Bespoke Snapshot Consumer Migration to Canonical Seam]] — related guide
- [[opus-architect-workflow|Opus Architect Workflow — Plan, Validate, Execute, Audit]] — related guide
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide
- [[chirp-ios-nmp-gallery-component-adoption|Chirp iOS NMP Gallery Component Adoption — Gap Audit and Implementation Plan]] — related guide
- [[component-owned-reactivity-architecture|Component-Owned Reactivity Architecture]] — related guide
- [[kernel-never-fetches-kind0-from-event-ingest|Kernel Never Fetches kind:0 From Event Ingest — Profile Fetching Is Presentation-Layer]] — related guide

