---
type: episode-card
date: 2026-05-26
session: fa300009-e498-4c80-a2d3-64d1531a09d4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/fa300009-e498-4c80-a2d3-64d1531a09d4.jsonl
salience: product
status: active
subjects:
  - publish-outbox-status
supersedes:
  - 2026-05-26-1-pending-reaction-root-cause-status-priority
related_claims: []
source_lines:
  - 559-618
captured_at: 2026-06-18T05:46:49Z
---

# Episode: Publish outbox returns "queued" when at least one relay accepted

## Prior State

When at least one relay had returned Ok but other relays were still Pending, publish_outbox_status fell through to the Pending branch and returned "pending" — implying the event hadn't been published at all, misleading the user.

## Trigger

Review of the unstaged publish_outbox.rs change revealed a logic gap: the Ok branch was missing, so the function could never surface the intermediate state where the event was live but fanout was incomplete.

## Decision

Inserted a new branch before the Pending check: if any relay has PerRelayState::Ok, return "queued" instead of falling through to "pending". This correctly communicates that the event is published and secondary fanout is still in progress.

## Consequences

- Users will see "queued" instead of "pending" when an event is confirmed on at least one relay but fanout is ongoing
- The status string hierarchy is now: retrying → sending → queued → pending, matching actual relay state

## Open Tail

*(none)*

## Evidence

- transcript lines 559-618

