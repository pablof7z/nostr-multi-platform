---
type: episode-card
date: 2026-06-11
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: root-cause
status: active
subjects:
  - android-decoder
  - tier3-flatbuffers
  - ffi-surface
supersedes: []
related_claims: []
source_lines:
  - 3040-3137
captured_at: 2026-06-11T23:31:21Z
---

# Episode: Android dark-frame root cause — Tier-3 spine rebuild

## Prior State

`KernelUpdateFrameDecoder.kt` gated the entire decode on `snapshot.payload ?: return null`. PR-B (#1082) stopped emitting `payload:Value`, so every frame was silently dropped — app completely dark.

## Trigger

Bug #1084 — Android app rendered nothing after payload removal.

## Decision

All reads moved to Tier-3 FlatBuffers fields: `snapshot.rev`, `snapshot.running`, `snapshot.lastErrorToast` (Tier-3), `snapshot.metrics` via new `decodeMetricsFromTier3()`, `snapshot.relayStatuses` via new `decodeRelayStatusesFromTier3()`, typed sidecar projections only. No payload fallback.

## Consequences

- Android renders on v0.3.x
- New Kotlin FlatBuffers bindings for 19 Tier-3 fields plus 4 new types
- Golden test proving red→green with real Rust-generated fixture
- Previous review's blind spot (gating on payload) now documented in tier3_frame.rs

## Open Tail

- #1093 — Android golden fixture lacks Rust-side drift assertion; Kotlin/TS bindings drift gates still needed

## Evidence

- transcript lines 3040-3137

