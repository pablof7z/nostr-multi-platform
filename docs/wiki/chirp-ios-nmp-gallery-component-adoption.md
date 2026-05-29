---
title: Chirp iOS NMP Gallery Component Adoption — Gap Audit and Implementation Plan
slug: chirp-ios-nmp-gallery-component-adoption
summary: Chirp iOS is not using the registry components from nmp-gallery; a comprehensive 5-agent audit identified gaps across avatar/profile, content rendering, embed/kind registry, NostrProfileHost wiring, and user/social components.
tags:
  - ios
  - nmp-gallery
  - components
  - chirp
  - adoption
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

# Chirp iOS NMP Gallery Component Adoption — Gap Audit and Implementation Plan

> Chirp iOS is not using the registry components from nmp-gallery; a comprehensive 5-agent audit identified gaps across avatar/profile, content rendering, embed/kind registry, NostrProfileHost wiring, and user/social components.

## Root Cause: Name Display Bug

The `TimelineItem` struct has an `authorPubkey` field but no `authorName` field, even though the Rust projection emits `author_display_name`. This means the generated Swift struct is incomplete compared to what the kernel provides.

There is a disparity in how author names are accessed: `ChirpEventCard` has an `authorDisplayName` field that `ModularBlockView` uses via `displayName()`, but `NoteRowView` in standalone note rows does not use this path and instead hardcodes `shortHex`. The `displayName()` function in module rows already works correctly — it's the standalone row path that is broken. PR #789 addressed these name display fixes across the codebase, adding proper profile-backed name resolution.

<!-- citations: [^9a2c7-1] [^38935-1] [^38935-12] -->
## NostrProfileHost — Three Missing Pieces

`KernelModel` has `claimProfile`/`releaseProfile` implemented (`KernelBridge.swift:137,145` and `KernelModel.swift:329,332`) — the kernel side is ready. PR #789 completed the three missing pieces: `KernelModel` now conforms to `NostrProfileHost`, the `.environment(\.nostrProfileHost, model)` injection is wired at the app root (`ChirpApp.swift`), and `ChirpAvatar` now requires a `pubkey:` parameter for wiring the claim/release lifecycle. PR #794 subsequently fixed all 11 `ChirpAvatar` call sites that were left unpatched by #789, each with the contextually correct pubkey.

<!-- citations: [^9a2c7-2] [^38935-13] -->
## Author Display Locations — Both Ignore Profiles

Two author display locations ignore the `mentionProfiles` dictionary: (1) `NoteRowView.authorDisplayLabel` is hardcoded to `item.authorPubkey.shortHex`, (2) `ModularBlockView.moduleRow` is hardcoded to `pubkey.shortHex`. Both should prefer `mentionProfiles[pubkey]?.display`. The `MentionProfile` struct already carries a `display` field for exactly this lookup. [^9a2c7-3]

## Priority 0: NostrProfileHost Wiring

Wiring `NostrProfileHost` fixes the name display bug and unblocks all registry avatar/profile usage. The kernel-side FFI (`claimProfile`/`releaseProfile`) is already implemented. PR #789 completed this: `KernelModel: NostrProfileHost` conformance, `claimedProfiles` decoding from `SnapshotProjections`, `.environment(\.nostrProfileHost, model)` at `ChirpApp.swift` root. `ChirpAvatar` now requires a `pubkey:` parameter. The remaining lifecycle gap is `.task`/`.onDisappear` calling `claimProfile`/`releaseProfile` on the avatar itself — the pubkey is now available for this wiring.

<!-- citations: [^9a2c7-4] [^38935-14] -->
## Priority 1: Content Rendering Divergence

Chirp and nmp-gallery share the same `NostrContent/` component files but they have drifted. The root divergence is `NostrContentRenderer` — gallery added a centralized image cache (`NostrImageCache`: 32MB memory / 256MB disk) + pluggable `imageLoader` closure that flows through to every component. Chirp uses raw `AsyncImage` everywhere. Specific divergences: `NostrContentView` is missing video support (`AVKit.VideoPlayer`), emoji images, and ADR-0034 embed dispatch via `EmbedHost`/`nostrKindRegistry` environment. `NostrMentionChip` is missing `NostrImageView` and geometric identicon. `NostrQuoteCard` is missing `NostrImageView` for avatar + thumbnail. `NostrMediaGrid` is missing `NostrImageView` (uses raw `AsyncImage`). `ContentTreeWire` is missing geometric 5×5 grid identicon (`IdenticonGridView`) and has a `CodingKeys` snake_case vs camelCase mismatch. `NostrMinimalContentView` is entirely missing — needed for compact flow-layout rendering in previews/search. [^9a2c7-5]

## Priority 2: Embed and Kind Registry

