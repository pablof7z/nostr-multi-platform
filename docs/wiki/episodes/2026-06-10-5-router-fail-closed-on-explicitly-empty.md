---
type: episode-card
date: 2026-06-10
session: 8db7983d-2852-4213-9b8c-43650a958e7a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/8db7983d-2852-4213-9b8c-43650a958e7a.jsonl
salience: product
status: active
subjects:
  - nmp-router
  - publish-engine
  - nip-65
  - d3-doctrine
supersedes: []
related_claims: []
source_lines:
  - 573-600
captured_at: 2026-06-11T23:11:53Z
---

# Episode: Router fail-closed on explicitly empty NIP-65 write set

## Prior State

When a user's NIP-65 write set was explicitly empty, the publish engine still published to local/bootstrap relays, leaking events the user did not intend to broadcast.

## Trigger

Audit identified that the router fell through to local relays even when the user had opted out by having an empty write set. D3 doctrine says explicit targets should win.

## Decision

Router now returns an empty resolved set for explicitly empty write sets, mapping to `PublishEngineError::NoTargets` with a visible toast (fail-closed). The bootstrap/new-user case where no kind:10002 exists at all is preserved as a separate path.

## Consequences

- Users who explicitly publish an empty NIP-65 write relay list no longer have their events leak to bootstrap relays
- Bootstrap users (no kind:10002 on file) still get the fallback path
- Visible toast informs the user rather than silently dropping the publish

## Open Tail

*(none)*

## Evidence

- transcript lines 573-600

