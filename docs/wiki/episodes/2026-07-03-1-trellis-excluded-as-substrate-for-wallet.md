---
type: episode-card
date: 2026-07-03
session: 91a86fdf-624c-446e-9b38-0fb02085121f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/91a86fdf-624c-446e-9b38-0fb02085121f.jsonl
salience: architecture
status: active
subjects:
  - trellis-scope
  - wallet-operation-journal
  - adr-0075-boundary
supersedes: []
related_claims: []
source_lines:
  - 328-329
  - 376-403
  - 407-409
  - 411-423
  - 425-446
captured_at: 2026-07-03T07:37:30Z
---

# Episode: Trellis excluded as substrate for wallet money-safety journal

## Prior State

Open question whether Trellis (ADR-0075 private reconciliation substrate) could power the wallet's durable operation journal (Draft→MintPending→MintSettled→PublishPending→Settled). Trellis already backs the feed-session read path, so extending it to the wallet write side seemed plausible.

## Trigger

User asked whether Trellis should be used for the internal mechanics of the wallet state machine, prompting examination of ADR-0075, the trellis_adapter implementation, and trellis-core source.

## Decision

No — Trellis is a category mismatch for the wallet journal. Trellis graphs are in-memory, per-session, and die with the process by design (built fresh in FeedSessionTrellisAdapter::new, torn down on close_scope, zero persistence in trellis-core). The wallet journal's defining requirement is surviving process death after an irreversible external effect (a mint spend). ADR-0075's charter grants Trellis only as machinery 'below typed read sessions'; a durable write saga is not below a read session, so extending it would require a new ADR — which is exactly the risk the ADR's Context section names. Use the standard NMP actor pattern: an actor-owned durable journal persisted through NMP storage, with command-shaped reducers that never await mint HTTP, capability-lane mint workers returning raw results, and the existing publish engine handling PublishPending.

## Consequences

- No new generic durable-saga crate should be built now — it would be a one-consumer abstraction
- If a second saga ever appears (nmp-marmot pending-MLS-autopublish is the likely candidate), extract a generic saga substrate post-hoc at that point
- The wallet read projection (balances, pending-op summaries keyed by correlation id) can still ride Trellis-backed reconciliation invisibly — Trellis stays on the read side only
- The ADR-0075 'prove equivalence against the existing path before deleting bespoke machinery' gate is incoherent for this use case since there is no existing path

## Open Tail

- If a future need arises to extend Trellis into write lifecycles, a new ADR is required — this session explicitly identified that gap

## Evidence

- transcript lines 328-329
- transcript lines 376-403
- transcript lines 407-409
- transcript lines 411-423
- transcript lines 425-446

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-03-1-trellis-excluded-as-substrate-for-wallet.json`](transcripts/2026-07-03-1-trellis-excluded-as-substrate-for-wallet.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-03-1-trellis-excluded-as-substrate-for-wallet.json`](transcripts/raw/2026-07-03-1-trellis-excluded-as-substrate-for-wallet.json)
