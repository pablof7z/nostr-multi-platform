---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: superseded
subjects:
  - ios-kernelbridge-double-decode
  - projection-cache-ios-interposer
supersedes: []
related_claims: []
source_lines:
  - 9026-9112
captured_at: 2026-06-14T10:26:44Z
---

# Episode: iOS double-decode per-tick waste eliminated — session/epoch threaded from single decode (R3-S3)

## Prior State

KernelBridge.swift re-parsed the entire FlatBuffers buffer a second time on every 4Hz frame just to read sessionId and snapshotEpoch (two scalars). This added O(buffer) per-tick work and required importing FlatBuffers into a file deliberately kept FlatBuffers-free. The app did not compile — the implementer never built or tested on iOS.

## Trigger

Opus review of PR #1409 found xcodebuild failed with 'cannot find ByteBuffer/getRoot in scope' because KernelBridge.swift had no import FlatBuffers. The reviewer identified the root cause as architectural: the double-decode block itself was wrong, not just the missing import.

## Decision

Thread sessionId/snapshotEpoch out of the single existing decode pass: the .snapshot case in KernelUpdateFrameDecoder became a 6-tuple carrying those two scalars read off the same frame.snapshot table, eliminating both the second buffer parse and the FlatBuffers dependency from KernelBridge.swift.

## Consequences

- Zero per-tick buffer re-parse — the exact per-frame waste the incremental ladder removes
- KernelBridge.swift stays FlatBuffers-free as designed
- Session/epoch available to the cache merge from the single decode — no redundant work
- This pattern became the template for Android (R3-S4) which also threads from the single decode
- Three implementer-written Swift tests had wrong expectations caught only because the suite was actually run this time

## Open Tail

*(none)*

## Evidence

- transcript lines 9026-9112

