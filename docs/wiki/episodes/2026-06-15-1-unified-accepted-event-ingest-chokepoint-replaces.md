---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - ingest-chokepoint
  - admission-gate-demotion
  - ephemeral-observer-fix
  - d9-clamp-preservation
  - pre-kind3-buffer-deletion
supersedes:
  - 2026-06-15-1-unified-ingest-chokepoint-replaces-dual-ladder
  - 2026-06-15-2-d9-timestamp-clamp-relocated-to-chokepoint
related_claims: []
source_lines:
  - 1-148
  - 2349-2372
captured_at: 2026-06-15T14:00:30Z
---

# Episode: Unified accepted-event ingest chokepoint replaces dual ladders (ADR-0057)

## Prior State

Two separate ingest ladders existed: (1) `record_local_publish_intent` for local publishes, which only had per-arm mirrors for replaceables (kind:0/3/10002) — kind:1/6/7 had no local echo arm, causing the ghost-post UX; (2) relay ingest via `handle_event` dispatching to `ingest_timeline_event` or wildcard arm. `should_store_event` (`timeline.rs:299`) was a persistence gate: its primary clause `timeline_authors.contains(author)` dropped non-followed-author events before `store.insert`, creating permanent relevance-shaped holes in the authoritative store. Ephemeral events reached NIP parsers but not app observers (wildcard `notify_event_observers` only fired on `Inserted|Replaced`). D9 future-date clamp existed only in the timeline ingest path. `pre_kind3_buffer` parked events before kind:3 processing.

## Trigger

Issue #1440 (ghost post — no optimistic echo for non-replaceable kind:1/6/7 events) + Issue #1442 (persistence holes from relevance-gated admission). Code investigation revealed the issue's suggested fix had two critical flaws: (a) calling both `verify_and_persist` and `ingest_timeline_event` caused double store-insert and double dispatcher fan-out; (b) after `ingest_timeline_event` (which already fires observers at `timeline.rs:252`), calling `notify_event_observers` again violated the D4 single-fire invariant. The real crux: `should_store_event` checks `timeline_authors.contains(author)` — a user is normally not in their own follow set, so a self-authored note would be dropped at the persistence gate, making a naive reuse of `ingest_timeline_event` silently fail for fresh root notes.

## Decision

Single `ingest_accepted_event(provenance, event)` chokepoint replaces both ladders. Admission is based on valid-signature only (not relevance). `should_store_event` demoted from persistence gate to read-time projection predicate. Observer notification gate expanded to `Inserted|Replaced|Ephemeral` (fixing latent ephemeral observer bug). D9 future-date clamp preserved specifically in the timeline observer (not in the generic chokepoint path, since `kernel_event_from_nostr` does not clamp). `record_local_publish_intent` ladder deleted entirely. `pre_kind3_buffer` deleted (complete store enables backfill via cache-serve replay instead of volatile buffer). New publish-in-flight GC pin source added to `derive_store_pin_set` to prevent eviction of unconfirmed local publishes.

## Consequences

- Read-your-writes works for all event kinds (kind:1/6/7 now get immediate local echo before any relay ACK)
- Non-followed-author events persist in store but do NOT project to the follow-feed view (persistence ≠ relevance)
- D4 single-fire invariant preserved: relay echo enters same chokepoint, store returns Duplicate, no re-notification
- Cold restart rebuilds all projections losslessly from the complete store (no relevance-shaped holes)
- Ephemeral events reach app observers but are never persisted or re-served after restart
- Timeline `relay_count` bump on Duplicate preserved as diagnostic signal
- Non-followed cold events are GC-reapable (unpinned class); in-flight publishes are pinned until relay confirmation
- Self-authored notes no longer rejected by follow-set gate — valid-sig admission bypasses relevance check

## Open Tail

- PR 2 (profiles→capability seam) and PR 3 (contacts→parser) still pending — profile/contacts kernel arms remain, to be migrated to IngestParser pattern
- Workstream F (doctrine gates banning store.insert/notify outside chokepoint) not yet implemented
- Stress harness not yet built — 13 areas / ~30 scenarios defined but not exercised against real code
- Sibling D (signer/capability authority) largely pre-empted by v0.7/v0.8 keystones; residual `active_local_keys` narrowing needs precise diff

## Evidence

- transcript lines 1-148
- transcript lines 2349-2372
