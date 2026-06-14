---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: active
subjects:
  - d3-4-decode-before-commit
  - projection-cache
  - flatbuffers-safety
  - ios-android-parity
supersedes: []
related_claims: []
source_lines:
  - 9233-9246
  - 9248-9336
  - 9358-9380
captured_at: 2026-06-14T12:04:57Z
---

# Episode: D3-4 decode-before-commit parity: both platforms reject empty only

## Prior State

iOS ProjectionCache used a per-key typed-decoder preflight in `decodeSucceeds`, while Android used bare `bytes.isNotEmpty()`. This was believed to be a semantic divergence where iOS provided a stronger D3-4 (no-corrupt-UI) guarantee than Android.

## Trigger

The Opus review traced the corrupt-payload path end-to-end on both platforms and found that iOS's 'real decoder' preflight provides no additional protection: FlatBuffers `getRoot` is unchecked, so non-empty garbage bytes produce a default-valued struct (not nil) on both platforms. The iOS test authors explicitly concede this in-code (ProjectionCacheTests.swift:317-327).

## Decision

Android's `isNotEmpty()` is an acceptable realization of D3-4 because the effective guarantee on both platforms is 'reject empty-payload Changed rows only.' Non-empty corrupt bytes are committed on both platforms, then fail-closed on re-decode via Android's try/catch + identifier-check or iOS's equivalent path. The perceived divergence was illusory.

## Consequences

- D3-4 is understood as 'reject empty-payload Changed rows' on both platforms — no stronger guarantee is achievable with unchecked FlatBuffers getRoot
- Android's future `decodeBytes()` per-key probe is a cleanup, not a correctness fix
- An Android corrupt-payload regression test was added (ProjectionCacheTest nonEmptyCorruptPayloadFailsClosedThenSelfHeals) mirroring iOS test 12
- The init-time `app` pointer leak on Android's declare_incremental-apply error path was fixed (nmp_app_free before early return)

## Open Tail

- A shared uniform `decodeBytes()` interface on Android could provide stronger future guarantees if FlatBuffers validation improves

## Evidence

- transcript lines 9233-9246
- transcript lines 9248-9336
- transcript lines 9358-9380

