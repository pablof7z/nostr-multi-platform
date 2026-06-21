---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: superseded
subjects:
  - nostrdb-rs-evaluation
  - store-cache
  - lmdb-backend
supersedes: []
related_claims: []
source_lines:
  - 133-143
  - 145-155
  - 157-179
  - 183-193
  - 197-213
  - 250-273
  - 275-285
captured_at: 2026-06-18T18:08:32Z
---

# Episode: Nostrdb-rs rejection reaffirmed on corrected grounds; visitor objection withdrawn, license blocker added

## Prior State

nostrdb-rs was rejected partly because the Rust binding exposed only materialized Vec queries with no visitor path (forcing per-recompute allocation), and the lessons doc carried a preliminary lean that Option A (adopt nostrdb-rs) was 'probably the right move for v1'

## Trigger

current upstream nostrdb-rs was found to expose fold/try_fold visitor semantics, invalidating the 'no visitor binding' rejection reason; additionally, nostrdb-rs Cargo.toml publishes GPL-3.0-or-later while the C repo appears BSD, creating an unresolved licensing ambiguity that independently blocks code-level borrowing

## Decision

Withdraw the stale visitor/allocation objection entirely; reaffirm nostrdb-rs rejection on corrected D8 grounds (NMP's composite reverse index naming interested views per insert is inexpressible in nostrdb's flat filter-poll subscription model) plus a new independent license blocker; explicitly supersede the preliminary 'Option A is probably right' lean in the lessons doc as historical context only

## Consequences

- The rejection stands but now rests on accurate technical reasoning (D4 ownership, ADR-0011 env-injection, D8 wake-model mismatch, licensing) rather than a stale visitor claim
- Code-level borrowing from nostrdb or nostrdb-rs is blocked until the GPL-vs-BSD license ambiguity is resolved with upstream
- Adopting nostrdb design concepts (visitor semantics, packed layout, provenance indexes) remains permitted and is tracked as concrete follow-up issues (#1516–#1521, #1524)
- The lessons doc §2.5 and §7 now carry explicit 'superseded' banners pointing to the evaluation doc as the resolved decision
- Follow-up issues are concretely cross-referenced in the evaluation doc §8 instead of left as prose

## Open Tail

- License clarification with upstream (issue #1524) must be resolved before any code-level borrowing can proceed
- Baseline capture (#1522) is a hard precondition before any performance-affecting PR (#1516 etc.)

## Evidence

- transcript lines 133-143
- transcript lines 145-155
- transcript lines 157-179
- transcript lines 183-193
- transcript lines 197-213
- transcript lines 250-273
- transcript lines 275-285

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-nostrdb-rs-rejection-reaffirmed-on-corrected.json`](transcripts/2026-06-18-1-nostrdb-rs-rejection-reaffirmed-on-corrected.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-nostrdb-rs-rejection-reaffirmed-on-corrected.json`](transcripts/raw/2026-06-18-1-nostrdb-rs-rejection-reaffirmed-on-corrected.json)
