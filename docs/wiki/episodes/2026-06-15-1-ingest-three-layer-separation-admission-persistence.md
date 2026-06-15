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
  - local-publish-echo
  - timeline-admission
supersedes:
  - 2026-06-15-1-ingest-pipeline-inversion-separate-admission-persistence
related_claims: []
source_lines:
  - 5-19
  - 62-144
  - 670-697
  - 996-1051
  - 1163-1188
  - 1241-1266
captured_at: 2026-06-15T09:59:45Z
---

# Episode: Ingest three-layer separation: admission / persistence / relevance

## Prior State

`should_store_event` (`timeline.rs:299`) conflated three distinct questions into one gate: admission (did I solicit/author this?), persistence (keep it?), and relevance (show it in this view?). The follow-set clause (`timeline_authors.contains`) blocked self-authored notes and made persistence depend on social state. Non-replaceable events (kind:1/6/7) had no read-your-writes path — they only appeared after relay echo. `record_local_publish_intent` only handled replaceables; kind:1/6/7 were invisible locally until a relay echoed them back.

## Trigger

Issue #1440: published kind:1 notes/replies not echoed optimistically into local subscriptions. Investigation revealed the follow-set clause in `should_store_event` as the rogue relevance gate that drops self-authored notes, and that `ingest_timeline_event` would silently reject them (user not in own follow set).

## Decision

Separate the ingest pipeline into three distinct layers: (1) Admission — a source-based trust boundary (did I ask for or author this?), not kind or follow-set dependent; (2) Persistence — unconditional behind admission, the complete canonical log; (3) Relevance — a read-time, per-projection concern. Unify all sources (relay, local publish, replay) through a single kind-agnostic `ingest_accepted_event(source, event)` chokepoint placed below relay bookkeeping (`ingest/mod.rs:281/282`). Demote timeline-cache append to an observer. Replace the ad-hoc relevance gate with pin-aware LRU eviction as the single storage bound.

## Consequences

- Read-your-writes for ALL kinds — local publish hits the same chokepoint, no per-kind arm needed; closes #1440
- Complete store — no relevance-shaped holes; cache-serve/offline sound; projections rebuildable from store; cross-session dedup floor restored; closes #1442
- No drift — one ingest path; relay echo of local publish dedups to Duplicate, observers fire once (D4)
- `pre_kind3_buffer` deleted — it only existed to park events the entangled gate would drop
- Profile/contacts parser migration descoped to later PRs — `ingest_contacts` drives planner/lifecycle side effects (CompileTrigger, sync_follow_feed_interests, timeline_authors rebuild, pre_kind3 flush, cache-serve) that an `IngestParser` structurally cannot reach (no `&mut self`, no `active_account`); these stay kernel-owned steps fed by the chokepoint
- GC/pin-aware LRU is the principled bound replacing the relevance gate — newly-stored non-followed notes are exactly the unpinned class LRU evicts first; must-verify: truncation→LRU-skip fallback (`ram_eviction.rs:309-316`) doesn't regress into unbounded growth
- All milestone/coverage sites (`status.rs`, EOSE coverage) read read-caches not store counts — unaffected by persisting more events
- Doctrine gates must land with PR 1: ban `store.insert` outside ingest module, ban `notify_event_observers` outside the chokepoint — these enforce the invariant the keystone establishes
- The issue's suggested fix had two flaws confirmed: double store-insert + double dispatcher fan-out (calling both `verify_and_persist` and `ingest_timeline_event`), and double observer fire (both `ingest_timeline_event` and explicit `notify_event_observers`)

## Open Tail

- #1443: research unbounded local event cache — never lose fetched events; LMDB is memory-mapped (disk-bound, not RAM-bound), so unbounded durable store may be feasible with bounded RAM tier only; filed as research-only, deliverable is a report + recommended design
- PR 2: profiles → `ProfileLookup` capability seam + parser ownership (~10 synchronous readers to reroute through trait)
- PR 3: contacts → parser + kernel-owned effect-signal seam for planner side effects; end state = zero kind literals in ingest path (crate-boundaries §4.2 finish-line)
- ADR-0042 must stop framing `should_store_event` as store admission; builder-guide eventstore/publish docs need amendments

## Evidence

- transcript lines 5-19
- transcript lines 62-144
- transcript lines 670-697
- transcript lines 996-1051
- transcript lines 1163-1188
- transcript lines 1241-1266
