---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: superseded
subjects:
  - ios-projection-cache
  - kernel-bridge
  - decode-path
supersedes: []
related_claims: []
source_lines:
  - 8988-9007
  - 9023-9105
  - 9106-9111
captured_at: 2026-06-14T09:35:38Z
---

# Episode: iOS ProjectionCache single-decode architecture

## Prior State

KernelBridge.swift was deliberately FlatBuffers-free. The initial S3 implementation added a double-decode block (re-parsing the entire buffer via ByteBuffer/getRoot every 4Hz tick) to read session_id and snapshot_epoch for the cache merge, which required importing FlatBuffers into a file that previously had no such dependency.

## Trigger

Opus review found the iOS app does not compile (ByteBuffer/getRoot not in scope) and identified the double-decode as reintroducing per-frame O(buffer) waste — the exact class of waste the ADR-0055 ladder exists to eliminate.

## Decision

Thread session_id and snapshot_epoch out of the existing single KernelUpdateFrameDecoder decode (add them to the .snapshot case alongside schemaVersion), passing them into cache.merge. Eliminates the second parse and keeps KernelBridge FlatBuffers-free.

## Consequences

- no per-tick buffer re-parse on the iOS host path
- layering invariant preserved: KernelBridge stays FlatBuffers-free
- session/epoch become first-class outputs of the decode path rather than extracted via side-channel re-parse
- S3 must be re-implemented with this architecture; fix dispatched but not yet landed

## Open Tail

- S3 implementation round 2 in progress; must verify iOS build + ChirpTests pass

## Evidence

- transcript lines 8988-9007
- transcript lines 9023-9105
- transcript lines 9106-9111

