---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - ingest-admission-model
  - should-store-event
  - unified-chokepoint
  - pin-aware-lru
supersedes:
  - 2026-06-15-1-unified-kind-agnostic-ingest-with-valid
related_claims: []
source_lines:
  - 124-144
  - 968-1098
  - 1378-1418
captured_at: 2026-06-15T10:51:19Z
---

# Episode: Ingest admission collapses to valid-signature-only (relevance gate eliminated)

## Prior State

Store admission was gated by `should_store_event` (relevance: follow-set membership / active interest match). The original #1440 fix suggested reusing `ingest_timeline_event` for local echo, which would cause double-insert and double-observer violations. `should_store_event` would silently drop a user's own note not in their follow set. An acquisition-match check was proposed as the new admission predicate.

## Trigger

Multiple code-level findings converged: (1) reusing `ingest_timeline_event` causes double store-insert and double observer-fire (violating D4 single-fire invariant), (2) `should_store_event`'s `timeline_authors.contains(author)` rejects self-authored notes outside the follow set, (3) GC is actually wired in production (stale 'never called' finding corrected) and pin-aware LRU eviction already bounds storage, (4) user explicitly rejected acquisition-match as unnecessary complexity: signatures prevent forgery and read-time projections hide unsolicited events, (5) `ingest_contacts` drives planner/lifecycle side effects unreachable from an IngestParser, blocking the proposed per-kind parser migration.

## Decision

Admission = valid signature, period. No acquisition-match, no relevance gate at the store layer. `should_store_event` is deleted from the store layer and survives only as the timeline view's read-time predicate. Relay/local/replay all collapse to one kind-agnostic rule. Pin-aware LRU eviction (not relevance gating) is the sole storage bound — newly-stored non-followed notes are unpinned and evicted first.

## Consequences

- Dissolves the B→PR1 dependency: acquisition one-door is a DRY/maintainability cleanup, not a correctness prerequisite for the ingest fix
- PR 1 simplified: no source-specific or kind-specific admission logic in the unified chokepoint
- The unified `ingest_accepted_event(source, event)` body is just the `match event.kind` block from `ingest/mod.rs:296+` with relay-only bookkeeping (lines 252-281) staying outside
- ADR-0042 must stop framing `should_store_event` as store admission
- One must-verify caveat: `derive_store_gc_inputs` disables LRU when floor-coherent pin scan truncates (ram_eviction.rs:309-316); persist-everything must be tested under that condition
- Profile/contacts parser migration descoped to PR 2/3 because `ingest_contacts` drives kernel-owned planner side effects (CompileTrigger, sync_follow_feed_interests, timeline_authors rebuild)

## Open Tail

- #1443 research: can the durable LMDB store be unbounded (disk-backed) with only the RAM tier bounded? The current 10k ceiling deletes events from the device entirely.
- DoS via write-amplification: a hostile relay can sign garbage under throwaway keys. Mitigation is transport-level per-relay quotas, not an ingest admission gate — but not yet implemented.

## Evidence

- transcript lines 124-144
- transcript lines 968-1098
- transcript lines 1378-1418
