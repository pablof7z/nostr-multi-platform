---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: active
subjects:
  - projection-cache-interposer
  - decode-before-commit
  - single-decode-doctrine
supersedes:
  - 2026-06-14-3-android-decodesucceeds-parity-ruling-isnotempty-is
related_claims: []
source_lines:
  - 9095-9112
  - 9248-9336
  - 9358-9379
captured_at: 2026-06-14T11:51:24Z
---

# Episode: ProjectionCache interposer: single-decode, both platforms, same D3-4 floor

## Prior State

iOS was assumed to have a stronger decode-before-commit guarantee via per-key typed-decoder preflight; session/epoch could be obtained by re-parsing the FlatBuffers buffer; Android's isNotEmpty() was suspected of being a weaker D3-4 floor; the two platforms' guarantees had not been traced end-to-end for corrupt payloads

## Trigger

iOS build failure revealed the implementer added a double-decode block (re-parsing FlatBuffers every 4Hz frame for two scalars) — the exact per-frame waste the ladder exists to kill. Opus review traced corrupt-payload paths on both platforms and found iOS's per-key decoder preflight does not actually reject non-empty garbage (FlatBuffers getRoot is unchecked, acknowledged in iOS test file lines 317-327 and 480-489)

## Decision

Thread session_id/snapshot_epoch from the single existing FlatBuffers decode pass on both platforms — never re-parse. Accept Android's isNotEmpty() as the same effective D3-4 floor: both platforms commit non-empty garbage and fail-closed via try/catch re-decode + identifier check, self-healing on the next good rev. The iOS per-key decoder switch is acknowledged as theater for the non-empty case; the only deterministic catch both platforms share is rejecting empty-payload Changed rows

## Consequences

- Both platforms have line-for-line semantically identical merge algorithms (rebaseline atomicity, reorder guard, Cleared handling, session-zero pass-through, changedKeys precision)
- No per-frame O(buffer) re-parse on either platform
- Corrupt non-empty payloads fail-closed to slot defaults and self-heal; no crash, no permanent stale
- Android decodeProjections promoted to internal for re-decode-from-merged-set pattern
- Non-empty corrupt-payload test added on Android mirroring iOS test 12 to pin the guarantee against regression
- Android init-time app pointer leak on declare_incremental_apply error path fixed (nmp_app_free before early return)

## Open Tail

- Optional: add uniform decodeBytes() interface to Android typed decoders so the per-key probe can be real rather than isNotEmpty() — currently deferred as cleanup, not a merge blocker

## Evidence

- transcript lines 9095-9112
- transcript lines 9248-9336
- transcript lines 9358-9379

