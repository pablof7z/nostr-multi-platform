---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - unified-event-chokepoint
  - ingest-fan-out
  - cache-serve-projection
supersedes:
  - 2026-06-15-1-cache-serve-fan-out-unified-into
related_claims: []
source_lines:
  - 3607-3787
captured_at: 2026-06-15T17:04:02Z
---

# Episode: Unified post-store fan-out doctrine: project_accepted_event splits persistence from projection

## Prior State

verify_and_persist was a monolithic operation that did sig-verify → store.insert → raw-tap → EventIngestDispatcher dispatch → TTL stamp → notify_event_observers all in one function. Cache-serve replay had no parser dispatch path, causing stale capability caches. Test code used fake direct-cache writers (inject_profile).

## Trigger

Cache-serve replay couldn't repopulate capability caches (profiles stale after cold restart); the split paths risked D4 single-fire violations; fake writers bypassed the real dispatcher path.

## Decision

Extract Kernel::project_accepted_event as one shared helper owning three post-store concerns kind-agnostically: (1) NIP-parser dispatch, (2) transition sweep (mailbox/dm-relay/profile + byte-estimate invalidation), (3) D9 future-created_at clamp + notify_event_observers. verify_and_persist becomes persistence-only, returning (InsertOutcome, VerifiedEvent). Both live ingest (ingest_accepted_event) and cache-serve replay (feed_served_event) call the shared helper on the Inserted|Replaced|Ephemeral gate.

## Consequences

- Cache-serve now repopulates capability caches via the shared helper — cold-restart profile/relay-list caches are current
- No double-dispatch possible: parser dispatch + notify live ONLY in project_accepted_event
- Duplicate/Superseded/Tombstoned/Rejected are projection-silent, preserving D4 single-fire / read-your-writes
- Ephemeral projects but is not stored (un-stored is the correct behavior)
- Fake direct-cache writers deleted; test injectors now route through project_accepted_event → genuine registered parsers
- Cache-serve remains store.insert-free (grep-proven); reconstructs VerifiedEvent from already-verified store fields via from_store_verified_unchecked

## Open Tail

- Stale doc comments and ADR-0057 §chokepoint text required fixes (shipped as 0878227cb)

## Evidence

- transcript lines 3607-3787