Several embed/kind components are entirely absent from Chirp: `NostrKindRegistry` (extensible kind → renderer dispatch), `EmbedKindProjection` (typed Swift mirror of Rust embed variants: note/article/highlight/profile), `EmbedChromeContainer` (depth-graded left-accent chrome wrapper), `ArticleEmbed` (kind:30023 — hero image + title + summary), `HighlightEmbed` (kind:9802 — pull-quote styling, source footer). `EmbeddedEvent` and `NostrLoginBlock` exist partially: `NostrQuoteCard` exists but is presentation-only with no claim/release lifecycle, and signer detection is inline in `OnboardingView+NIP46` with no reusable extraction.

The embed system is NOT blocked on Rust. The C FFI symbols `nmp_app_claim_event` / `nmp_app_release_event` are already in `NmpCore.h`. The kernel already emits `claimed_events` in the snapshot — the gallery reads it successfully from the same `libnmp_app_chirp.a` that Chirp links against. `KernelTypes.generated.swift` is generated by a dev-time tool (not at build time), lives in git, and the codebase already has precedent for adding fields to it. The remaining work is pure Swift: (1) Add `ClaimedEventDto` + `claimedEvents` to `SnapshotProjections`, (2) Port `EmbedHost` adapted to read from typed Swift structs, (3) Add `claimEvent`/`releaseEvent` to `KernelBridge`, (4) Port `EventClaimSinkProtocol` + wire `KernelModel`, (5) Port `NostrKindRegistry`, `EmbeddedEvent`, `EmbedChromeContainer`, `ArticleEmbed`, `HighlightEmbed`, (6) Update `NostrContentView` to use the embed dispatch path, (7) Wire `ChirpApp`.

PR #795 completed the embed system implementation: 12 agents, 8 new files (EmbedKindProjection.swift, EmbedHost.swift, NostrKindRegistry.swift, EmbedChromeContainer.swift, EmbeddedEvent.swift, ArticleEmbed.swift, HighlightEmbed.swift), zero new FFI symbols. The implementation followed the 7-step plan, reading ClaimedEventDto from SnapshotProjections via typed Swift structs. Quoted events in the home feed now render with native SwiftUI: kind:1 notes as embedded note cards, kind:30023 articles with hero images, kind:9802 highlights with pull-quote styling. The earlier claim that this work was "blocked on Rust" was incorrect — the C FFI symbols were already present, and the work was pure Swift. [^38935-22]

<!-- citations: [^9a2c7-6] [^38935-2] -->
## Priority 3: Inline UI Components

Several components exist functionally inline in Chirp but should be extracted: `NostrUserCard` is inline in `ProfileView.swift:71-177`, `NostrNpubChip` is inline in `ProfileView.swift:153-166` (functionally identical to gallery), `NostrNip05Badge` is inline in `ProfileView.swift:142-151` (functionally identical), `NostrRelayList` is inline in `RelaySettingsView.swift:211-243` but missing connection-status dots. [^9a2c7-7]

## Implementation Order

Recommended implementation sequence: (1) Wire `NostrProfileHost` — fixes names, unblocks all registry avatar/profile usage (~4 files, ~50 LOC). (2) Sync `NostrContentRenderer` — image cache + loader injection; all other content components inherit it. (3) Sync diverged content components — pull video, emoji, geometric identicon from gallery into Chirp's copies. (4) Add `NostrMinimalContentView` — copy from gallery for search/preview surfaces. (5) Kind registry + embeds — `NostrKindRegistry` → `ArticleEmbed` → `HighlightEmbed` in sequence. (6) Extract P3 components — low urgency, purely a cleanup/reuse win.

PR #789 completed the bulk of this work (priorities 0–4): `NostrContentView`, `NostrContentRenderer`, `NostrMediaGrid`, `NostrMentionChip`, `NostrQuoteCard`, `NostrMinimalContentView`, `KernelModel` conformance to `NostrProfileHost`, `ChirpApp` env injection, and name display fixes everywhere. Priority 5 (kind registry + embeds) is the remaining gap. It is pure Swift work — not blocked on Rust. The 7-step implementation plan is: (1) Add `ClaimedEventDto` + `claimedEvents` to `SnapshotProjections` in `KernelTypes.generated.swift`, (2) Port `EmbedHost` adapted to read from typed Swift structs, (3) Add `claimEvent`/`releaseEvent` to `KernelBridge`, (4) Port `EventClaimSinkProtocol` + wire `KernelModel`, (5) Port `NostrKindRegistry`, `EmbeddedEvent`, `EmbedChromeContainer`, `ArticleEmbed`, `HighlightEmbed`, (6) Update `NostrContentView` to use the embed dispatch path, (7) Wire `ChirpApp`.

The embed plan was verified against all project doctrines: D8 (no polling) — consumes kernel-pushed snapshots reactively; ADR-0025 — uses the existing projection registry seam, mints zero new FFI symbols; One-way principle — one mechanism for projections; ADR-0037 — reads typed Swift structs from `SnapshotProjections`, not raw JSON; Component-owned reactivity — components signal their own data requirements via claim/release, kernel never pre-fetches. The C FFI symbols `nmp_app_claim_event` / `nmp_app_release_event` are already present in `NmpCore.h`.

