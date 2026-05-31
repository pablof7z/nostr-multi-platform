---
title: "Kernel Never Fetches kind:0 From Event Ingest — Profile Fetching Is Presentation-Layer"
slug: kernel-never-fetches-kind0-from-event-ingest
summary: "The kernel must never fetch an author's kind:0 because an event was ingested. Profile fetching is a presentation-layer decision made by self-claiming NMP components."
tags:
  - architecture
  - doctrine
  - kernel
  - reactivity
  - components
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-25
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
  - session:fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53
  - session:7b4ae585-801c-441f-811d-5308e1002f08
  - session:53838558-81bd-433d-a46d-d117ecebb361
---

# Kernel Never Fetches kind:0 From Event Ingest — Profile Fetching Is Presentation-Layer

> The kernel must never fetch an author's kind:0 because an event was ingested. Profile fetching is a presentation-layer decision made by self-claiming NMP components.

## Core Principle

No event ingest path may ever trigger the kernel to fetch an author's kind:0 profile. Fetching kind:0 is always, exclusively, the presentation layer's decision — 'I need to render this user.' The kernel ingests events; components claim profiles. These are separate concerns and must never be coupled. This applies universally: kind:7 reaction authors must not receive a profile claim through the ingest path and must fall through to the default arm with no claim, just as no other event kind triggers a profile fetch. [^6a951-70]

There are narrow, explicitly scoped exceptions where the kernel fetches kind:0 for its own operational needs, not triggered by general event ingest: on launch, the kernel fetches kind:0 for the active account and seed accounts; opening a profile explicitly fetches kind:0 for that specific pubkey on demand; and when a note (kind:1 or kind:6) arrives in the home feed, the kernel harvests all unknown p-tag pubkeys from that event and queues them for a metadata backfill. For followed users generally, kind:0 metadata is lazily retrieved via timeline-driven discovery rather than by subscribing to kind:0 for every followee upfront.

nmp_app_claim_profile must work without a signed-in user; the kernel connects to the indexer relay and fetches kind:0 and kind:10002 automatically. Profile data in the kernel snapshot surfaces at 'projections.author_view.profile', which requires calling nmp_app_open_author to populate.

<!-- citations: [^6a951-70] [^fd809-3] [^7b4ae-4] [^53838-4] -->
## What This Replaces

The pre-existing request_profile_for_rendered_note call at ingest/timeline.rs:172 fetches an author's kind:0 when a note is ingested for timeline rendering. This is the wrong architecture: it couples event ingest to profile fetching, making the kernel responsible for a decision that belongs to the presentation layer. It also enriches claimed_events with author_display_name and author_picture_url (projections.rs:278-285), injecting profile data into the event projection that the component never asked for. Both must be removed. [^6a951-71]

## Migration Path — Components First, Kernel Cleanup Last

Removing the kernel-side enrichment must happen after (not before) all platform components have migrated to self-claim. The three-phase order is: (1) Every atomic component that displays an author byline (NostrProfileName, NostrAvatar, embed renderers) becomes self-claiming — it calls claim_profile(pubkey) when it mounts. (2) Apps (gallery showcase pages, Chirp's NoteRowView, timeline, thread) stop passing static authorDisplayName strings and instead compose self-claiming primitives. (3) Only after all platforms are migrated: delete request_profile_for_rendered_note from ingest/timeline.rs, strip author_display_name and author_picture_url from ClaimedEventDto, and remove the claimed_events enrichment block. [^6a951-72]

## Why This Is A Hard Rule

The user explicitly stopped a kernel-fix agent that was implementing the forbidden behavior — making verify_and_persist request an author profile on kind:30023/9802 ingest. The user's principle reverses the agent's approach: fetching kind:0 is always the presentation layer's decision. The kernel must never fetch kind:0 because an event was ingested. This applies to all event kinds — kind:1 notes, kind:30023 articles, kind:9802 highlights, kind:6 reposts — without exception. [^6a951-73]

## Self-Claiming Component Pattern

The correct pattern: NostrAvatar(pubkey:) already self-claims — it calls claim_profile when it appears and release_profile when it disappears. NostrProfileName(pubkey:) must do the same. Embed renderers compose these primitives for author bylines: they render NostrAvatar(pubkey: authorPubkey) + NostrProfileName(pubkey: authorPubkey) instead of reading a static authorDisplayName string from the event DTO. When these components mount, they claim the profile. The kernel responds by emitting the profile in resolved_profiles, and the name resolves through the normal fallback chain. Nothing is enriched in the event ingest path. [^6a951-74]

## See Also
- [[self-claiming-nmp-components|Self-Claiming NMP Components — Components Own Their Data Claims, Apps Compose Them]] — related guide
- [[resolved-profiles-kernel-projection|resolved_profiles — Kernel-Level Profile Merge Projection]] — related guide
- [[claimed-events|claimed_events Snapshot Projection]] — related guide

