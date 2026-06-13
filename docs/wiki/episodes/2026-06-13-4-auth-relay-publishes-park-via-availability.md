---
type: episode-card
date: 2026-06-13
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: product
status: active
subjects:
  - auth-relay-publish
  - publish-availability-gate
supersedes: []
related_claims: []
source_lines:
  - 5030-5032
captured_at: 2026-06-13T19:22:03Z
---

# Episode: AUTH relay publishes park via availability gate instead of failing

## Prior State

Publish to AUTH-required relays burned a one-shot reauth budget within a 250ms tick, causing terminal failures for bunker accounts trying to publish.

## Trigger

Architecture review Finding B — publish to AUTH relays falsely terminal-failed.

## Decision

Park the publish via the existing availability gate (InFlight→Pending, unavailable_relays) and re-dispatch on Authenticated event, mirroring socket-reconnect wiring. Deleted dead Reauth/auth_required_max_retries machinery and stale M6 comment. No parallel mechanism per D8.

## Consequences

- AUTH relay publishes are event-driven, not polling
- Bunker accounts can successfully publish to AUTH relays
- State machine only signals (ParkAwaitingAuth); engine owns the gate mutation

## Open Tail

*(none)*

## Evidence

- transcript lines 5030-5032

