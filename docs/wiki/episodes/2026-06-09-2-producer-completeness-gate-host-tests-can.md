---
type: episode-card
date: 2026-06-09
session: 63af4b96-d3d3-45c3-ab96-9f899beafa1b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/63af4b96-d3d3-45c3-ab96-9f899beafa1b.jsonl
salience: root-cause
status: active
subjects:
  - typed-projections
  - producer-completeness-gate
  - snapshot-registry
supersedes: []
related_claims: []
source_lines:
  - 6220-6558
captured_at: 2026-06-11T23:10:26Z
---

# Episode: Producer-completeness gate: host tests can't prove typed side is complete

## Prior State

The 162 ChirpTests pass through the JSON fallback, so they can't prove the typed side is complete — a typed-side omission would still produce correct results via the generic path, giving a false green

## Trigger

Advisor flagged the blind spot: host tests passing through JSON fallbacks give false-positive completeness assurance; also discovered that app and actor registries share one Arc<Mutex> (line 783-784), so introspecting NmpApp sees actor built-ins too

## Decision

Assert producer-side completeness: every non-null generic projection key must have a typed sidecar under the same key (json_keys ⊆ typed_keys). Null-valued generic keys are excluded (they carry no data the typed side needs to mirror — JSON-null and typed-absent both decode to nil). Test added as producer_completeness.rs.

## Consequences

- Fallback removal is now evidence-based, not optimistic — the gate is population-independent and runs in CI
- The wallet key false-positive was identified and excluded: generic emits Value::Null when disconnected but typed omits the key, both decoding to nil
- Shared Arc<Mutex> means the gate covers actor built-ins (bunker_handshake, nip46_onboarding) without additional work

## Open Tail

*(none)*

## Evidence

- transcript lines 6220-6558

