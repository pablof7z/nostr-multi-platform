---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-signer-broker
  - handshake
supersedes: []
related_claims: []
source_lines:
  - 586-601
  - 634-635
captured_at: 2026-06-18T20:12:30Z
---

# Episode: Signer handshake: 200ms polling loop → event-driven crossbeam

## Prior State

signer-broker handshake.rs:258 used recv_timeout(200ms) in a loop to re-poll the cancel AtomicBool. The handshake thread held its own live inbound_tx, so a bare recv() would never wake on cancel, requiring the 200ms poll.

## Trigger

Issue #1493 audit (P5 F3) identified this as a D8 polling violation (though the D8 lint catches sleep/tokio::sleep, not recv_timeout). Codex design confirmed the fix.

## Decision

Switch to crossbeam-channel + one-shot cancel channel + select_biased!(cancel_rx, after(deadline), inbound_rx). Timer poll disappears; cancel and deadline become events; the deadline fires once (D8-clean). Scope expanded to broker.rs, broker/nostrconnect.rs, and Cargo.toml (crossbeam-channel dep) since handshake.rs alone won't compile.

## Consequences

- Cancel signaling is immediate rather than 200ms-delayed.
- The D8 no-polling doctrine is enforced in the handshake path.
- p5's handshake PR (#1547) must merge before p9's PR1 (which touches nostrconnect.rs with a different change); p9 rebases on top.

## Open Tail

*(none)*

## Evidence

- transcript lines 586-601
- transcript lines 634-635

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-9-signer-handshake-200ms-polling-loop-event.json`](transcripts/2026-06-18-9-signer-handshake-200ms-polling-loop-event.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-9-signer-handshake-200ms-polling-loop-event.json`](transcripts/raw/2026-06-18-9-signer-handshake-200ms-polling-loop-event.json)
