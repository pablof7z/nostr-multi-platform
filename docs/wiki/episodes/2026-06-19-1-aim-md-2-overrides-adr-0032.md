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
  - signer-state-labels
  - sf-symbols-in-kernel
supersedes:
  - 2026-06-19-1-architecture-doctrine-aim-md-2-overrides
related_claims: []
source_lines:
  - 1593-1612
  - 1697-1700
  - 2083-2102
captured_at: 2026-06-19T00:46:24Z
---

# Episode: aim.md §2 overrides ADR-0032: presentation formatting belongs in shells, not core

## Prior State

ADR-0032/#1099 had Rust precomputing display labels (status_label, status_tone, stage_label) in FlatBuffers schemas, reasoning that native platforms shouldn't branch on discriminants. The code comments cited "ADR-0032/#1099" as authority. SF Symbol names and English prose were baked into platform-neutral nmp-core.

## Trigger

p9 agent hit a genuine doctrine conflict: ADR-0032/#1099 says precompute labels in Rust, but aim.md §2 says Rust emits raw data and shells format. Three sibling lanes (#1568/#1536/#1537) had already reversed this exact precompute pattern. Investigation revealed ADR-0032 actually already agreed with raw-data-out; #1099 was a regression of it, and the in-code citations were a miscitation.

## Decision

Direction A confirmed: aim.md §2 is the immutable north star and overrides ADRs. Rust emits raw semantic tokens (signer_kind, connection_state, stage); shells map token→localized label via shared parity-consistent helpers. Removed status_label/status_tone from signer_state.fbs, stage_label from bunker_handshake.fbs, deleted signer_state_label.rs entirely. ADR-0032 amended with a dated record of the #1099 regression and #1580 removal.

## Consequences

- All presentation formatting (SF Symbol names, English prose, bech32) removed from nmp-core; shells own formatting
- Signer state labels rendered by shared helpers in iOS (SignerStateTone), Android (deriveStatusLabel/deriveStatusTone), and desktop
- P4 F3 signerKind→rowLabel folded into the same shell-mapping pattern (deleted SignInScreen inline if)
- ADR-0032 amended (#1584) to prevent re-introduction of the precompute regression
- Subagents must run full cargo build --tests, not scoped -p (CI caught 3 default-workspace consumers the subagent missed)

## Open Tail

- format_sats_display exemption left intentionally in core (currency formatting is data, not presentation)

## Evidence

- transcript lines 1593-1612
- transcript lines 1697-1700
- transcript lines 2083-2102

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-1-aim-md-2-overrides-adr-0032.json`](transcripts/2026-06-19-1-aim-md-2-overrides-adr-0032.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-1-aim-md-2-overrides-adr-0032.json`](transcripts/raw/2026-06-19-1-aim-md-2-overrides-adr-0032.json)
