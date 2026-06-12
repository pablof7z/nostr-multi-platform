---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: root-cause
status: active
subjects:
  - claimed-profiles-visibility
  - typed-projections-surface
  - example-build-gap
supersedes: []
related_claims: []
source_lines:
  - 3736-3767
captured_at: 2026-06-12T00:32:21Z
---

# Episode: Claimed Profiles Decode Was Never Publicly Exported

## Prior State

claimed_profiles decode functions (decode_claimed_profiles, CLAIMED_PROFILES_SCHEMA_ID, ClaimedProfilesModel) were pub(crate) / #[cfg(test)] gated, never promoted to the public typed-projections facade despite other clusters (claimed_events, resolved_profiles) being public.

## Trigger

#1100's example migration imported these pub(crate) symbols, causing cargo test (which builds examples) to fail — a gap class that escaped both scoped cargo test runs and cargo build --workspace (neither builds examples).

## Decision

Promote claimed_profiles decode cluster to pub with the same pattern as decode_claimed_events. Add cargo test -p nmp-app-template and cargo build --workspace --examples to the validation playbook to close the gap class.

## Consequences

- claimed_profiles is now on the public typed-projections surface
- Example compilation is now a gate that would have caught this
- Android and chirp consumers were already using these via different import paths — nothing live was deleted by the cascade

## Open Tail

*(none)*

## Evidence

- transcript lines 3736-3767

