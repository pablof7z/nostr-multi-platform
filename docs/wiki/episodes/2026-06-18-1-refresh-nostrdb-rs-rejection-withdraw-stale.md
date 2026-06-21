---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: superseded
subjects:
  - nostrdb-rs-evaluation
  - nostrdb-notedeck-lessons
  - eventstore-lmdb
  - lmdb-architecture
supersedes:
  - 2026-06-18-1-nostrdb-rs-rejection-reaffirmed-on-corrected
related_claims: []
source_lines:
  - 49-301
captured_at: 2026-06-18T18:13:32Z
---

# Episode: Refresh nostrdb-rs rejection: withdraw stale visitor objection, add license blocker, clarify D8 blocker

## Prior State

The nostrdb-rs evaluation rejected direct adoption, citing (among other reasons) that the Rust binding exposed only materialized Vec queries with no visitor path — forcing per-recompute allocation incompatible with D8's zero-per-event-alloc requirement. The lessons doc had a preliminary recommendation leaning toward adopting nostrdb-rs ('Option A is probably right').

## Trigger

Issue #1515 identified that current nostrdb-rs upstream now exposes fold/try_fold visitor semantics, making the original visitor objection factually stale. A second-pass review also identified an unresolved GPL-3.0-or-later vs BSD licensing ambiguity that independently blocks code-level borrowing.

## Decision

Direct nostrdb-rs adoption remains rejected, but on corrected grounds: (1) the visitor/allocation objection is withdrawn — fold/try_fold now exist in upstream; (2) the real D8 blocker is restated as NMP's composite reverse index naming interested views per insert, which is inexpressible in nostrdb's flat filter-poll subscription model; (3) a new §5b license checkpoint blocks all code-level borrowing until upstream clarifies GPL vs BSD; (4) the lessons doc's preliminary 'adopt nostrdb-rs' lean is superseded with a banner pointing to the evaluation. NMP adopts nostrdb design concepts (visitor scans, packed layouts, single-writer discipline) but not the crate dependency.

## Consequences

- The stale visitor-unboundedness claim is removed from three locations in the evaluation doc (§1, §4, §6), eliminating a false architectural objection from the record
- A new independent rejection reason (license ambiguity) is added as decisive reason #4, gating any code-level borrowing regardless of future architectural fit
- The D8 incompatibility is now correctly scoped: it's about composite reverse index / view-naming wake semantics, not about Vec materialization
- Follow-up implementation work is tracked as concrete GitHub issues (#1516–#1524) instead of prose aspirations
- The lessons doc no longer presents an adoption lean — future readers see a superseded banner and are directed to the evaluation doc

## Open Tail

- Upstream license clarification (GPL-3.0-or-later vs BSD) is unresolved and blocks code-level borrowing
- The remaining 8 sub-issues of epic #1523 (baselines, streaming query_visit, provenance indexes, projection sidecars, event-driven wakeups, diagnostics, acceptance gates) are queued but not yet implemented

## Evidence

- transcript lines 49-301

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-refresh-nostrdb-rs-rejection-withdraw-stale.json`](transcripts/2026-06-18-1-refresh-nostrdb-rs-rejection-withdraw-stale.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-refresh-nostrdb-rs-rejection-withdraw-stale.json`](transcripts/raw/2026-06-18-1-refresh-nostrdb-rs-rejection-withdraw-stale.json)
