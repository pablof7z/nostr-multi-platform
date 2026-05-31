---
title: NMP Event Ingest Dispatcher & NIP Crate Registration
slug: nmp-event-ingest-dispatcher
summary: NIP crates register kind-specific ingest parsers via `EventIngestDispatcher` at composition time — the kernel never pattern-matches NIP kinds directly.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-23
updated: 2026-05-29
verified: 2026-05-23
compiled-from: conversation
sources:
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:855be2a2-4866-4d8d-ad4f-145309da56bc
---

# NMP Event Ingest Dispatcher & NIP Crate Registration

## Event Ingest Dispatcher

NIP crates register kind-specific ingest parsers via `EventIngestDispatcher` at composition time — the kernel never pattern-matches NIP kinds directly. Per D0 doctrine, the nmp-core substrate has zero NIP knowledge; no NIP-specific nouns may appear in the substrate API. The `RootIndexedFeed` engine accepts a caller-supplied `EventGate` predicate that filters events at the ingest entry point before any state is touched. For example, the `nmp-nip01` `register_op_feed` wiring supplies an `EventGate` that accepts only kind:0, kind:1, and kind:6 events into the `RootIndexedFeed`. Kind:0 events pass the gate because `profile_detector` would short-circuit them anyway, and blocking them would break the `profile_refresh_updates_buffered_attribution` test path. Kind:3 (contact list) and kind:10002 (relay list) events echoed back by relays after account creation are blocked from becoming phantom root cards in the NOFS feed.

<!-- citations: [^1670f-7] [^f2605-9] [^855be-4] -->
## See Also

