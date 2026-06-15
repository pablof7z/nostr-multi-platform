---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: reversal
status: superseded
subjects:
  - profiles-ownership
  - contacts-ownership
  - pr-scope
supersedes: []
related_claims: []
source_lines:
  - 996-1052
  - 1190-1198
captured_at: 2026-06-15T10:42:44Z
---

# Episode: Profiles and contacts caches stay kernel-owned in initial PR; migration to parser ownership deferred to PR 2/3

## Prior State

The initial plan proposed demoting profile and contacts caches to parser/observer ownership (plan step 4) as part of the same atomic PR as the ingest chokepoint fix, killing all kind-match arms in one shot.

## Trigger

Code research revealing that ingest_contacts drives kernel-owned planner/lifecycle effects (CompileTrigger, sync_follow_feed_interests, timeline_authors rebuild, pre_kind3 flush, cache-serve — contacts.rs:245-258) that an IngestParser structurally cannot reach (it receives only VerifiedEvent with no &mut self or active_account). Additionally, seed_contacts has a non-ingest writer at sign-in (identity.rs:1032). Owner directive: multiple PRs acceptable, zero deferred debt, one plan reaching the full endpoint.

## Decision

Descoped profiles and contacts parser migration from the core ingest fix PR. PR 1 keeps profiles/contacts as kernel-owned steps called by the chokepoint (gated on Inserted|Replaced). PR 2 migrates profiles to ProfileLookup capability seam + parser ownership. PR 3 migrates contacts with a kernel-owned effect-signal seam for the planner side effects. End state remains zero kind literals in the ingest path.

## Consequences

- PR 1 stays atomic for the bug fix (#1440/#1442) without taking on planner-coupling risk
- The contacts effect-signal seam (CompileTrigger, sync_follow_feed_interests, timeline_authors rebuild) must be designed as a kernel-owned interface the parser can signal — not a parser-owned mutation
- Profiles ~10 synchronous readers (including hot-path profile_for_pubkey) must route through a ProfileLookup capability trait before migration
- seed_contacts sign-in writer (prepopulate_seed_contacts) must be rerouted through the parser-owned cache in PR 3

## Open Tail

- PR 2 and PR 3 designs need their own ADR supplements
- The contacts effect-signal seam pattern may become reusable for other parser→kernel side-effects

## Evidence

- transcript lines 996-1052
- transcript lines 1190-1198
