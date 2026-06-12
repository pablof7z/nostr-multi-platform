---
title: Store-to-Projection Replay
slug: store-projection-replay
topic: store-projection-replay
summary: ADR-0045 specifies store→projection replay at interest-open time using existing StoreQuery indexes, replaying via existing post-store projection functions with
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-11
updated: 2026-06-12
verified: 2026-06-11
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
---

# Store-to-Projection Replay

## Store→Projection Replay

No code path replays stored events into projections; the watermark rewrite (apply_watermark_rewrite) floors every sub-shape's `since` at newest-stored+1, meaning offline second launch renders an empty feed and empty DM inbox despite a full local store, contradicting D1 offline-first. ADR-0045 Rev 2 establishes a single event-acquisition mechanism where serving from the local store is the first half and the planner's wire REQ is the refinement half — the same seam, not two modes — running always (cold/warm/offline/online) with no special cases. Store replay must NOT go through `store.insert` because the Duplicate arm in `kernel/ingest/timeline.rs` is a deliberate no-op, meaning insert-based replay would silently surface nothing. Replay depth defaults to 1× the view's visible window (fast first paint), with deeper replay available as a per-interest opt-in.

<!-- citations: [^da6b1-19] [^da6b1-35] [^da6b1-69] [^da6b1-109] -->
## Staged Rollout

Universal cache-serve gates v1; the acceptance test is: second launch offline, every open interest renders from the store. Store→projection replay stages 1–2 (timeline + DM offline rendering) are recommended to gate v1 by ADR-0045, with the owner's final adjudication pending.

<!-- citations: [^da6b1-20] [^da6b1-57] [^da6b1-82] [^da6b1-91] [^da6b1-110] -->
