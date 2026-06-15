---
title: Store-Projection Replay
slug: store-projection-replay
topic: store-projection-replay
summary: ADR-0045 storeâprojection replay is accepted (staged); implementation is tracked separately
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:bf035812-6f7a-46ec-a11d-30fc7369342f
  - session:019ec57a-fb01-7081-80c8-d7107f302049
  - session:78b50727-bccd-4088-8493-a07624a4fa83
  - session:019eca68-85c6-77e0-b237-e58f6c894f72
---

# Store-Projection Replay

## Store→Projection Replay

ADR-0045 store→projection replay is accepted (staged); implementation is tracked separately. ADR-0042 was corrected in place to remove the false `should_store_event` store-admission framing and state that persistence is unconditional with feed-engine projection at read time. A relevance-gated store cannot faithfully rebuild projections and permanently loses historical events that were dropped at ingest, violating event-sourcing replayability. The resubscribe fix also gets store-replay for free via the #1237 push_interest_and_serve choke point, replaying stored-but-unprocessed kind:445s on restart. Cache replay keeps its special rule: it skips `store.insert` (already persisted) and feeds the same post-store projection/notify seam directly, per ADR-0045. The three provenance encodings (relay URL, local://publish for local publish, relay_count:0 for cache-serve) are preserved in the unified architecture; the ADR does not introduce a Provenance::LocalStore enum. The feedback one-change fix collapses to one mechanism with one retention policy: `action_lifecycle` is the sole host-facing projection with TTL-anchored retention where ack is purely early-dismiss. Hydration reuses the F-12 store→projection replay mechanism; the web port inherits it and plugs in a backend, requiring no new replay logic. No delta protocol is built because WireDelta was shipped, consumed by zero consumers, and deleted; the snapshot bet is empirically validated. New nondeterministic inputs (time, randomness, network, OS callbacks, capability completions) must enter the actor as explicit actions/events or injected seams; reducers must remain replayable from message history. iOS `SignerStateTone.derivedLabel` and `WalletStatusTone.derivedLabel` duplicate Rust business logic by switching on raw state tokens as `??` fallbacks for 'older buffers' that cannot exist in a running app (once a build ships the fields, old buffers from the same process are impossible).

<!-- citations: [^02745-19] [^da6b1-39] [^78c8e-31] [^2e544-70] [^bf035-171] [^019ec-19] [^2e544-441] [^78b50-27] [^78b50-33] [^019ec-54] [^78b50-137] [^78b50-225] -->
