---
title: Local Echo (Read-Your-Writes)
slug: local-echo
topic: event-acquisition
summary: "Non-replaceable events (kind:1 notes/replies, kind:6 reposts, kind:7 reactions) must receive optimistic local echo via read-your-writes, just as replaceable eve"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-15
updated: 2026-06-15
verified: 2026-06-15
compiled-from: conversation
sources:
  - session:78b50727-bccd-4088-8493-a07624a4fa83
---

# Local Echo (Read-Your-Writes)

## Local Echo for Non-Replaceable Events

Non-replaceable events (kind:1 notes/replies, kind:6 reposts, kind:7 reactions) must receive optimistic local echo via read-your-writes, just as replaceable events do. Local publishes must be tagged with `Provenance::Local` (the `local://publish` marker), and the timeline-cache observer must treat `Provenance::Local` as unconditionally relevant so that read-your-writes for all kinds falls out naturally without per-kind arms. Locally accepted publish events must be pinned while in the publish queue until first relay confirmation or terminal settlement, so read-your-writes events are not LRU-evicted before the relay echo.

<!-- citations: [^78b50-1] [^78b50-10] [^78b50-136] -->
## Self-Publish Admission Gate

The `should_store_event` admission gate blocks read-your-writes because a user is not normally in their own follow set, causing a self-authored kind:1 root note to be dropped or parked in `pre_kind3_buffer`. The gate must include a self-publish admission clause (e.g. a sentinel sub_id like `"local://publish"` or `Provenance::Local` tag) that unconditionally admits self-authored events.

<!-- citations: [^78b50-4] [^78b50-11] -->
## Gift-Wrap Exclusion and Behavioral Predicates

Kind:1059 gift-wraps must be excluded from local echo, handled via the parser registry rather than a kind literal. The kernel must gate event routing by behavioral predicates (e.g. `follow_feed_kinds.contains`, `is_replaceable`, `is_parameterized_replaceable`, the `KIND_GIFT_WRAP` constant) rather than hardcoding NIP kind numbers, per doctrine D0.

<!-- citations: [^78b50-5] [^78b50-12] -->
## Required Test Coverage

Tests must verify: a local kind:1 publish appears in the timeline and fans out to observers exactly once; a local kind:1 reply with no active interest is still admitted (admission-gate regression guard); a relay echo of a local kind:1 dedups to `Duplicate` and does not double-fire (D4); a kind:7 reaction gets a local echo; and a gift-wrap is not locally ingested. <!-- [^78b50-6] -->
