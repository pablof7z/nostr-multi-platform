---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-ffi
  - embed-sidecar
  - embedhost-migration
  - claimed-event-embeds
supersedes:
  - 2026-06-13-2-d0-thin-shell-violation-resolved-embed
related_claims: []
source_lines:
  - 8871-8897
  - 8915-8926
captured_at: 2026-06-13T20:42:49Z
---

# Episode: EmbedHost resolution moves from per-platform Swift to Rust FFI sidecar

## Prior State

Embed content resolution (kind-dispatch, profile metadata extraction, media extraction, tag decoding) was done per-platform in Swift (Gallery's EmbedHost.swift ~150 lines of resolver code); each consumer app maintained its own resolver

## Trigger

Issue #1283 (direction GO) plus the new breaking-change doctrine: start the EmbedHost→nmp-ffi migration now rather than scheduling it

## Decision

Move embed resolution into nmp-ffi as a claimed_event_embeds sidecar — a listener thread reads KCEV FlatBuffer frames, resolves via nmp_content::resolve_embed_projection, and stores a pre-resolved primary_id→EmbeddedEventEnvelope map in a shared slot. Gallery iOS resolver deleted; Chirp iOS scaffolded for follow-up

## Consequences

- Phase 0 landed (#1333): Rust embed_sidecar.rs (320 LOC) + Gallery iOS EmbedHost.swift resolver deleted (~150 LOC), replaced by 5-line update(claimedEventEmbeds:)
- Chirp iOS needs a typed FlatBuffer sidecar (its frame schema lacks JSON payload) — tracked in #1335
- Android gallery needs decode wiring — tracked in #1335
- External consumers need git-rev bump + new decode path (per the breaking-change doctrine, upgrade by hand)
- lib.rs hit file-size cap; resolved by extracting install_embed_sidecar_projection() into embed_sidecar.rs

## Open Tail

- #1335 tracks remaining phases: Chirp typed-sidecar swap, Android gallery decode wiring

## Evidence

- transcript lines 8871-8897
- transcript lines 8915-8926

