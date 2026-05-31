---
title: Timeline Sort Order & Determinism
slug: timeline-sort-order
summary: Event timeline sorting uses `created_at DESC` as the primary sort key, tie-broken by `event_id ASC` (lexicographic)
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-28
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:8ac184a6-c923-4b67-b978-63cfe335d37a
  - session:e3b42d41-ffd2-44b3-9e5a-93832feb46e0
  - session:200932fb-5a92-44e0-8d42-2184d2e69094
  - session:3a906f87-ee2b-4d3a-9d5f-e82ccab29349
---

# Timeline Sort Order & Determinism

## Sort Order

Event timeline sorting uses `created_at DESC` as the primary sort key, tie-broken by `event_id ASC` (lexicographic). The event ID tiebreaker ensures the sort is fully deterministic regardless of relay delivery order. [^8ac18-1]



The timeline module must use the full `content` field instead of `content_preview`, and the dead `content_preview` helper function must be removed. The `author` field on row structs must be named `author_profile` of type `ProfileWire`, and author label references must use `row.author_label()`. When resolving merge conflicts in timeline.rs, `author_label`, `media_urls`, `author_profile_from_card`, and `push_unique_urls` must be kept while `content_preview` must remain removed. [^e3b42-3]

The `page` field on timeline snapshots is `Option` with `skip_serializing_if` so existing decoders are not disturbed by the new bounded window API. [^20093-15]

Timeline window cursors use `(created_at, id)` ordering with consistent tiebreakers across `is_newer_than`, `is_older_than`, and `newest_first`, and `page_start_after_cursor` uses strict `is_older_than` semantics to start after the last returned block. [^20093-16]

The `block_window_cursor` fallback yields `(0, "")` for blocks with no event IDs or evicted cards, placing the cursor at the absolute bottom of the sort. [^20093-17]

Visible quote cards are transitively included in timeline windows by walking the visible blocks' `content_tree` for `EventRef::Event` nodes, preventing dangling quote references in render shells. [^20093-18]
## Kernel Timeline

The `nmp-core` kernel performs binary-search-style insertion into the timeline via `insert_timeline_id_sorted` so the VecDeque is always sorted before any consumer sees it. The kernel timeline VecDeque is capped at 500 entries (`TIMELINE_CACHE_LIMIT`). iOS timeline limit constants `80` (default) and `500` (max) are exposed from Rust via `#define` in `NmpCore.h` or FFI accessor functions to prevent silent drift from the Rust-defined `DEFAULT_TIMELINE_WINDOW_LIMIT` and `MAX_TIMELINE_WINDOW_LIMIT`.

A `make_window_us` metric tracks the duration of the O(N log N) full-projection sort executed on every snapshot window request to ensure performance remains observable as projection size grows. [^20093-21]

<!-- citations: [^8ac18-2] [^20093-20] -->
## Client Re-sorting Behavior

The chirp-tui re-applies the same `created_at DESC` sort on its snapshot rows. Chirp iOS never re-sorts; it renders events in the order delivered by the Rust projection. The TUI timeline window sends requests with `limit` but no cursor, deliberately re-walking blocks from offset 0 on every refresh to avoid implementing page concatenation in the shell.

The chirp-tui re-applies the same `created_at DESC` sort on its snapshot rows. Chirp iOS never re-sorts; it renders events in the order delivered by the Rust projection. The TUI timeline window sends requests with `limit` but no cursor, deliberately re-walking blocks from offset 0 on every refresh to avoid implementing page concatenation in the shell. The `raw_card` in chirp-tui timeline.rs uses a canonical recursively key-sorted pretty-print, matching `encode_value`'s alphabetical sort rule. [^3a906-6]

<!-- citations: [^8ac18-3] [^20093-19] -->
## See Also

