---
title: Event Relay Provenance — Backend Exists, Full UI Missing
slug: event-relay-provenance
summary: relay_count is exposed on TimelineItem and shown as a badge on iOS; the full provenance list (which relays, when) is not yet surfaced to any UI layer.
tags:
  - relay
  - provenance
  - f09
  - timeline
  - ios
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# Event Relay Provenance — Backend Exists, Full UI Missing

> relay_count is exposed on TimelineItem and shown as a badge on iOS; the full provenance list (which relays, when) is not yet surfaced to any UI layer.

## What Exists

`EventStore::provenance_for(event_id)` is implemented in `crates/nmp-store/src/events.rs:286`, returning `Vec<ProvenanceEntry>` with relay URLs, timestamps, and a primary flag. Stores up to 32 relays per event, LMDB-persisted.

A `relay_count: u32` field is exposed on `TimelineItem` (`crates/nmp-core/src/kernel/types.rs:134`), counting relays that delivered the event.

iOS Chirp shows a relay-count chip (radio antenna icon + count, e.g. "📡 3") when `relayCount > 0` in `NoteRowView.swift:173-187`. The iOS bridge binding exposes `relayCount: UInt32` in `KernelTypes.generated.swift`. [^42908-17]

## What Is Missing (F-09)

- No `relay_provenance` field on `TimelineItem` or `TimelineEventCard` exposing the actual list of relay URLs
- No drill-down "Received from" view showing which specific relays delivered the event and their delivery timestamps
- `TimelineEventCard` in `crates/nmp-nip01/src/timeline_projection.rs` has no relay-related fields at all
- No Android or TUI implementation of the relay count or provenance UI [^42908-18]

## See Also

