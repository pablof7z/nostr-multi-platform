---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: reversal
status: superseded
subjects:
  - adr-0032-supersession
  - signer-state-labels
  - presentation-formatting-doctrine
supersedes:
  - 2026-06-18-1-display-label-doctrine-aim-md-2
related_claims: []
source_lines:
  - 1587-1612
  - 1697-1700
  - 1761-1763
captured_at: 2026-06-18T23:24:57Z
---

# Episode: ADR-0032 superseded: aim.md §2 overrides precomputed labels in Rust projections

## Prior State

ADR-0032/#1099 mandated precomputed display labels in Rust so native shells wouldn't branch on discriminants. signer_state.fbs included status_label and status_tone (English prose like "Connecting to bunker relays…"), and bunker_handshake included stage_label. P4 F3 also directed "ship a precomputed signer label" in Rust.

## Trigger

p9 discovered that PR3's brief (labels-to-shells, matching aim.md §2) and P4 F3 (precompute in Rust, render verbatim) pointed in OPPOSITE directions for the same signer_state projection, creating a genuine ADR-vs-aim conflict that required resolution before schema changes.

## Decision

Direction A confirmed: aim.md §2 ("Rust emits raw data, NEVER presentation in projections") is the immutable north star and OVERRIDES ADR-0032. Remove status_label/status_tone from signer_state.fbs and stage_label from bunker_handshake.fbs; delete signer_state_label.rs and stage_label_for(). Rust emits raw semantic tokens (signer_kind, connection_state, stage); shells map token→localized label via shared, parity-consistent helpers. ADR-0032 must be formally superseded/annotated. P4 F3 folds into the same change (shell helper for signerKind→label, not Rust precompute).

## Consequences

- signer_state.fbs, bunker_handshake.fbs lose presentation fields; FlatBuffers bindings must be regenerated
- iOS AccountsView + Android SignInScreen own label rendering via shared token→label mapping
- status_tone kept only if it's a semantic token not 1:1 derivable from the state enum, otherwise derived shell-side
- ADR-0032 becomes a doc-lie unless superseded — p9 must annotate/retire the relevant clause
- Establishes precedent that aim.md §2 overrides ADRs for any future projection design conflict
- Three sibling lanes (#1568 publish_outbox, #1536 marmot, #1537 nip29) already reversed this same pattern with approval

## Open Tail

- PR3 (labels-to-shells) in progress on p9's lane; not yet merged
- ADR-0032 formal supersession annotation still needed

## Evidence

- transcript lines 1587-1612
- transcript lines 1697-1700
- transcript lines 1761-1763

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-adr-0032-superseded-aim-md-2.json`](transcripts/2026-06-18-1-adr-0032-superseded-aim-md-2.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-adr-0032-superseded-aim-md-2.json`](transcripts/raw/2026-06-18-1-adr-0032-superseded-aim-md-2.json)
