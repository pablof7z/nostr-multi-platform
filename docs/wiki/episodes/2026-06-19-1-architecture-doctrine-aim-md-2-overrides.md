---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - presentation-formatting
  - adr-0032
  - signer-state
  - flatbuffers-schema
supersedes:
  - 2026-06-18-1-presentation-formatting-doctrine-aim-md-2
related_claims: []
source_lines:
  - 1587-1612
captured_at: 2026-06-19T00:18:35Z
---

# Episode: Architecture doctrine: aim.md §2 overrides ADR-0032 — raw tokens out, shells format

## Prior State

ADR-0032/#1099 directed that labels be precomputed in Rust so native shells don't branch on discriminants. English labels (status_label, stage_label, status_tone) and SF Symbol names were baked into platform-neutral nmp-core projections (signer_state.fbs, bunker_handshake.fbs, publish_outbox).

## Trigger

P9 PR3 brief said 'signer labels belong in shells' but P4 F3 said 'precompute signer label in Rust' — contradictory directives for the same signer_state projection. Three sibling lanes (#1568, #1536, #1537) had already reversed this exact precompute pattern.

## Decision

aim.md §2 is the immutable north star and overrides ADRs. Rust emits raw semantic tokens (signer_kind, connection_state, stage — enumerable, non-prose); shells map tokens to localized labels via shared parity-consistent helpers. ADR-0032 must be formally superseded so it doesn't become a doc-lie.

## Consequences

- P1 removed SF Symbol names and English prose from nmp-core across publish_outbox, marmot, nip29, relay_diagnostics projections
- P9 PR3 removed status_label/status_tone from signer_state.fbs and stage_label from bunker_handshake.fbs, deleting signer_state_label.rs
- P4 F3 (signerKind→rowLabel) stays shell-side — not a Rust precompute
- Desktop/gallery consumers that still referenced removed fields were caught by CI and fixed with desktop_signer_label/desktop_stage_label helpers
- All future projections must follow the raw-tokens-out pattern; ADR-0032 superseded

## Open Tail

- ADR-0032/#1099 formal supersession not yet filed

## Evidence

- transcript lines 1587-1612

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-1-architecture-doctrine-aim-md-2-overrides.json`](transcripts/2026-06-19-1-architecture-doctrine-aim-md-2-overrides.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-1-architecture-doctrine-aim-md-2-overrides.json`](transcripts/raw/2026-06-19-1-architecture-doctrine-aim-md-2-overrides.json)
