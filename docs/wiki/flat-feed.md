---
title: FlatFeed and Author/Thread Views
slug: flat-feed
topic: flat-feed
summary: `FlatFeed` in `nmp-nip01` provides a flat chronological note list for author and thread views — distinct from `RootIndexedFeed` which is thread-roots-only with
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-01
updated: 2026-06-12
verified: 2026-06-01
compiled-from: conversation
sources:
  - session:37035e20-9c1c-418f-88f1-68e464b51ec7
  - session:cf071d35-ee9b-4a1f-a3b8-885c651e8cce
  - session:f1b740a8-d601-4b63-8633-072c83a6de22
  - session:63af4b96-d3d3-45c3-ab96-9f899beafa1b
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
---

# FlatFeed and Author/Thread Views

## Overview

`FlatFeed` in `nmp-nip01` provides a flat chronological note list for author and thread views — distinct from `RootIndexedFeed` which is thread-roots-only with replies as attribution. It reuses the same `RootFeedSnapshot` wire shape as the home feed (with empty attribution), requiring no new FlatBuffers schema or Swift codegen. nmp-feed owns repost-aware feed assembly because reposts affect ordering and composition of any feed regardless of the feed's primary event kind. nmp-feed is kind-agnostic and can serve a feed of kind:1, kind:20, or kind:30023 events. nmp-nip01 should only own the NIP-10 reply tag builder, not feed assembly or timeline projections.

<!-- citations: [^37035-3] [^cf071-3] -->
## Forward-Only Semantics

FlatFeed is forward-only — it shows notes that arrive after the feed registers, not notes already in LMDB. Runtime verification on the iOS simulator is the acceptance gate. <!-- [^37035-4] -->


PR #941 ships FlatFeed decode infrastructure but deliberately does not cut over ProfileView/ThreadScreen to the new feed, because relay backfill was not delivering events in the test environment. <!-- [^f1b74-5] -->
## Registration & Kind Policy

Author and thread feed registration lives in the host composition crate (`nmp-app-chirp`), not in kernel dispatch, so the kind policy (`{1, 6}`) stays in the app layer (D0-correct). <!-- [^37035-5] -->

## Swift Consumption

ProfileView and ThreadScreen in Swift read dynamic per-open feed projections via `projections["nmp.feed.author.<pk>"]` / `projections["nmp.feed.thread.<id>"]` rather than static typed `authorView`/`threadView` properties. Dynamic author/thread feeds (nmp.feed.author.*/nmp.feed.thread.*) use FlatFeed::snapshot() returning OpFeedSnapshot, so the existing NOFS typed encoder/schema applies with zero new FlatBuffers work. FlatFeed projection decode uses a non-generated arbitrary-key reader (extractFeedProjections/FeedProjectionKey) because the transport is a FlatBuffer Value decode into fixed-key generated structs, not arbitrary JSON.

<!-- citations: [^37035-6] [^f1b74-4] [^63af4-2] -->
## Feed Item Data Purity

The feed item type should carry raw protocol data only, not UI/render decisions like content_preview, is_repost, or per-row denormalized display fields. TimelineItem should not carry any NIP-57 (zap) awareness; whether to show zaps is the app's decision. The author_lnurl field on TimelineItem should be deleted entirely; zap capability comes from ProfileCard.lnurl in the resolved_profiles map. Display names in the desktop feed should resolve through the resolved_profiles projection (BTreeMap<String, ProfileCard>), not be baked into feed item rows. <!-- [^cf071-4] -->

## TimelineItem Migration

TimelineEventCard exists in crates/nmp-nip01/src/timeline_projection.rs. The feed cluster of TimelineItem (visible_items, diff_items, last_emitted_items, and the timeline/inserted/updated/removed projection keys) should be deleted from nmp-core. The legacy {1,6} C-ABI symbols (nmp_app_open_author, nmp_app_close_author, nmp_app_open_thread, nmp_app_close_thread) and the parallel AuthorViewState/ThreadViewState state machine, author_view/thread_view typed projections, and their FlatBuffers schemas are deleted (−5,750 LOC across 66 files). Migration from the deleted author/thread surfaces uses the generic seam nmp_app_open_interest with a verbatim NIP-01 filter, and profile hydration is component-owned via nmp_app_claim_profile/nmp_app_release_profile.

<!-- citations: [^cf071-5] [^f1b74-3] [^da6b1-75] [^da6b1-99] -->
## Home Feed Projection

The home feed should route through the nmp.feed.home OP-feed projection, not the legacy timeline/inserted/updated/removed keys. chirp-desktop Home tab should read from nmp.feed.home with wrapped RootCard shape (card + attribution), not a bare Vec of cards. <!-- [^cf071-6] -->

## Open-View Pin Sets

Open-view pin sets protect: thread focused id + derived root + `referenced_event_ids(focused)` + all four hydration bookkeeping sets + every cached event matching the `thread_items()` membership predicate; author view pins every cached event by the selected author. The Opus review of PR #1096 caught that open thread/author views read from self.events with no store fallback, so eviction would blank live thread rows; the fix pins the open-view working set including all four hydration bookkeeping sets. The Opus review of PR #1100 caught that merging it would silently break open-view RAM pinning because the pin code referenced thread_view/author_view fields that this PR deletes, and the 4 ram_eviction_view_pin_tests called deleted open_thread/open_author APIs.

<!-- citations: [^da6b1-63] [^da6b1-74] -->
