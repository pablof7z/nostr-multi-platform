---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - presentation-formatting
  - signer-state-projection
  - adr-0032
  - aim-section-2
supersedes:
  - 2026-06-18-1-adr-0032-superseded-aim-md-2
related_claims: []
source_lines:
  - 1587-1612
captured_at: 2026-06-18T23:38:04Z
---

# Episode: Presentation formatting doctrine: aim.md §2 overrides ADR-0032

## Prior State

ADR-0032/#1099 established that Rust should precompute display labels (status_label, status_tone, stage_label) so native shells render verbatim without branching on discriminants. In practice, English prose like SF Symbol names ("person.fill", "heart") and status strings were baked into the platform-neutral nmp-core kernel.

## Trigger

p9 discovered a direct conflict between ADR-0032 and aim.md §2 when implementing signer_state PR3: ADR-0032 says precompute in Rust, but aim.md §2 says Rust emits raw data and NEVER presentation in projections. Every sibling lane (#1568 publish_outbox, #1536 marmot, #1537 nip29, #1577 relay_diagnostics) had already reversed this exact precompute pattern with the team-lead's approval.

## Decision

Direction A confirmed: aim.md §2 is the immutable north star and overrides ADRs. Rust emits raw semantic TOKENS (signer_kind, connection_state, stage — enumerable, non-prose); shells map token→localized label via a SHARED, parity-consistent helper. ADR-0032 is superseded for display labels. P4 F3's signerKind→rowLabel switch collapses into the same direction (shell-side mapping, not Rust precompute).

## Consequences

- status_label/status_tone/stage_label removed from signer_state.fbs and bunker_handshake.fbs; signer_state_label.rs + stage_label_for() deleted
- All projection formatting (SF Symbols, status strings, tone labels, relay diagnostics formatting) moved out of nmp-core into shell render code
- ADR-0032 must be formally superseded/noted so it doesn't become a doc-lie
- Future projections must not bake English prose or platform-specific presentation into Rust; emit raw tokens only

## Open Tail

- ADR-0032 formal supersession still needed in docs

## Evidence

- transcript lines 1587-1612

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-presentation-formatting-doctrine-aim-md-2.json`](transcripts/2026-06-18-1-presentation-formatting-doctrine-aim-md-2.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-presentation-formatting-doctrine-aim-md-2.json`](transcripts/raw/2026-06-18-1-presentation-formatting-doctrine-aim-md-2.json)
