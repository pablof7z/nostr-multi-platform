---
type: episode-card
date: 2026-06-11
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: root-cause
status: active
subjects:
  - android-decoder
  - tier3-frame
  - adr-0044
supersedes: []
related_claims: []
source_lines:
  - 3040-3137
captured_at: 2026-06-11T23:22:45Z
---

# Episode: Android Tier-3 spine eliminates payload dependency

## Prior State

KernelUpdateFrameDecoder.kt gated the entire frame decode on snapshot.payload (line 104-107). When PR-B (#1082) stopped emitting payload:Value, every frame was silently dropped — the app went completely dark.

## Trigger

Root cause confirmed: the payload gate made every field unreachable when payload was null, a single point of failure that the #1074 review had missed.

## Decision

Moved all field reads to Tier-3 spine fields (snapshot.rev, snapshot.running, snapshot.lastErrorToast, snapshot.metrics via new decodeMetricsFromTier3(), snapshot.relayStatuses) and typed sidecars (no payload fallback). Added FlatBuffers bindings for all 19 ADR-0044 Tier-3 fields plus a real-frame golden test that fails on the broken decoder.

## Consequences

- Android rendering no longer depends on payload for any field
- Golden fixture verified byte-for-byte against Rust encoder output (1152 hex chars)
- Follow-up #1093 filed for missing Rust-side drift gate on the Android golden fixture

## Open Tail

- Kotlin bindings-drift gate deferred (no issue cited for .kt drift gate)
- Stale docstring at KernelModel.decodeUpdate:428 still references payload

## Evidence

- transcript lines 3040-3137

