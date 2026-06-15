---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - ingest-chokepoint-unification
  - admission-persistence-relevance-separation
  - should-store-event-decoupling
supersedes:
  - 2026-06-15-1-unify-ingest-separate-persistence-from-admission
related_claims: []
source_lines:
  - 124-150
  - 590-652
  - 654-697
  - 995-1053
  - 1190-1238
captured_at: 2026-06-15T09:24:55Z
---

# Episode: Ingest pipeline inversion: separate admission/persistence/relevance into three layers

## Prior State

The ingest pipeline entangled three distinct concerns in one gate (`should_store_event` at `timeline.rs:299`): admission (did I ask for this?), persistence (should I store it?), and relevance (should I display it?). Kind-specific match arms in `handle_event` (`ingest/mod.rs:296-425`) routed events differently per kind, with `ingest_timeline_event` doing its own store-insert + sig-verify + observer-notify independently from `verify_and_persist`. Non-replaceable events (kind:1/6/7) had no read-your-writes echo — they were invisible until a relay echoed them back. A user's own note could be dropped because `timeline_authors.contains(author)` (the follow set) was the primary admission clause, and a user is normally not in their own follow set. The issue's suggested fix (call both `verify_and_persist` and `ingest_timeline_event`) would cause double store-insert and double observer fire, violating the D4 single-fire invariant.

## Trigger

Issue #1440 investigation — 'ghost post' UX where published kind:1 notes/replies are invisible until relay echo. Root-cause tracing revealed `record_local_publish_intent` hard-returns for non-replaceables (`local_publish_intent.rs:59-62`), and `should_store_event`'s follow-set clause blocks self-authored notes. The issue's proposed fix was found to have two technical flaws (double persist, double notify), and a deeper structural problem: the admission/persistence/relevance conflation means relevance-shaped holes in the store make cache-rebuild on restart unsound (e.g. DmRelayCache can't rebuild, DMs silently fail).

## Decision

Invert the pipeline into three kind-agnostic layers: (1) Admission — per-source trust boundary ('did I ask for this or make it?'), not kind or follow-set dependent; local publishes admitted unconditionally. (2) Persistence — a single chokepoint function `ingest_accepted_event(source, event)` that does verify → persist → dispatch-to-parser-registry → notify-observers, once, for every admitted event regardless of kind. (3) Relevance — moved to read-time, per-projection; `should_store_event` becomes a predicate the timeline-cache observer consults for display filtering, with no power over persistence. The store becomes a complete, GC-bounded log (pin-aware LRU, ~10k ceiling) of everything admitted; eviction — not admission — is the sole storage bound.

## Consequences

- Read-your-writes for ALL kinds — local publishes are admitted and flow through the same notify step; #1440 closed.
- Complete store — no relevance-shaped holes; cache-serve/offline sound, projections rebuildable, cross-session dedup floor restored; #1442 closed.
- No drift — one ingest path means relay echo of a local publish dedups to Duplicate; observers fire once (D4).
- pre_kind3_buffer deleted — only existed to park events the entanglement would have dropped.
- App-agnostic (D0) — persistence no longer assumes 'social'; third-party NMP app interests get stored.
- Profile/contacts caches stay kernel-owned in PR 1 (ingest_contacts drives planner/lifecycle side effects — CompileTrigger, sync_follow_feed_interests, timeline_authors rebuild — that an IngestParser structurally cannot reach); parser migration deferred to PR 2 (profiles) and PR 3 (contacts effect-signal seam).
- GC/pin-set correctness becomes the critical safety property — pin-aware LRU eviction replaces the relevance gate as the storage bound; the truncation→LRU-skip fallback (ram_eviction.rs:309-316) must be verified under persist-everything.
- Unbounded cache research filed as #1443 — LMDB is memory-mapped (disk-bound, not RAM-bound), so unbounded durable store with bounded RAM tier may be feasible.

## Open Tail

- ADR draft still needed (PR 0) before implementation begins.
- Profile/contacts parser migration (PR 2/PR 3) — contacts requires extracting planner/lifecycle effects into a kernel-owned effect seam the parser can signal; seed_contacts has a non-ingest writer at sign-in to reroute.
- Must-verify: pin-set correctness under persist-everything regime; truncation→LRU-skip fallback doesn't regress into unbounded growth.
- Follow-up #1443: research whether durable LMDB store can be unbounded (disk-paged) with only RAM read-caches bounded, eliminating the 10k event ceiling.

## Evidence

- transcript lines 124-150
- transcript lines 590-652
- transcript lines 654-697
- transcript lines 995-1053
- transcript lines 1190-1238
