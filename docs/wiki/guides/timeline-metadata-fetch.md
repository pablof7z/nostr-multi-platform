---
title: Timeline Metadata Fetch
slug: timeline-metadata-fetch
topic: marmot
summary: The follow-feed/timeline is a higher-order layer that sits above the base event-query substrate; it must be clearly delineated from the base layer in issue trac
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

The follow-feed/timeline is a higher-order layer that sits above the base event-query substrate; it must be clearly delineated from the base layer in issue tracking and design.

The store layer must support a multi-author query primitive (`StoreQuery::AuthorsKind { authors, kinds, since, until }`) so that consumers can request events of given kinds from a set of pubkeys in a single batched call.

The follow feed uses one `AuthorsKind` multi-author interest with `ContactsLookup` instead of reading `follow_set` directly. The 500-author cap (`TIMELINE_AUTHOR_LIMIT`) is retired per #1497 amendment 5/6 and the code should be removed.

In Android's tab-based UI, when a user on first launch signs in on tab 4 and then taps tab 0, TimelineScreen enters composition and its LaunchedEffect(model) fires, calling model.openTimeline().

Removing the imperative bridge.openTimeline() calls from signInNsect/createAccount/switchAccount is safe for account-switch and already-active-account cases because the Rust identity paths call reconcile_follow_feed_after_identity_change(), which re-targets an existing follow-feed registration on active-account change.

P4 Finding 1 is widened so p4 owns both the kernel `follow_feed_kinds` persistence fix and the native `openTimeline` deletion in a single PR, avoiding an unmasked intermediate state where both platforms could have no feed after sign-in.

PR #1545 implements this P4 fix: nmp-core now stores host-declared `follow_feed_kinds` unconditionally (even with no active account), and the imperative `openTimeline` call is removed from Android signInNsec/createAccount/switchAccount, fixing both platforms.

<!-- citations: [^7c780-9] [^019ed-47] [^019ed-48] [^019ed-49] [^129d2-68] [^11850-24] [^11850-88] -->
