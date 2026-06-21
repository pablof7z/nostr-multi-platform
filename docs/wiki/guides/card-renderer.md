---
title: Card Renderer
slug: card-renderer
topic: ui-components
summary: The author field is renamed to author_profile (type ProfileWire) and displayed via author_label()
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-06-19
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:161ad3af-aeba-42f7-98ab-a71d2fda69a7
  - session:c5325e71-7d4e-451e-8c15-81cdae440f5f
  - session:e4d33847-af62-4a40-a7f2-1a77b96605a3
  - session:e3b42d41-ffd2-44b3-9e5a-93832feb46e0
  - session:fa300009-e498-4c80-a2d3-64d1531a09d4
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edcba-b578-71f3-be33-f670962f11a7
---

# Card Renderer

## Author Resolution

The author field is renamed to author_profile (type ProfileWire) and displayed via author_label(). The card renderer resolves the author field using a fallback that checks both author_pubkey (for TimelineEventCard objects) and pubkey (for raw events). nip01-attribution mirror-dedup removes the redundant author_display_name and author_picture_url flat mirrors from Nip10ReplyAttribution and AuthorDisplay.npub; shells read nested authorDisplay + nmp_app_encode_profile.

<!-- citations: [^161ad-2] [^c5325-1] [^e3b42-2] [^11850-93] -->
## Rendering

The Chirp snapshot refreshes on every tick rather than only when items change, so that quoted events arriving via discovery oneshots are included in the cards map. <!-- [^c5325-2] -->


Content truncation is applied at render time, not at data-storage time. <!-- [^e4d33-2] -->

The embedded event widget on TUI, iOS, and Android renders with clear boxing so that it is obvious the event is embedded inside another event. The iOS embedded event widget border line width is 1.5 (not the previous invisible hairline of 0.5), and the Android embedded event widget border width is 1.5.dp (not the previous invisible hairline of 0.5.dp). <!-- [^fa300-1] -->
## Data Model

TimelineRow.content stores the raw, full content rather than a pre-truncated preview. <!-- [^e4d33-1] -->


ProfileCard must expose app-neutral raw fields (name, display_name, banner, website, lud16, lud06) alongside typed fields, without turning ProfileCard into an arbitrary JSON bag. <!-- [^019ed-149] -->
## Dead Code

The content_preview field and its helper function are removed in favor of content.

<!-- citations: [^e4d33-3] [^e3b42-1] -->
