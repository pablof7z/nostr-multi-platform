---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: superseded
subjects:
  - nostrdb-rs-adoption
  - event-store-backend
  - d8-doctrine
supersedes:
  - 2026-06-18-1-refresh-nostrdb-rs-rejection-withdraw-stale
related_claims: []
source_lines:
  - 1-367
captured_at: 2026-06-18T18:23:36Z
---

# Episode: Nostrdb-rs rejection restated on corrected grounds: visitor API exists, real blockers are composite-reverse-index mismatch and GPL/BSD licensing ambiguity

## Prior State

nostrdb-rs was rejected for M3 LMDB backend partly because the Rust binding allegedly exposed only materialized Vec queries (no visitor/early-stopping path). The decision doc claimed ndb_query_visit was 'unbound in nostrdb-rs', forcing Vec materialization on every recompute, which violated D8's zero-per-event-alloc visitor mandate.

## Trigger

Issue #1515 identified that current upstream nostrdb-rs now exposes fold / try_fold on query iterators, making the visitor-allocation objection stale. A second-pass review also found an unresolved licensing ambiguity (Cargo.toml says GPL-3.0-or-later; C repo appears BSD) that independently blocks code-level borrowing.

## Decision

The overall rejection of nostrdb-rs stands, but on corrected grounds: (1) the visitor/allocation objection is formally withdrawn — fold/try_fold make early-stopping zero-full-buffer scans expressible; (2) the real D8 blocker is restated as composite reverse-index incompatibility — NMP's per-insert named-view wakeup mechanism has no expression in nostrdb's flat filter-poll subscription model (subscribe + poll_for_notes); (3) a new decisive reason #4 is added: GPL-3.0-or-later vs BSD licensing ambiguity blocks code-level borrowing independently of technical fit; (4) the preliminary 'Option A is probably right' lean in the lessons doc is marked superseded with a Resolved banner. NMP continues with hand-rolled LmdbEventStore targeting nostr-lmdb per ADR-0011, adopting nostrdb concepts but not the dependency.

## Consequences

- Concepts from nostrdb (visitor semantics, packed layout, single-writer discipline, provenance indexes) are tracked as concrete sub-issues #1516–#1521, not left as prose recommendations
- Code-level borrowing/copying from nostrdb or nostrdb-rs is blocked until the GPL-vs-BSD ambiguity is resolved with upstream
- D8 doctrine interpretation shifts: the incompatibility is not about allocation from Vec materialization but about NMP's composite reverse-index wake model being inexpressible in nostrdb's subscription API
- Baseline capture (#1522) must precede any performance-affecting PRs like #1516's true-streaming query_visit

## Open Tail

- Upstream license clarification (GPL-3.0-or-later vs BSD) needed to lift the code-borrowing gate (#1524)
- True streaming LMDB query_visit implementation (#1516) will change the current materialize-then-visit behavior that #1522 baselines

## Evidence

- transcript lines 1-367

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-nostrdb-rs-rejection-restated-on-corrected.json`](transcripts/2026-06-18-1-nostrdb-rs-rejection-restated-on-corrected.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-nostrdb-rs-rejection-restated-on-corrected.json`](transcripts/raw/2026-06-18-1-nostrdb-rs-rejection-restated-on-corrected.json)
