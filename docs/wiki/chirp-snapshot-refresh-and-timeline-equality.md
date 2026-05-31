---
title: Chirp Snapshot Refresh and Timeline Equality
slug: chirp-snapshot-refresh-and-timeline-equality
summary: The Chirp snapshot must refresh every tick, not only when items change, so that quoted events arriving via discovery oneshots are included in the cards map
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-23
updated: 2026-05-27
verified: 2026-05-23
compiled-from: conversation
sources:
  - session:c5325e71-7d4e-451e-8c15-81cdae440f5f
  - session:64f3e239-c4c1-4c32-82de-458516b28418
  - session:200932fb-5a92-44e0-8d42-2184d2e69094
  - session:ff4522ea-fb76-45dd-915e-ca14874698e7
---

# Chirp Snapshot Refresh and Timeline Equality

## Snapshot Refresh and Timeline Equality

The Chirp snapshot must refresh every tick, not only when items change, so that quoted events arriving via discovery oneshots are included in the cards map. A nextTimeline != modularTimeline equality check prevents spurious SwiftUI re-renders when refreshing the snapshot every tick. However, chirp-repl reads the timeline snapshot synchronously immediately after firing REQs, without waiting for relay responses, causing 0 cards on login even when events exist on the relay. Timeline windows use cursor-paginated bounded requests via nmp_app_chirp_snapshot_window, with Rust owning sorting, paging, card de-duplication, and quote-card inclusion. The TimelineWindowRequest page field is Option with skip_serializing_if, ensuring backward compatibility with the legacy unbounded snapshot() endpoint. TimelineWindowCursor uses (created_at, id) ordering with strict is_older_than semantics for page_start_after_cursor. cards_for_blocks transitively includes quoted-event cards from visible blocks' content_tree so render shells never have a dangling quote reference. The TUI sends timeline window requests with a limit but no cursor, deliberately re-walking blocks from offset 0 on every refresh.

When the `blocks` key is absent from a feed snapshot, `TimelineRow::from_snapshot` returns an empty Vec rather than falling back to promoting all cards to depth 0. [^ff452-1]

<!-- citations: [^c5325-1] [^64f3e-1] [^20093-2] -->

## Diagnostics and Invariants

A debug_assert is added to block_window_cursor to catch the impossible case of a block with no event ids, alongside a comment explaining the eviction fallback to (0, ""). A comment is added in HomeFeedView.onAppear explaining why no retry/spin happens when the visible limit hits 500 and nextCursor becomes nil. [^20093-3]


Tests for timeline row parsing use realistic snapshots that include a `blocks` key, not cards-only snapshots. [^ff452-2]
## Metrics

A make_window_us metric is added to the metrics block to observe the O(N log N) sort cost per render tick. [^20093-4]
## See Also

