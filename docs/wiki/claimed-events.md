---
title: claimed_events Snapshot Projection
slug: claimed-events
summary: `claimed_events` is a built-in kernel snapshot projection mapping `primary_id` keys to `ClaimedEventDto` payloads for every event that has been claimed via `claim_event` and is now available in the local read cache, enabling push-based rendering of embedded event references.
tags:
  - kernel
  - snapshot
  - projection
  - claimed-events
  - ADR-0034
volatility: warm
confidence: low
created: 2026-05-29
updated: 2026-05-29
verified: 
compiled-from: codebase
sources:
  - codebase
---

# claimed_events

The `claimed_events` snapshot projection (introduced in F‑CR‑06 / ADR‑0034) serves UI components that need to render an event after calling `claim_event(primary_id, consumer)` (event_claim_tests.rs:2). It is a built‑in, kernel‑owned projection included in every snapshot update under the key `"claimed_events"` (snapshot_registry_tests.rs:127). Its value is a JSON object mapping each `primary_id` to a `ClaimedEventDto` payload (types.rs:874).

**Keying**: Entries are keyed by `primary_id`, which is either a hex‑64 event id (for nevent / note URIs) or a `kind:pubkey:d_tag` coordinate (for naddr URIs) (projections.rs:262‑265).

**Population**: On every snapshot tick, the kernel iterates `self.event_claims.keys()` and calls `lookup_for_primary_id` to retrieve the corresponding `StoredEvent` from the in‑memory cache or the EventStore (projections.rs:273‑289, views.rs:33). Only events that are already available locally produce an entry; missing events are silently omitted (projections.rs:289‑298). This best‑effort, push‑based model means a renderer gets an immediate snapshot of all resolved claims and receives further updates in subsequent ticks when the event arrives (D8 / push semantics). The `ClaimedEventDto` is enriched with the author’s display name and picture URL from the kernel’s profile cache, if available, so the embed renderer can compose a full preview without additional FFI calls (projections.rs:278‑285).

**Always present**: The `"claimed_events"` key is always present in the `projections` object, even when no events have been claimed (the value is an empty `{}`). This allows hosts to pre‑allocate a map slot without guarding against a missing key (snapshot_registry_tests.rs:127‑128).

**Integration**: The projection is assembled inside `snapshot_projections_with_publish_cluster` alongside other kernel‑owned projections (projections.rs:262‑298). It is not user‑configurable via the host‑extension registry; it is part of the stable contract of every snapshot. In tests, assertions verify that claimed events appear only after ingest and that the correct DTO fields are emitted (event_claim_tests.rs:153‑154, 234‑237, 319‑345).
