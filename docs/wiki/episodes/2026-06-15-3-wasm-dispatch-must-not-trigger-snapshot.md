---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: architecture
status: active
subjects:
  - wasm-snapshot-on-claim
  - solidjs-remount-loop
  - web-dispatch-arm
supersedes:
  - 2026-06-15-3-wasm-dispatch-no-snapshot-on-claim
related_claims: []
source_lines:
  - 2778-2782
  - 3158-3177
captured_at: 2026-06-15T09:49:18Z
---

# Episode: Wasm dispatch must not trigger snapshot projections on profile claims

## Prior State

The web/wasm dispatch arm triggered snapshot projections when profile claims were processed, same as other state changes. This caused no apparent issue before the profile-resolution overhaul because claims were rare (only profile-screen views).

## Trigger

After the profile-resolution fix expanded claim surfaces and added progressive re-resolution, the web feed entered an infinite SolidJS <For> component remount loop — each profile claim triggered a snapshot, which caused a re-render, which triggered more claims, ad infinitum. Caught as a CI regression during the v0.8.0 work.

## Decision

Established the no-snapshot-on-claim rule: web/wasm dispatch must not trigger snapshot projections when processing profile claims. Stated as a general rule for future dispatch arms to prevent reoccurrence.

## Consequences

- Web feed no longer enters infinite remount loops when profiles resolve progressively
- Future dispatch arms (e.g. new platform targets) must follow the no-snapshot-on-claim rule
- Profile resolution on web still works via the subscription/recompile path but without triggering a full state snapshot

## Open Tail

- Rule needs formal codification (e.g. in ADR or dispatch-arm checklist) beyond session documentation

## Evidence

- transcript lines 2778-2782
- transcript lines 3158-3177
