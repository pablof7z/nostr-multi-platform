---
type: episode-card
date: 2026-05-21
session: 1c093fa5-0f0e-4dee-bf38-99781e763f13
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1c093fa5-0f0e-4dee-bf38-99781e763f13.jsonl
salience: product
status: active
subjects:
  - publish-signed-event
  - kind-1059-guard
  - d10-doctrine
supersedes: []
related_claims: []
source_lines:
  - 3585-3619
  - 3951-3966
captured_at: 2026-06-18T04:41:52Z
---

# Episode: D10 guard refuses kind:1059 publish with empty relays

## Prior State

publish_signed_event would silently proceed when relays was empty for kind:1059 envelopes, potentially relying on cfg(test) Content-relay fallback as a backstop. No explicit refusal existed.

## Trigger

Codex review on PR #229 identified that kind:1059 could reach publish_signed_event with no relay targets, representing a broken-promise scenario where the dispatch correlation_id would never resolve.

## Decision

Defensive kernel-level guard at the top of publish_signed_event: when raw.kind == 1059 && relays.is_empty(), refuse with tracing::warn!, set D6 toast, record failed terminal verdict under the dispatch correlation_id (broken-promise fix), and return Vec::new() — no outbound frames, no publish-queue entry, envelope dropped.

## Consequences

- 5 new tests covering empty-slice refusal, empty-Vec refusal (the shape relays_for_target(&Auto) produces), non-1059 kinds continuing to Auto-route, kind:1059 + explicit pin happy path, and correlation_id broken-promise contract
- NmpApp::publish_signed_explicit docstring updated to remove kind:1059 inbox-routing approximation allowance and point to the kernel-side guard
- clear_relay_edit_rows_for_test seam added so tests can express truly-empty bootstrap independent of cfg(test) fallback
- D10 lint fixtures extended with positive dispatch fixture proving the lint catches publish_signed_event inside a marked dispatcher

## Open Tail

*(none)*

## Evidence

- transcript lines 3585-3619
- transcript lines 3951-3966

