---
title: Timeline Metadata Fetch
slug: timeline-metadata-fetch
topic: marmot
summary: "Historical capture of follow-feed and metadata-fetch work; current rule is open_feed(FeedParams), ReducedSource composition, and component-owned dependent interests."
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-26
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53
  - session:5d180e52-7c43-4a99-bfc4-769eb40dc03f
  - session:c4b2e655-ca6b-42d2-9383-89bf52215d0a
  - session:17ef19cd-8549-4fa9-b09c-5266aaf480a7
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
  - session:6e4c3a3a-9515-4437-a4bf-b4228a10ae57
  - session:64f3e239-c4c1-4c32-82de-458516b28418
---

# Timeline Metadata Fetch

## Follow Feed Subscription

This is a historical capture. The current authoritative rule lives in
ADR-0036, ADR-0042, and issue #2092:

- Apps open feeds with typed `open_feed(FeedParams)`.
- `FeedParams.primary_kinds` carries app primary content kinds such as `[1]`
  for Chirp or `[20]` for Olas.
- `FeedParams.acquisition` carries a closed `FeedScope` / `PubkeySetExpr`
  ReducedSource such as `ActiveUserFollows`.
- Protocol/defaults code reduces that source into planner-owned
  `LogicalInterest`s.
- Native shells never pass concrete follow pubkeys, watch kind:3, or build
  author filters.

The older V-45 contact-list source names were design directions that did not
ship. They must not be used as current implementation guidance. The replacement
direction is #2092: one ReducedSource/dependent
interest mechanism that can express active follows, NIP-51 lists/mutes/follow
packs, and similar source-derived acquisition without adding a bespoke door per
source kind.

NMP supports an "authors from current user's follows" feed by compiling the
active-account ReducedSource into materialized interests. Following accounts
automatically changes the downstream subscription set through Rust-owned
source reduction and planner recompilation, not Swift/Kotlin/TUI logic.

TimelineItem carries a `kind` field (u32 in Rust, UInt32 in Swift) populated from `event.kind`, allowing Swift to branch on `item.kind == 6` for reposts instead of relying on fragile JSON heuristics. The Swift `TimelineItem` Decodable struct uses `.convertFromSnakeCase`, so no CodingKeys override is needed for the new `kind` field.

#2092 retires the bespoke `follow_feed_*` / `sync_follow_feed_interests` paths.
Active follows are one ReducedSource/dependent-interest feed source; apps should
open the active-follows feed through the normal feed API and let Rust-owned
source reduction recompile the dependent interests.

<!-- citations: [^fd809-4] [^17ef1-4] [^5d180-1] [^c4b2e-9] [^6e4c3-3] [^64f3e-6] -->
## Kind:0 Metadata Fetching

Components/read models drive profile and event metadata by claiming dependent
interests or refs for the pubkeys/events they render. Feed acquisition does not
bundle kind:0 profiles, missing repost targets, reply counts, or meta-target
hydration. Those dependencies use the normal registry/planner/cache lifecycle
and remain Rust-owned.

<!-- citations: [^95d02-16] [^fd809-5] [^5d180-2] [^c4b2e-10] [^17ef1-3] [^86221-9] -->
## Kind:6 Repost Rendering

Kind:6 reposts in the Chirp timeline render the inner note's text content with a "Repost" badge, rather than displaying raw JSON. The previous `effectiveContent` heuristic in NoteRowView detected reposts by checking for a `sig` field in the embedded JSON, which failed when relays or clients stripped the `sig` field from reposts. A potential follow-up is to have the kernel emit a `contentTree` for the inner event of a kind:6 repost so the body renders with entity decoration rather than plain text. <!-- [^17ef1-5] -->
