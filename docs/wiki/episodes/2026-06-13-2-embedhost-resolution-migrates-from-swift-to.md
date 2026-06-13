---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: architecture
status: active
subjects:
  - nmp-ffi-embed-sidecar
  - embed-resolution
  - d0-thin-shell
supersedes:
  - 2026-06-13-4-embed-resolution-migrated-from-swift-embedhost
related_claims: []
source_lines:
  - 8871-8897
  - 9111-9115
  - 9378-9406
  - 9411-9447
captured_at: 2026-06-13T21:35:37Z
---

# Episode: EmbedHost resolution migrates from Swift to Rust (D0 thin-shell)

## Prior State

iOS Gallery app had ~150-line Swift kind-dispatch resolver for embeds (update(fromSnapshotJSON:), resolve(), parseProfileMetadata(), extractTopLevelMedia(), decodeTags(), envelope()); per-platform hand-written decoders with no parity gate; embed resolution lived in the shell layer

## Trigger

Owner decision #1283 to start migration; architecture finding that nmp-content depends on nmp-core (can't be imported from kernel), so resolution must live in nmp-ffi

## Decision

Embed resolution moves to Rust: nmp-ffi's new embed_sidecar.rs resolves embeds via nmp_content::resolve_embed_projection; claimed_event_embeds ships as runtime JSON sidecar (NOT codegen-declared — KernelTypes.generated.swift drops claimedEventEmbeds); Gallery iOS Swift resolver deleted, replaced with 5-line update(claimedEventEmbeds:)

## Consequences

- Gallery iOS Swift resolver fully deleted (~150 lines → 5 lines)
- claimed_event_embeds is a runtime JSON sidecar, not a FlatBuffer/codegen projection
- Chirp iOS needs typed FlatBuffer sidecar for Phase 1 (frame has no JSON payload) — tracked in #1335
- Android gallery needs decode wiring — tracked in #1335
- lib.rs file-size baseline drift from merge interleave with #1326 required #1343 net-zero refactor

## Open Tail

- Chirp iOS typed FlatBuffer sidecar (Phase 1)
- Android gallery decode wiring (Phase 1)
- External consumer git-rev bumps

## Evidence

- transcript lines 8871-8897
- transcript lines 9111-9115
- transcript lines 9378-9406
- transcript lines 9411-9447

