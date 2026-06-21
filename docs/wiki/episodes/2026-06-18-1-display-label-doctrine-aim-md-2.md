---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - adr-0032-override
  - presentation-formatting-in-kernel
  - signer-state-labels
supersedes:
  - 2026-06-18-3-presentation-formatting-doctrine-aim-md-2
related_claims: []
source_lines:
  - 1587-1612
  - 1604-1612
  - 1697-1700
captured_at: 2026-06-18T23:05:39Z
---

# Episode: Display-label doctrine: aim.md §2 supersedes ADR-0032 for presentation formatting

## Prior State

ADR-0032/#1099 mandated precomputing display labels in Rust projections so native shells render verbatim without branching on discriminants. signer_state.fbs already contained status_label, status_tone, and stage_label produced in Rust (signer_state_label.rs). This pattern contradicted aim.md §2 ("Rust emits raw data, NEVER presentation in projections; shells format") but the conflict was latent because no prior lane had touched signer_state specifically.

## Trigger

p9 agent discovered a direct conflict while starting PR3: the lane brief said "labels belong in shells" (remove from Rust), but P4 F3 said "ship a PRECOMPUTED signer label" (add to Rust) — opposite directions for the same projection. Three sibling lanes (#1568 publish_outbox, #1536 marmot, #1537 nip29) had already reversed this exact precompute pattern with the team lead's approval.

## Decision

Direction A confirmed: aim.md §2 is the immutable north star and overrides ADRs for display labels. Rust emits raw semantic tokens (signer_kind, connection_state, stage enums); shells map token → localized label via shared parity-consistent helpers. status_label/status_tone/stage_label are removed from Rust/FlatBuffers; signer_state_label.rs and stage_label_for() are deleted. P4 F3 folds in: the SignInScreen `if signerKind=="nip55"` switch becomes shared shell render code off the raw signer_kind token, not a Rust precompute. ADR-0032/#1099 must be formally superseded for display-label clauses.

## Consequences

- signer_state.fbs loses status_label/status_tone; bunker_handshake.fbs loses stage_label; FlatBuffers bindings must be regenerated
- iOS AccountsView + Android SignInScreen each gain a shared token→label mapping helper (parity-consistent, no per-screen ad-hoc switches)
- status_tone kept only if it is a semantic token not 1:1 derivable from the state enum; otherwise derived shell-side
- Every sibling lane (publish_outbox #1568, marmot #1536, nip29 #1537) already reversed this same pattern — signer_state is the last instance
- ADR-0032/#1099 becomes a doc-lie for display-label clauses unless formally superseded

## Open Tail

- ADR-0032 formal supersession write-up still needed
- Whether status_tone is kept or derived shell-side was left to p9's judgment (direction A allows keeping semantic tokens)

## Evidence

- transcript lines 1587-1612
- transcript lines 1604-1612
- transcript lines 1697-1700

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-display-label-doctrine-aim-md-2.json`](transcripts/2026-06-18-1-display-label-doctrine-aim-md-2.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-display-label-doctrine-aim-md-2.json`](transcripts/raw/2026-06-18-1-display-label-doctrine-aim-md-2.json)
