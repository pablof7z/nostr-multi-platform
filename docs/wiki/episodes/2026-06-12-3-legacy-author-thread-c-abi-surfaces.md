---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: reversal
status: active
subjects:
  - ffi-surface
  - open-interest
  - author-view
  - thread-view
supersedes: []
related_claims: []
source_lines:
  - 3622-3667
  - 3676-3687
  - 3717-3741
captured_at: 2026-06-12T00:59:06Z
---

# Episode: Legacy author/thread C-ABI surfaces replaced by generic interest seam

## Prior State

NMP had hardcoded `nmp_app_open_author`/`close_author`/`open_thread`/`close_thread` C-ABI symbols that carried the hardcoded Chirp social-kind default `{1,6}` inside the generic FFI layer (a D0 violation), driving a kernel-resident `AuthorViewState`/`ThreadViewState` state machine with `author_view` and `thread_view` typed projections

## Trigger

ADR-0042 and #958/#957 — the hardcoded kind-default surfaces were a doctrine violation, and the parallel state machine was redundant with the generic interest/feed engine

## Decision

Delete the four `nmp_app_*` C-ABI symbols, the `AuthorViewState`/`ThreadViewState` state machine, the `author_view` (KAVW) and `thread_view` (KTVW) typed projections, their FlatBuffers schemas (`author_view.fbs`, `thread_view.fbs`, `timeline_item.fbs`), and generated Swift readers. Projection registry drops from 36→34 keys (28 typed decode stubs). Migration: apps use `nmp_app_open_interest(filter_json, consumer_id, scope)` with verbatim NIP-01 filters, paired with `nmp_app_close_interest`. Profile hydration via `nmp_app_claim_profile`/`release_profile`. Version bumped to 0.4.0 (not 0.3.1) due to the C-ABI break.

## Consequences

- −5,750 LOC across 66 files
- C-ABI breaking change requires version 0.4.0; Android consumers must skip v0.3.0 entirely
- Open-view pin derivation had to be re-derived from `lifecycle.registry().iter_active()` + `shape.matches_event_with_id()` (the same predicate ingest uses)
- The 4 ram_eviction view-pin tests were migrated to `open_interest` seam (not deleted — they guard a live invariant)
- The `claimed_profiles` decode cluster was promoted to the public typed surface as part of this migration

## Open Tail

- ADR-0032 conformance for bunker label/tone in Rust (#1099)
- Shared membership predicate extraction for pin derivation

## Evidence

- transcript lines 3622-3667
- transcript lines 3676-3687
- transcript lines 3717-3741

