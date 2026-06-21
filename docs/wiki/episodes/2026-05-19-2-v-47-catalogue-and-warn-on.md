---
type: episode-card
date: 2026-05-19
session: 12b3f443-3c2d-4e47-976a-7f4ceab75343
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/12b3f443-3c2d-4e47-976a-7f4ceab75343.jsonl
salience: architecture
status: active
subjects:
  - escape-hatches
  - ffi-surface
  - doctrine-d0-d8
supersedes: []
related_claims: []
source_lines:
  - 287529-287661
captured_at: 2026-06-18T04:36:30Z
---

# Episode: V-47: Catalogue and warn on all framework escape hatches

## Prior State

Four FFI escape hatches existed without documentation or warnings: `register_raw_event_observer`, `inject_pre_verified_events`, `inject_signed_event_json`, and `NmpSnapshotProjector`. A Notes spike proved that 96 LOC of Swift using the raw-event tap could bypass D3 outbox routing, kernel-owned formatting, lifecycle gating, and codegen contracts — all without any indication to the caller that they were leaving the safe API.

## Trigger

V-47 backlog item: 'register_raw_event_observer gives FFI callers a lane that defeats all D1/D3/D5/D8 guarantees.' The Notes spike demonstrated the breach was real, not theoretical.

## Decision

Escape hatches are not removed (they serve legitimate internal/testing use cases) but are now explicitly named, documented, and warned against. (1) `raw_event_tap.rs` module doc now includes a 'Framework bypass' warning listing which doctrines each escape hatch circumvents. (2) New `docs/escape-hatches.md` catalogues all four bypass lanes with a decision tree for when each is appropriate. (3) `aim.md` §1 updated with the caveat: 'the framework guards the kernel; FFI callers can bypass its guarantees by registering raw taps — see escape-hatch doc.'

## Consequences

- Any future contributor adding a new FFI escape hatch must document it in escape-hatches.md and cross-reference the affected doctrines
- The aim.md north-star now explicitly acknowledges that the type-system guarantees have intentional perforations, rather than claiming impossibility
- V-47 marked DONE in BACKLOG
- Shell developers (SwiftUI/Compose) can now discover the bypass risk before using raw-event taps in production code

## Open Tail

- Whether escape-hatch registrations should emit a runtime log warning in debug builds
- Whether `inject_pre_verified_events` should be gated behind a `test-support` feature flag rather than being in the public ABI

## Evidence

- transcript lines 287529-287661

