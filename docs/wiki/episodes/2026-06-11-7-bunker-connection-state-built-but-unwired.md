---
type: episode-card
date: 2026-06-11
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: product
status: active
subjects:
  - bunker-connection
  - v-14
  - typed-projections
supersedes: []
related_claims: []
source_lines:
  - 3489-3520
captured_at: 2026-06-11T23:31:21Z
---

# Episode: Bunker connection state — built-but-unwired V-14 fixed

## Prior State

V-14 violation: bunker connection state symbols existed in code but no real state transitions reached the UI — 'built but unwired'.

## Trigger

Bug #963 — users had no visibility into bunker connection status.

## Decision

Full typed FlatBuffers pipeline: new `bunker_connection_state.fbs` schema → Rust codec → FFI → actor slot → typed sidecar → UI on both iOS (green/amber/red BunkerConnectionStateRow) and Android (StateFlow + Composable).

## Consequences

- Users see real connection transitions (connected/reconnecting/failed) on both platforms
- Reviewer verified emission chain end-to-end (Pool → broker → FFI → actor → slot → sidecar → UI)
- Follow-up #1099: move status label+tone into Rust for ADR-0032 conformance

## Open Tail

- #1099 — label/tone synthesis should move to Rust side

## Evidence

- transcript lines 3489-3520

