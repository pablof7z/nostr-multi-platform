---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - unified-ingest-chokepoint
  - should-store-event-demotion
  - optimistic-local-echo
  - adr-0057
supersedes:
  - 2026-06-15-1-unified-kind-agnostic-ingest-chokepoint-valid
related_claims: []
source_lines:
  - 1-31
  - 84-123
  - 124-161
  - 1615-1644
  - 1722-1783
  - 1879-1915
  - 2034-2071
  - 2157-2223
captured_at: 2026-06-15T13:38:06Z
---

# Episode: Unified kind-agnostic event ingest chokepoint replaces split pipeline

## Prior State

Non-replaceable events (kind:1/6/7) had no optimistic local echo — invisible until relay echo-back (the #1440 ghost-post bug). Replaceables got echo via `record_local_publish_intent`, but that function hard-returned for non-replaceables. `should_store_event` was a store-admission gate controlling persistence; a user not in their own follow set could have their own note silently dropped. The relay ingest had two arms (timeline vs wildcard) with inconsistent persistence/observer semantics. Ephemeral events never fired observers (latent bug). No publish-in-flight pinning existed for LRU eviction safety.

## Trigger

Issue #1440 investigation revealed the split pipeline root cause, plus deeper findings: (1) `should_store_event` as admission gate means self-authored notes can be silently dropped; (2) the two ingest arms have different observer-firing contracts; (3) ephemeral events never reach observers; (4) the suggested fix in the issue would cause double store-insert and double observer-fire. Codex reviews (two rounds) further surfaced: the D9 timestamp clamp must fire at the chokepoint observer (not just timeline), duplicates must remain projection-silent while preserving relay_count bump, and publish-in-flight pinning is required.

## Decision

ADR-0057: a single `ingest_accepted_event(IngestSource, event)` chokepoint at the `:281→282` seam. All events persist unconditionally via `verify_and_persist` (admission = valid signature only). `should_store_event` demoted to a read-time timeline-VIEW projection predicate with zero persistence authority. Delivery (dispatch+notify) fires exactly once on `Inserted|Replaced|Ephemeral` (fixing the ephemeral observer bug); `Duplicate|Superseded|Tombstoned|Rejected` are projection-silent but `Duplicate` still bumps relay_count. `record_local_publish_intent` and `pre_kind3_buffer` deleted. D9 future-dated clamp applied at the chokepoint observer fan-out AND the timeline read-cache. Publish-in-flight pin source wired to `publish_engine.snapshot().in_flight`. Both relay and local-publish sources route through the same chokepoint.

## Consequences

- Read-your-writes now works for all event kinds (kind:1/6/7 appear immediately in local timeline without relay round-trip)
- Admission/delivery/persistence three-way split formalized: admission=valid sig, delivery=gated by store outcome, persistence=non-ephemeral canonical outcomes only
- kind:1/6 re-routed through verify_and_persist (duplicate sig-verify and store-insert killed)
- Ephemeral events now reach protocol parsers and observers for the first time (latent bug fixed)
- Future-dated hostile events can no longer pin to the top of app feeds (D9 clamp at observer level)
- Locally-published events are pinned while in publish queue, preventing LRU eviction before relay confirmation
- should_store_event can never reintroduce a persistence gate (doc comment warns against it explicitly)
- pre_kind3_buffer deleted; backfill verified via cache-serve path
- ADR-0042 corrected to reflect that persistence is unconditional and should_store_event is read-time only
- FlatFeed/app feeds receive clamped timestamps; StoredEvent retains raw wire timestamp for provenance

## Open Tail

- PR 2 (ProfileLookup capability seam) and PR 3 (contacts parser+effect seam) depend on PR 1 chokepoint
- Workstream F gates (store.insert/notify_event_observers bans) to prevent dual-ladder regrowth
- NMP consumer app upgrades and version cut (outside monorepo scope)
- Auto-compiled wiki page `docs/wiki/kernel-timestamp-clamp.md` has stale 'where' clause but was deliberately left for compiler refresh
- Workstream B reconciliation — profile-claim/reverify migration may already be in `fix/profile-claim-registry-migration` branch

## Evidence

- transcript lines 1-31
- transcript lines 84-123
- transcript lines 124-161
- transcript lines 1615-1644
- transcript lines 1722-1783
- transcript lines 1879-1915
- transcript lines 2034-2071
- transcript lines 2157-2223
