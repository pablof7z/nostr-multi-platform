---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - verify-and-persist
  - project-accepted-event
  - ingest-chokepoint
  - cache-serve-fan-out
supersedes:
  - 2026-06-15-2-unified-post-store-fan-out-doctrine
related_claims: []
source_lines:
  - 3607-3700
captured_at: 2026-06-15T17:37:54Z
---

# Episode: Unified post-store fan-out via project_accepted_event

## Prior State

verify_and_persist was a monolithic chokepoint doing sig-verify → store.insert → raw-tap → EventIngestDispatcher fan-out → TTL AND firing observer notify — all in one function. Cache-serve replay path could not reuse the fan-out logic without also re-inserting into the store. Test support had a fake inject_profile direct-cache writer bypassing the genuine dispatcher path.

## Trigger

Profiles capability seam (PR 2) required cache-serve to run the same post-store fan-out (parser dispatch + transition sweep + observer notify) as live ingest, but cache-serve must never call store.insert (ADR-0045 preserved). The monolithic verify_and_persist made this impossible without code duplication or fake writers.

## Decision

Split verify_and_persist into persistence-only (returns InsertOutcome + VerifiedEvent) and a new shared helper project_accepted_event that owns all post-store concerns kind-agnostically: (1) NIP-parser dispatch, (2) transition sweep (mailbox, dm-relay, profiles_ver), (3) D9 future-created_at clamp on observer KernelEvent, (4) notify_event_observers. Two call sites: ingest_accepted_event (live) and feed_served_event (cache-serve replay). Fake test writers replaced by genuine dispatcher-path routing through project_accepted_event.

## Consequences

- Both live ingest and cache-serve replay use identical projection logic — no behavioral divergence possible
- cache-serve remains store.insert-free (grep-proven)
- Duplicate/Superseded/Tombstoned/Rejected outcomes do NOT project — preserving D4 single-fire for read-your-writes
- Ephemeral events project (reach parsers + observers) but are not stored
- Test inject_replaceable_event and inject_profile now route through genuine TestKind0Parser via project_accepted_event — no fake writer paths
- PR 1b D9 clamp test subsumed into the shared helper (PR 1b superseded)

## Open Tail

- Stale ADR-0057 and ingest/mod.rs comments corrected (verify_and_persist no longer fires dispatch+notify)

## Evidence

- transcript lines 3607-3700
