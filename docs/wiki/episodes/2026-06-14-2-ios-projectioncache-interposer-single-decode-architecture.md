---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: active
subjects:
  - ios-projection-cache
  - kernel-bridge-decode
  - adr-0055-r3-s3
supersedes:
  - 2026-06-14-2-ios-double-decode-per-tick-waste
related_claims: []
source_lines:
  - 9023-9153
captured_at: 2026-06-14T10:37:17Z
---

# Episode: iOS ProjectionCache interposer — single-decode architecture, no per-frame buffer re-parse

## Prior State

The initial R3-S3 implementation added a second FlatBuffers buffer parse (ByteBuffer + getRoot) inside KernelBridge.decodeFlatBuffer every 4Hz frame to read session_id and snapshot_epoch. This required importing FlatBuffers into KernelBridge.swift (a deliberately FlatBuffers-free file) and reintroduced the exact per-frame O(buffer) waste the incremental-emission ladder exists to eliminate.

## Trigger

Opus review found the iOS app didn't compile — ByteBuffer and getRoot were not in scope because KernelBridge.swift had no import FlatBuffers. The review identified the deeper problem: adding the import would mask the architectural violation (double-decode on every frame) rather than fix it.

## Decision

Thread session_id and snapshot_epoch out of the single existing decode pass. The .snapshot case in KernelUpdateFrameDecoder became a 6-tuple carrying those two scalars (read off the same frame.snapshot table next to schemaVersion). The second buffer parse and the FlatBuffers import were both deleted. KernelBridge stays FlatBuffers-free and no per-frame re-parse occurs.

## Consequences

- iOS is the first host to realize the incremental-emission savings — ProjectionCache merge runs before typed decoders, and KernelModel.apply only updates changed slots (changedKeys gating)
- KernelBridge.swift remains deliberately FlatBuffers-free — the dependency boundary is preserved
- 13 real ProjectionCacheTests pass (14 after the added atomic-reset coverage test); all 179 ChirpTests pass
- Decode-before-commit (D3-4): on decode failure, needsResync is latched and the prior cache entry is untouched; the key does NOT enter changedKeys
- Finding-4 regression avoided: omitted projection slots retain their prior value via cache reinstatement, not blanking

## Open Tail

*(none)*

## Evidence

- transcript lines 9023-9153

