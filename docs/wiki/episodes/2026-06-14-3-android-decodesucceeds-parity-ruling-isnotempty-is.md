---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: superseded
subjects:
  - android-projection-cache
  - d3-4-decode-before-commit
  - cross-platform-parity
supersedes:
  - 2026-06-14-3-android-d3-4-decodesucceeds-parity-doctrine
related_claims: []
source_lines:
  - 9248-9347
captured_at: 2026-06-14T10:37:17Z
---

# Episode: Android decodeSucceeds parity ruling — isNotEmpty() is equivalent D3-4 to iOS per-key decoder preflight

## Prior State

The Android ProjectionCache implementation used decodeSucceeds = bytes.isNotEmpty() rather than calling each per-key typed decoder as a preflight (as iOS does). This appeared to be a semantic divergence from D3-4's decode-before-commit guarantee — iOS's per-key real-decoder switch seemed to provide stronger protection against non-empty corrupt payloads.

## Trigger

Opus adversarial review was tasked with ruling whether the isNotEmpty() shortcut broke D3-4 end-to-end on Android. The review traced the corrupt-payload path on both platforms and found that iOS's test authors explicitly concede (ProjectionCacheTests.swift:317-327) that unchecked FlatBuffers getRoot does NOT reliably reject non-empty garbage — an out-of-range offset yields an empty/default-valued struct, not nil. iOS test 12 commits Data([0x00]) and iOS also returns true for it.

## Decision

Android's isNotEmpty() is an acceptable realization of D3-4. Both platforms have the same effective floor: only empty-payload Changed is deterministically caught (which isNotEmpty() handles identically). The iOS per-key decoder switch is stronger only in theory, never in the reproducible path. Android's re-decode path provides equivalent fail-closed self-healing via try/catch + FlatBuffers identifier check in every typed decoder. No uniform decodeBytes() interface is needed for D3-4 compliance.

## Consequences

- The cross-platform D3-4 parity concern is closed — no architectural change needed on Android
- Both mobile hosts now realize the incremental-emission savings (iOS merged as 15c267cf9, Android pending with nits)
- NIT: an unreachable app-pointer leak on the declare_incremental_apply error path (nmp-android-ffi/lib.rs:67-77) should be fixed before merge
- NIT: a corrupt-non-empty Android test should be added mirroring iOS test 12, to pin the documented fail-closed behavior against regression

## Open Tail

- Future cleanup: a uniform decodeBytes() per-key interface for Android decoders (comment in kotlin_projection_cache.rs:88-90) — legitimate but not blocking

## Evidence

- transcript lines 9248-9347

