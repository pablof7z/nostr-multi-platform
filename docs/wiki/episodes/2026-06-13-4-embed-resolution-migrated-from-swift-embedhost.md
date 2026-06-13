---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-ffi-embed-sidecar
  - embedhost-migration
  - claimed-event-embeds
supersedes:
  - 2026-06-13-3-embedhost-resolution-moves-from-per-platform
related_claims: []
source_lines:
  - 8871-8896
  - 8898-8926
captured_at: 2026-06-13T20:56:22Z
---

# Episode: Embed resolution migrated from Swift EmbedHost to Rust nmp-ffi sidecar

## Prior State

Embed resolution was done client-side in Swift: each iOS app (Gallery, Chirp) had its own EmbedHost.swift (~150 lines of kind-dispatch resolver) that decoded JSON update payloads, called resolve_embed_projection, and built EmbeddedEventEnvelope maps.

## Trigger

Issue #1283: move embed resolution ownership to Rust (nmp-ffi) as the source of truth, eliminating per-platform resolver duplication and making the resolution logic testable and updatable from the Rust side.

## Decision

New nmp-ffi embed_sidecar.rs (320 LOC): a listener thread reads KCEV FlatBuffer on each update frame, calls nmp_content::resolve_embed_projection for every claimed event, stores pre-resolved primary_id → EmbeddedEventEnvelope map in a shared slot. Gallery iOS EmbedHost.swift deleted entirely (~150 lines → 5-line update). claimed_event_embeds registered as a snapshot projection from Rust. Chirp iOS scaffolded (Decodable + CodingKeys on projections, claimedEventEmbeds field added) but Swift resolver retained with TODO — needs typed FlatBuffer sidecar because Chirp's frame has no JSON payload.

## Consequences

- Rust (nmp-ffi) is now the source of truth for embed resolution; iOS apps consume pre-resolved data
- Gallery iOS embed resolver code deleted; Chirp iOS resolver still pending (tracked in #1335)
- Android gallery has no centralized resolver yet — needs decode path for claimed_event_embeds when wiring views
- External consumers need git-rev bump + new decode path when updating
- lib.rs must stay at 2976 LOC baseline (wiring extracted to embed_sidecar module)

## Open Tail

- #1335 tracks Chirp typed-sidecar swap + Android gallery decode
- Chirp iOS resolver retained as TODO pending FlatBuffer sidecar

## Evidence

- transcript lines 8871-8896
- transcript lines 8898-8926