Priority 5 (kind registry + embeds) was completed in PR #795. The 7-step plan was executed as a single PR with 12 haiku agents across 5 phases: Foundation types → Bridge + registry → Views → Wire → Ship (ChirpApp env injection + xcodegen + PR). The result: quoted events render with native SwiftUI instead of collapsed placeholders. Zero new FFI symbols were introduced. [^38935-23]

<!-- citations: [^9a2c7-8] [^38935-3] [^38935-6] -->
## Audit Method

The gap audit was performed by 5 parallel haiku agents each covering a different angle: (1) Avatar/profile name — `ChirpAvatar` vs `NostrAvatar`, name rendering across all note row call sites. (2) Content rendering — structural diff of every `NostrContent/*` component between Chirp and nmp-gallery. (3) Embed & kind registry — `EmbeddedEvent`, `ArticleEmbed`, `HighlightEmbed`, `NostrKindRegistry` gaps. (4) NostrProfileHost wiring — exact steps to wire `claimProfile`/`releaseProfile` end-to-end in Chirp. (5) User/social components — `NostrUserCard`, `NostrNpubChip`, `NostrNip05Badge`, `NostrRelayList` gaps. [^9a2c7-9]

## Gallery Components Already Shared

The `NostrContentView` and related content components already live in `Chirp/Components/NostrContent/` — these are the only gallery components currently shared. The avatar (`NostrAvatar`), profile name (`NostrProfileName`), and profile host (`NostrProfileHost`) components from nmp-gallery are not used by Chirp. [^9a2c7-10]


Implementation Order

The implementation is organized into three waves of parallel haiku agent tasks, each in isolated git worktrees, merged into the integration branch `ios/nmp-component-adoption` (worktree at `.claude/worktrees/nmp-integration`) after sonnet review. After ≥8 tasks merge, run `xcodegen generate` and open a PR to master. [^9a2c7-32]

Wave 1 (5 parallel, foundation, no dependencies): T01 creates `ProfileWire` + `NostrProfileHost` bridge files. T02 syncs `NostrContentRenderer` with image cache + `NostrImageView` + emoji. T03 fixes `ContentTreeWire` geometric identicon + CodingKeys mismatch. T04 adds `NostrMinimalContentView` as a new compact renderer. T05 creates `BACKLOG.md` with gallery extraction candidates. [^9a2c7-33]

Wave 2 (5 parallel, starts after Wave 1 is merged): T06 adds `KernelModel: NostrProfileHost` conformance + `claimedProfiles` decoding. T07 injects environment at `ChirpApp` root. T08 adds video + emoji rendering to `NostrContentView`. T09 adds `NostrImageView` + geometric identicon to `NostrMentionChip`. T10 adds `NostrImageView` to `NostrQuoteCard` + `NostrMediaGrid`. [^9a2c7-34]

Wave 3 (3 parallel, starts after Wave 2): T11 adds `claimProfile`/`releaseProfile` lifecycle to `ChirpAvatar`. T12 fixes name display in `NoteRowView`, `ProfileNoteRow`, and `ThreadNoteRow` via profileHost. T13 fixes name display in `ModularBlockView` via profileHost. [^9a2c7-35]

Root Cause: Name Display Bug

The `TimelineItem` struct has an `authorPubkey` field but no `authorName` field, even though the Rust projection emits `author_display_name`. This means the generated Swift struct is incomplete compared to what the kernel provides. [^9a2c7-36]

There is a disparity in how author names are accessed: `ChirpEventCard` has an `authorDisplayName` field that `ModularBlockView` uses via `displayName()`, but `NoteRowView` in standalone note rows does not use this path and instead hardcodes `shortHex`. The `displayName()` function in module rows already works correctly — it's the standalone row path that is broken. [^9a2c7-37]
## See Also
- [[gallery-vs-production-app-distinction|Gallery App Implementations Do Not Satisfy Production Backlog Items]] — related guide
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide
- [[chirp-ios-avatar-profile-lifecycle|Chirp iOS Avatar and Profile Lifecycle — NostrProfileHost Gap]] — related guide
- [[reactive-profile-mentions-architecture|Reactive Profile Mentions — LiveProfileMap Architecture]] — related guide
- [[op-centric-home-feed|OP-Centric Home Feed (V-80) — Architecture and Status]] — related guide
- [[chirp-ios-kernel-types-generated|Chirp iOS KernelTypes.generated.swift — Dev-Time Generation, Lives in Git]] — related guide
- [[chirp-ios-embed-system-implementation|Chirp iOS Embed System — Implementation and Architecture]] — related guide
- [[architectural-compliance-verification-gate|Architectural Compliance Verification Gate — Verify Before Implementing]] — related guide

