---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: reversal
status: superseded
subjects:
  - p5-polling
  - nmp-signer-broker
  - handshake-cancel
supersedes:
  - 2026-06-18-7-replace-signer-broker-handshake-polling-with
related_claims: []
source_lines:
  - 29-30
  - 911-924
  - 926-937
  - 1360-1382
captured_at: 2026-06-18T21:31:23Z
---

# Episode: Handshake polling eliminated: event-driven cancel via crossbeam

## Prior State

Handshake used a 200ms cancel-poll loop (checking AtomicBool in a tight loop) and mpsc channels for inbound events — classic D8 polling violation.

## Trigger

#1493 P5 finding: polling/sleep loops flagged as architectural violation.

## Decision

Replaced with crossbeam-channel select_biased! on (cancel_rx, after(deadline), inbound_rx). cancel() does try_send(()) on a one-shot cancel channel (non-blocking, off the actor path); sender drop also wakes the select. AtomicBool kept only for cheap pre-dial checkpoints. nmp-signer-broker adds crossbeam-channel as a direct dep (not in workspace.dependencies).

## Consequences

- Three signer-broker files pushed over the 500-line file-size hard cap by the crossbeam additions (handshake/tests.rs 724, broker.rs 710, handshake.rs 584) — #1547 failing file-size gate, needs split before merge.
- PR #1540 (nip60 complete_deposit NUT-04 doc) merged as independent P5 finding 2.
- Finding 1 (wallet-poc sleep loops) stale — deleted by #1509.

## Open Tail

- #1547 needs file-size split (3 oversized files) before it can merge; p5 agent resting, lead handling merge sweeps.

## Evidence

- transcript lines 29-30
- transcript lines 911-924
- transcript lines 926-937
- transcript lines 1360-1382

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-6-handshake-polling-eliminated-event-driven-cancel.json`](transcripts/2026-06-18-6-handshake-polling-eliminated-event-driven-cancel.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-6-handshake-polling-eliminated-event-driven-cancel.json`](transcripts/raw/2026-06-18-6-handshake-polling-eliminated-event-driven-cancel.json)
