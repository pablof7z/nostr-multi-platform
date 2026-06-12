---
title: Snapshot Emission
slug: snapshot-emission
topic: snapshot-emission
summary: The 4Hz snapshot emit only fires when state has changed (changed_since_emit) and 250ms has elapsed; user-dispatched commands emit immediately via maybe_emit_aft
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
  - session:cf071d35-ee9b-4a1f-a3b8-885c651e8cce
---

# Snapshot Emission

## Snapshot Emission

The 4Hz snapshot emit cadence (DEFAULT_EMIT_HZ = 4) is a ceiling, not a timer; it emits only when state has changed since the last emit, and user-dispatched commands emit immediately via `maybe_emit_after_dispatch`/`emit_now`. Each snapshot emit calls `Kernel::make_update`, which bumps rev, assembles a KernelSnapshot from registered projections (bounded by open views, never the whole store), encodes once into a FlatBuffers `UpdateFrame`, and pushes over an mpsc to the host callback; no store scans on the hot path (aggregates are maintained incrementally). The snapshot transport uses FlatBuffers UpdateFrame with a typed_projections sidecar alongside the legacy payload:Value JSON tree; the typed path is preferred by Swift and both are emitted from the same accessor in the same tick. The snapshot perf gate thresholds were tightened from 250,000/150,000 µs to 15,000/8,000 µs (approximately 17× tighter), calibrated against measured under-contention values with ~10× margin for CI variance. Denormalized profile data (author_display_name, author_picture_url) should live in the snapshot's resolved_profiles map, not baked into each feed item row. The 10050 fix's snapshot-diff pattern clones two caches before and after every ingested wildcard event, creating O(caches) per-event cost that scales with each new cache adopting the pattern, and should be replaced with a mutation signal from EventIngestDispatcher parsers.

<!-- citations: [^da6b1-18] [^cf071-8] [^da6b1-34] [^da6b1-56] [^da6b1-68] [^da6b1-81] [^da6b1-90] [^da6b1-108] -->
