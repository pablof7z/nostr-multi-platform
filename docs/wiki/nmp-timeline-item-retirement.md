---
title: NMP TimelineItem Retirement & Framework Feed Type Design
slug: nmp-timeline-item-retirement
summary: TimelineItem must be retired from nmp-core, not merely moved elsewhere
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-03
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:cf071d35-ee9b-4a1f-a3b8-885c651e8cce
  - session:f1b740a8-d601-4b63-8633-072c83a6de22
  - session:37035e20-9c1c-418f-88f1-68e464b51ec7
---

# NMP TimelineItem Retirement & Framework Feed Type Design

## Retirement Rationale

TimelineItem must be retired from nmp-core entirely. The D0 violation in TimelineItem is caused by its producer (views.rs::timeline_item()) computing NIP-18 repost semantics in nmp-core, not by the struct's crate location. The 'card' concept is a UI responsibility and has no place at the framework layer; the correct shape of a feed item at the framework layer is an EventRecord (a raw Nostr event with related profile data denormalized), not a UI-oriented card type. Timeline types must not carry any NIP-57 (zap) awareness at any layer; whether to show zaps on an event is an app-level decision, not a framework concern.

<!-- citations: [^cf071-6] [^cf071-12] [^cf071-16] -->
## Type Refactoring

A FeedItem contains only protocol-level fields: id, author_pubkey, kind, created_at, content, content_tree, relay_count. Display-oriented fields (content_preview, is_repost, nav_target_id, repost_inner_content, author_lnurl, author_display_name, author_picture_url) must be deleted from framework-level feed item types. TimelineItem's is_repost, nav_target_id, and repost_inner_content fields must fold into an Option<RepostAttribution> type owned by nmp-nip18. TimelineItem's author_lnurl field must be deleted because zap display is an app-level decision, not a framework concern. content_preview and render hints are UI decisions and must not appear in framework-level feed types. TimelineEventCard in nmp-nip01 must be reshaped: stripped of author_display, author_display_name, author_picture_url, content_preview, content_render; RepostAttribution stripped to raw author_pubkey + note_created_at only.

<!-- citations: [^cf071-7] [^cf071-13] [^cf071-17] -->
## Wire Contract Coordination

The JSON wire contract change from retiring TimelineItem requires atomic updates to iOS codegen, Android decoder, and chirp-desktop, plus a SNAPSHOT_SCHEMA_VERSION bump. [^cf071-8]

## Partial Retirement & FFI Constraints

TimelineItem struct, timeline_item() producer, author_view, and thread_view clusters must remain in nmp-core until issue #911 resolves the frozen FFI symbols (nmp_app_open_author, nmp_app_open_thread). The timeline_item() producer, visible_items(), diff_items(), and the 'timeline'/'inserted'/'updated'/'removed' projection keys must be deleted from nmp-core. The legacy TimelineItem cluster is deleted as part of a timeline/feed projection cleanup. The final M2 delete-cascade removes open_author, open_thread, close_author, close_thread, all their bespoke kernel machinery (author_view, thread_view, ViewInterest, thread hydration methods), the snapshot projections, and migrates ProfileView/ThreadScreen to the feed engine across all apps.

<!-- citations: [^cf071-18] [^f1b74-8] [^37035-25] -->
## See Also

