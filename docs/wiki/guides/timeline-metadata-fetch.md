---
title: Timeline Metadata Fetch
slug: timeline-metadata-fetch
topic: marmot
summary: Timeline acquisition is a higher-order ReducedSource layer above the base event-query substrate; metadata and secondary hydration stay as dependent interests.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-18
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:7c780fef-d33c-4d22-bcdb-2d9ab625a4f9
  - session:019edc0c-2dd1-7b80-b737-7499340e1b49
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# Timeline Metadata Fetch

## Timeline Metadata Fetch

The follow-feed/timeline is a higher-order layer above the base event-query
substrate; it must be clearly delineated from static `open_interest` in issue
tracking and design.

Current canonical direction (#2092): app feeds open typed `FeedParams` with
primary content kinds and a closed `FeedScope` / `PubkeySetExpr` source. Rust
protocol/defaults code reduces that source into materialized `LogicalInterest`s.
The planner sees only those concrete interests; native shells never pass a
snapshot of follow pubkeys, list members, mute-list authors, or follow-pack
members.

The store layer still needs efficient multi-author queries such as
`StoreQuery::AuthorsKind { authors, kinds, since, until }` so the derived
interest can serve cache results in one batched read. That store primitive is
not the source-reduction primitive; it is the materialized read path after the
source has reduced.

The historical follow feed used one `AuthorsKind` multi-author interest with
`ContactsLookup` instead of reading `follow_set` directly. The 500-author cap
(`TIMELINE_AUTHOR_LIMIT`) is retired per #1497 amendment 5/6 and remaining
bespoke follow-feed code is tracked as #2092 migration debt.

In Android's tab-based UI, when a user on first launch signs in on tab 4 and then taps tab 0, TimelineScreen enters composition and its LaunchedEffect(model) fires, calling model.openTimeline().

Removing imperative bridge `openTimeline()` calls from
signInNsec/createAccount/switchAccount is safe only when the Rust owner of the
feed source survives those transitions and re-runs source reduction on account
switch. The older implementation did that through
`reconcile_follow_feed_after_identity_change()`; #2092 moves the responsibility
to the generic ReducedSource/dependent-interest owner.

P4 Finding 1 is widened so p4 owns both the kernel `follow_feed_kinds` persistence fix and the native `openTimeline` deletion in a single PR, avoiding an unmasked intermediate state where both platforms could have no feed after sign-in.

PR #1545 implemented the P4 fix: nmp-core stores host-declared
`follow_feed_kinds` unconditionally (even with no active account), and the
imperative `openTimeline` call was removed from Android
signInNsec/createAccount/switchAccount. #2092 is the follow-up that retires the
bespoke follow-feed mechanism entirely in favor of the generic primitive.

<!-- citations: [^7c780-9] [^019ed-47] [^019ed-48] [^019ed-49] [^129d2-68] [^11850-24] [^11850-88] -->
