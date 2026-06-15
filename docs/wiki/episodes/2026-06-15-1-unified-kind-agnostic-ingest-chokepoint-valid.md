---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - ingest-chokepoint
  - should-store-event
  - ephemeral-observer-gate
  - local-publish-echo
  - adr-0057
supersedes:
  - 2026-06-15-1-ingest-admission-collapses-to-valid-signature
  - 2026-06-15-2-ephemeral-events-must-reach-app-observers
related_claims: []
source_lines:
  - 1-31
  - 62-144
  - 126-148
  - 378-395
  - 1397-1418
  - 1427-1493
  - 1497-1523
  - 1645-1687
  - 1688-1718
  - 1770-1783
  - 1879-1919
captured_at: 2026-06-15T11:57:07Z
---

# Episode: Unified kind-agnostic ingest chokepoint: valid-sig admission, universal delivery, persistence-only-for-non-ephemeral

## Prior State

Two hand-maintained per-kind per-source ingest ladders (relay `match event.kind` arms + `record_local_publish_intent` mirror). `should_store_event` framed as a store admission gate in ADR-0042, checking `timeline_authors.contains(pubkey)` — incoherently gating only kind:1/6 while kind:0/3/7/10002 were already admitted on valid-sig alone. Ephemeral events reached NIP parsers but NOT `KernelEventObserver` (latent bug: wildcard arm's notify gate `Inserted|Replaced` excluded `Ephemeral`). Non-replaceable local-publish events (kind:1/6/7) were invisible to the app until a relay echoed them back (ghost-post UX gap, issue #1440). The `acquisition-match` admission check was proposed as part of the initial fix.

## Trigger

Three converging triggers: (1) Issue #1440 — ghost-post UX where non-replaceable local publishes don't echo optimistically. (2) User directive rejecting acquisition-match admission: 'a relay can't fake data (signatures) and if it sends us something we didn't ask for then so fucking what? it's not shown anywhere in the app anyway' — signatures prevent forgery, read-time projections hide unsolicited events, DoS is a transport-layer quota concern. (3) User correction that ephemeral events must reach app observers: 'non-ephemeral is just for the cache part, but applications must (obviously) be able to use ephemeral events.'

## Decision

One kind-agnostic ingest chokepoint (`verify_and_persist` with `notify_event_observers` pulled inside), replacing the two per-kind per-source ladders. Admission = valid signature only; acquisition-match is deleted (not generalized). Three concerns cleanly separated: admission (valid sig), delivery (dispatch to NIP parsers + notify to KernelEventObserver on `Inserted|Replaced|Ephemeral` — fixing the ephemeral-observer gap), and persistence (store-insert only for non-ephemeral canonical outcomes). `should_store_event` demoted from store admission to a read-time timeline-view projection predicate (amending ADR-0042). `record_local_publish_intent`, `local_publish_intent.rs`, and `pre_kind3_buffer` are deleted; local publish enters via `local://publish` provenance through the same chokepoint. DoS mitigation is explicitly a transport-layer concern, not ingest.

## Consequences

- Ghost-post gap (issue #1440) is solved: non-replaceable local-publish events now hit the same chokepoint and reach observers immediately without relay round-trip.
- Latent ephemeral-observer bug surfaced and fixed: `Inserted|Replaced|Ephemeral` gate replaces `Inserted|Replaced`, so ephemerals reach app-facing KernelEventObserver for the first time.
- Workstream B (acquisition one-door) is decoupled from PR 1: admission no longer depends on InterestRegistry being single-door, so B is purely a DRY/maintainability cleanup, not a correctness prerequisite.
- Publish-in-flight pinning is a new PR-1 requirement: locally accepted publish events must be pinned in the LRU until relay confirmation, since they're now persisted unconditionally but could be evicted before echo.
- D9 created_at clamp stays in the timeline observer (not the generic chokepoint), since it only applies to kind:1/6 feed projection.
- Duplicate outcomes must still bump cached `relay_count` (timeline.rs:143) while remaining projection-silent to preserve D4 single-fire for relay echoes.
- ADR-0042 corrected in place (wrong `should_store_event` admission framing removed, not annotated as superseded).
- Three durable docs (subsystems.md, 08-eventstore.md, 12-publish-and-ledger.md) verified clean — wrong framing was only ever in ADR-0042 and code.
- ADR-0057 drafted, codex-reviewed twice, at clean SHIP. Not yet committed.
- Plan scope narrowed to event-flow trinity (A/B/C + F gates); D/E (capability/lifecycle authority) spun to sibling plan `arch-authority-lifecycle.md`.

## Open Tail

- PR 1 not yet started — requires the unified chokepoint implementation with publish-in-flight pinning and relay_count-on-Duplicate preservation.
- Full D0 (zero kind literals) only lands after PR 3; PR 1 only removes the relevance persistence gate (partial D0).
- Workstream tracking issues for B/C/F not yet filed.
- v1 vs post-v1 scope reconciliation with docs/plan.md not yet done.

## Evidence

- transcript lines 1-31
- transcript lines 62-144
- transcript lines 126-148
- transcript lines 378-395
- transcript lines 1397-1418
- transcript lines 1427-1493
- transcript lines 1497-1523
- transcript lines 1645-1687
- transcript lines 1688-1718
- transcript lines 1770-1783
- transcript lines 1879-1919
