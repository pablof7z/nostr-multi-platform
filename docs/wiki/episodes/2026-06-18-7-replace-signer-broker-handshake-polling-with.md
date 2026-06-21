---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: reversal
status: superseded
subjects:
  - p5-polling
  - signer-broker
  - handshake
  - d8-no-poll
supersedes:
  - 2026-06-18-9-signer-handshake-200ms-polling-loop-event
related_claims: []
source_lines:
  - 19-48
  - 585-601
  - 634-635
  - 911-924
captured_at: 2026-06-18T20:25:04Z
---

# Episode: Replace signer-broker handshake polling with event-driven crossbeam

## Prior State

signer-broker handshake.rs used recv_timeout(200ms) in a loop purely to re-poll an AtomicBool cancel flag — a D8 doctrine violation (thread::sleep/tokio::sleep polling pattern).

## Trigger

Issue #1493 P5 identified the 200ms poll loop as a real D8 violation (not just a lint finding — the D8 lint flags thread::sleep/tokio::sleep only, not recv_timeout, but the pattern is still polling).

## Decision

Switched inbound channel from mpsc to crossbeam-channel + added one-shot cancel channel + select_biased!(cancel_rx, after(deadline), inbound_rx). Timer poll eliminated; cancel and deadline become events. AtomicBool kept only for cheap pre-dial checkpoints.

## Consequences

- D8-clean: --workspace-d8 no-polling sweep returns 0 findings
- Cargo.toml gains crossbeam-channel as a direct dep on nmp-signer-broker only (NOT added to root workspace.dependencies to avoid shared-file collision)
- Wallet-poc sleep loops stale (crate deleted by #1509); nip60 complete_deposit caller-poll judged non-actionable (parked crate, zero callers, Cashu NUT-04/23 has no push primitive) — doc-comment only

## Open Tail

*(none)*

## Evidence

- transcript lines 19-48
- transcript lines 585-601
- transcript lines 634-635
- transcript lines 911-924

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-7-replace-signer-broker-handshake-polling-with.json`](transcripts/2026-06-18-7-replace-signer-broker-handshake-polling-with.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-7-replace-signer-broker-handshake-polling-with.json`](transcripts/raw/2026-06-18-7-replace-signer-broker-handshake-polling-with.json)
