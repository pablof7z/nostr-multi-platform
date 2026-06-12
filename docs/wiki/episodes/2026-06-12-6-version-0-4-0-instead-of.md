---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: architecture
status: active
subjects:
  - versioning
  - c-abi-stability
  - release
supersedes: []
related_claims: []
source_lines:
  - 3835-3944
  - 3956-3966
captured_at: 2026-06-12T00:59:06Z
---

# Episode: Version 0.4.0 instead of 0.3.1 — C-ABI break forces major bump

## Prior State

The release was originally planned as v0.3.1 (a patch-level bump for fixes)

## Trigger

The legacy `{1,6}` surface deletion removed four `nmp_app_*` C-ABI symbols from `nmp-ffi` and `NmpCore.h`, which is a breaking change for any rev-pinned consumer on v0.3.x

## Decision

Bumped to v0.4.0 instead. CHANGELOG carries a `### Removed (BREAKING)` section with migration notes and the NIP-01-filter pattern. Android consumers must skip v0.3.0 entirely (it shipped completely dark).

## Consequences

- C-ABI consumers on v0.3.x must migrate before pinning v0.4.0
- Android consumers must skip v0.3.0 and pin v0.4.0 directly (v0.3.0 was dark)
- All downstream repos (nmp-feedback, hl, podcast-player) lockstep-bumped to the new rev

## Open Tail

*(none)*

## Evidence

- transcript lines 3835-3944
- transcript lines 3956-3966

