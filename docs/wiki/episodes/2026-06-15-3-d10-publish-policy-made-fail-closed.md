---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: active
subjects:
  - d10-publish-policy
  - gift-wrap-privacy
  - publish-one-door
supersedes:
  - 2026-06-15-1-private-fail-closed-publish-enforcement-moved
related_claims: []
source_lines:
  - 3250-3278
  - 3386-3405
captured_at: 2026-06-15T16:41:53Z
---

# Episode: D10 publish-policy made fail-closed; refused rows terminally finalized

## Prior State

D10 publish policy was declared but unenforced at the dispatch-emit site. Gift-wrap events (kind:1059) could leak to public relays. When dispatch_due refused all relays for a row, the row was left Pending in the durable store — re-refused on every resume_from_store call, never terminally settled.

## Trigger

Codex review across 4 rounds found the privacy leak (gift-wrap to public relay), the resume-bypass (refused rows left Pending), and the lingering-row bug (refused rows never deleted from store).

## Decision

Make D10 fail-closed universally at the dispatch-emit site. Route every publish path through policy.rs (typed kind classification, no scattered literals). Extract finalize_completed_rows from tick/on_ack into a reusable method called after dispatch on every non-tick emit path (resume_from_store, retry_now, mark_relay_available, start_publish_inner). Refused rows are now terminally settled and deleted from durable store exactly once — never re-refused.

## Consequences

- Gift-wrap→public-relay privacy leak closed
- Refused rows never left Pending; never re-refused on resume
- No new finalization logic — extracted existing tick/on_ack path, reused identically
- Non-vacuity proven: disabling resume-path finalize_completed_rows fails the store-cleanup assertion
- 4 commits landed to master, 1573 tests pass

## Open Tail

*(none)*

## Evidence

- transcript lines 3250-3278
- transcript lines 3386-3405
