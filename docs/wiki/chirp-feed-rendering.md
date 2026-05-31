---
title: Chirp Feed Event Rendering
slug: chirp-feed-rendering
summary: "Kind:6 repost events display extracted inner note content with a '↩ Repost' label rather than raw JSON."
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-26
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:582fca30-be51-4861-bb16-3788610c6fb7
  - session:f7021d71-aadd-4666-a266-a033744efd77
  - session:17ef19cd-8549-4fa9-b09c-5266aaf480a7
  - session:161ad3af-aeba-42f7-98ab-a71d2fda69a7
  - session:c5325e71-7d4e-451e-8c15-81cdae440f5f
  - session:6e6bcf78-bf6b-4ddd-a2b8-4fb829d86604
---

# Chirp Feed Event Rendering

## Repost Rendering

Kind:6 repost events must be handled centrally in crates/nmp-core/kernel/update.rs's make_timeline_item, which detects kind == 6, parses the content as JSON, and extracts the inner event's content string. When a user reposts a note that already appears in the feed, the original note's block is bumped to the top rather than displaying a duplicate standalone block for the repost. (Previously: a 'Repost' badge and the extracted inner text content were shown instead of raw JSON.) The feed must display the original note's author, kind, and content for reposts, not the reposter's information as the event author, and includes an optional `reposted_by: RepostAttribution` to distinguish the reposter. The `reposted_by` field on `TimelineEventCard` uses `skip_serializing_if = "Option::is_none"` so that existing external consumers (Swift/Kotlin) that do not decode it continue working without breaking. For a repost card, `card.created_at` uses the kind:6 repost timestamp so the feed positions the bumped item at the top, but the UI displays the original note's creation time for the age indicator. TimelineItem (Rust struct and Swift Decodable struct) includes a `kind` field (u32 / UInt32) populated from `event.kind` to distinguish repost events from regular notes. Swift views use `item.kind == 6` for repost detection rather than heuristic JSON parsing. The event renderer reads the author pubkey from a field named `author_pubkey` on card objects, with a fallback to `pubkey` for raw events. The UI renders a repost indicator line prepended above the author line displaying "↻ <reposter> reposted <age>". When the profile of an original note's author arrives, it must refresh any repost cards for that author, and similarly for the reposter's profile updating the attribution. Tapping a kind:6 repost navigates to the inner note's thread (the original note's ID), not the wrapper kind:6 event's thread. ThreadNoteRow shows a 'Repost' badge and extracted inner text for kind:6 items. ModularBlockView.syntheticItem propagates the `kind` field to its generated TimelineItem. All event embeds must display the referenced event. The Chirp snapshot refreshes on every projection tick rather than only when items change, so that quoted events arriving via discovery oneshots are included in the cards map. A nextTimeline != modularTimeline equality check prevents spurious SwiftUI re-renders when the snapshot refreshes every tick.

<!-- citations: [^582fc-9] [^f7021-1] [^17ef1-1] [^161ad-1] [^c5325-1] [^6e6bc-1] -->
## See Also

