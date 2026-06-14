---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: superseded
subjects:
  - android-projection-cache-d3-4
  - cross-platform-decode-before-commit-parity
supersedes: []
related_claims: []
source_lines:
  - 9248-9350
captured_at: 2026-06-14T10:26:44Z
---

# Episode: Android D3-4 decodeSucceeds parity doctrine — isNotEmpty() ruled acceptable (R3-S4)

## Prior State

Android's ProjectionCache used bytes.isNotEmpty() for decodeSucceeds instead of iOS's per-key typed-decoder preflight, appearing to be a semantic D3-4 (decode-before-commit) violation that could let corrupt non-empty payloads through to the UI on Android but not iOS.

## Trigger

Architectural concern raised during review of PR #1410 about per-platform divergence in the corrupt-payload safety guarantee, specifically whether Android's weaker check broke the no-corrupt-UI invariant.

## Decision

Ruled that isNotEmpty() is an acceptable D3-4 realization. The divergence is illusory: both platforms use unchecked FlatBuffers getRoot, so iOS's per-key decoder preflight also does NOT reject non-empty garbage (the iOS test authors explicitly concede this in-code). The only deterministic failure both platforms catch is the empty-payload Changed row, which isNotEmpty() handles identically. Android's re-decode path fail-closes via try/catch + FlatBuffers identifier check, so corrupt slots default and self-heal on the next good rev.

## Consequences

- No need for a uniform decodeBytes() interface across Android typed decoders before shipping
- D3-4 is honored to the same degree on both platforms — documented as accepted in the codegen template comment
- Future cleanup to add decodeBytes() remains legitimate but non-blocking
- Added corrupt-non-empty Android test recommended to pin the guarantee against regression (NIT, not blocking)

## Open Tail

- Add Android corrupt-non-empty test mirroring iOS test 12; fix unreachable init-time app pointer leak in nmp-android-ffi error path

## Evidence

- transcript lines 9248-9350

