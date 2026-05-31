---
title: Self-Claiming NMP Components — Components Own Their Data Claims, Apps Compose Them
slug: self-claiming-nmp-components
summary: "Every atomic NMP component (NostrAvatar, NostrProfileName, embed renderers) owns its data-claiming lifecycle; apps are composed of self-claiming primitives and the kernel never fetches kind:0 because of an event ingest."
tags:
  - architecture
  - components
  - reactivity
  - kernel
  - doctrine
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-28
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:c8c2902c-43a6-4b1c-8215-1732dc266895
  - session:63dfcbb3-3ae0-48bb-9228-a494f85df203
  - session:8bd548b9-af6d-4108-bc64-16ebf8dfa4f7
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:6e8af009-f065-464a-98f1-3ec1ee4ed933
---

# Self-Claiming NMP Components — Components Own Their Data Claims, Apps Compose Them

> Every atomic NMP component (NostrAvatar, NostrProfileName, embed renderers) owns its data-claiming lifecycle; apps are composed of self-claiming primitives and the kernel never fetches kind:0 because of an event ingest.

## Core Principle

Every atomic NMP component is self-claiming: it declares and owns its data requirements (claim_profile, claim_event) when it mounts. Apps — including Chirp — are composed of these self-claiming primitives. NostrAvatar(pubkey:) already self-claims. NostrProfileName must too. Embed renderers, timeline rows — everything — composes these self-claiming primitives. The kernel never fetches kind:0 off an event ingest; fetching kind:0 is always the presentation layer's decision ('I need to render this user'). UI components render best-effort data immediately (e.g. identicon + truncated npub for profiles) and update reactively when kind:0 arrives — never showing a loading spinner. [^6a951-57]

All fake data must be removed from both platforms' content components, including hardcoded names like 'jack' and 'satoshi', fake URIs like 'nostr:npub1example', and fake pubkeys like 'deadbeefcafebabedeadbeefcafebabe'. Android's demoMentionTree must use the real npub URI 'nostr:npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft' and DEMO_PUBKEY instead of synthetic example values. iOS SampleContent.richTree must use the real DEMO_PUBKEY_HEX and real npub URI instead of fake 'nostr:npub1example' and 'deadbeef' pubkeys. [^63dfc-5]

NMP owns protocol and projection contracts (ContentTreeWire, claim/release sinks); apps own copied source, styling, and a single shell adapter. [^54ae9-14]

<!-- citations: [^6a951-57] [^53838-17] -->
## Kernel Must Never Fetch kind:0 From Event Ingest

No event ingest path may ever trigger the kernel to fetch an author's kind:0 profile. This is a hard architectural rule. The pre-existing request_profile_for_rendered_note at ingest/timeline.rs:172 that fetches author kind:0 on event ingest is a violation that must be removed — but only after all platforms' components have migrated to self-claim. Removing it earlier would break author display names before the components pick up the slack. The kernel also must not enrich claimed_events with author_display_name/author_picture_url — those fields are stripped from ClaimedEventDto once the migration is complete. [^6a951-58]

## Refactoring Order — Components Self-Claim First, Kernel Cleanup Last

The three-phase ordering ensures nothing half-breaks: (1) Components self-claim — iOS first, then Android/TUI/Desktop registry components. Each atomic component (NostrProfileName, embed renderers) gains its own claim lifecycle, composing NostrAvatar(pubkey:) and NostrProfileName(pubkey:) for author bylines instead of reading static authorDisplayName strings. (2) Apps compose them — gallery embed renderers and Chirp's NoteRowView/timeline/thread on each platform stop passing static names and instead compose NostrAvatar/NostrProfileName. (3) Kernel removal, last — delete request_profile_for_rendered_note (ingest/timeline.rs:172) and strip author_display_name/author_picture_url from ClaimedEventDto plus the claimed_events enrichment. This goes last because removing it before the components self-claim would break author names mid-flight. [^6a951-59]

## What Self-Claiming Replaces

Before self-claiming, embed renderers read static authorDisplayName strings from ClaimedEventDto — the kernel enriched the event payload with author profile data during ingest. This was the wrong architecture: it meant the kernel was fetching kind:0 because an event was ingested, violating the principle that profile fetching is a presentation-layer decision. After self-claiming, embed renderers compose NostrAvatar(pubkey:) and NostrProfileName(pubkey:) for author bylines. These atomic components call claim_profile when they mount, and the kernel responds to those claims. The author name resolves through the same resolved_profiles projection as everywhere else, with zero special enrichment in the event ingest path. The nmp-gallery app must not manually extract profile fields from snapshots to mutate app data, as that pattern forces reactivity boilerplate into every app.

Before self-claiming was fully wired, there was an intermediate state where EmbedHost.swift ignored authorDisplayName from ClaimedEventDto. The kernel already emitted author_display_name in the JSON projection at projections.rs:278-285, and EmbedKindProjection subtypes (ArticleProjection, NoteProjection, HighlightProjection) already accepted authorDisplayName/authorPictureUrl in their initializers — but the DTO decode in EmbedHost.swift's envelope() function was not forwarding these fields. This was fixed by plumbing the fields through the decode step, so embed author bylines show resolved display names (PABLOF7z, Gigi) instead of hex pubkeys.

On Android, the gallery app reads profile data from projections.author_view.profile (populated by nmp_app_open_author), not from claim_profile or snapshot.profiles.

<!-- citations: [^6a951-60] [^6a951-100] [^c8c29-4] [^8bd54-5] -->
## Implementation Status

iOS foundation (PR #833): NostrProfileName(pubkey:) becomes self-claiming, mirroring NostrAvatar which already does this. Embed renderers (DefaultShortNoteRenderer, DefaultArticleRenderer, ArticleEmbed, HighlightEmbed) compose NostrAvatar(pubkey:) + NostrProfileName(pubkey:) for author bylines instead of reading static authorDisplayName strings. No kernel changes. Merged to master.

Desktop renderers (PR #837): Self-claiming author bylines in gallery renderers. Also removed the central pre-warming violation where the desktop gallery claimed all profiles on every snapshot tick. Merged to master.

Android renderers (PR #839): Self-claiming author bylines mirroring iOS #833. The embed-article component claims the article author's kind:0 via DisposableEffect so the byline resolves to the display name instead of hex. Merged to master.

TUI renderers (PR #838): Self-claiming author bylines in gallery renderers. Merged to master.

Iced renderers: Iced components (UserName, Nip05Badge, NpubChip, UserCard) use owned data with no lifetime parameters leaking into the element lifetime.

<!-- citations: [^6a951-61] [^6a951-134] [^6e8af-5] -->
## Design Rationale — Why No Hidden Claim-Trigger Components

Adding a hidden NostrAvatar to a page purely to trigger a profile claim is a hack and exactly what the goal forbids. The user explicitly reverted this approach. If a component needs a profile to render, it must own the claim openly — by requiring pubkey: as input and composing self-claiming primitives. If the author shows as hex in an embed, that is a kernel bug or a missing claim in the component, not something to work around with hidden trigger components. [^6a951-62]

## See Also
- [[component-owned-reactivity-architecture|Component-Owned Reactivity Architecture]] — related guide
- [[kernel-never-fetches-kind0-from-event-ingest|Kernel Never Fetches kind:0 From Event Ingest — Profile Fetching Is Presentation-Layer]] — related guide
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide

