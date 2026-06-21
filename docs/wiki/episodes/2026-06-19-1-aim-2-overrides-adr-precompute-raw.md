---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - signer-labels-to-shells
  - aim-section-2-doctrine
  - adr-0032
  - presentation-formatting
supersedes:
  - 2026-06-19-1-aim-md-2-overrides-adr-0032
related_claims: []
source_lines:
  - 1605-1612
  - 2024-2034
  - 2083-2093
captured_at: 2026-06-19T06:25:53Z
---

# Episode: aim§2 overrides ADR precompute: raw semantic tokens out, shells format

## Prior State

Signer state labels (status_label, status_tone, stage_label) were precomputed English prose in Rust .fbs schemas and nmp-core, citing 'ADR-0032/#1099' as mandate. SF Symbol names and bech32 formatting also baked into platform-neutral nmp-core. ADR-0032 was believed to mandate precompute in Rust to prevent iOS/Android divergence.

## Trigger

P1 audit found SF Symbol names and English prose in nmp-core (D1 breach); P9 PR3 design hit a conflict where ADR-0032/#1099 precompute mandate contradicted aim.md §2 (raw data out, shells format). Three sibling lanes (#1568/#1536/#1537) had already reversed this exact precompute pattern.

## Decision

aim.md §2 is the immutable north star and OVERRIDES ADRs. Rust emits raw semantic TOKENS (signer_kind, connection_state, stage — enumerable, non-prose); shells map token→localized label via a SHARED parity-consistent helper. status_tone was dropped entirely (1:1 derivable from state enum). On inspection, ADR-0032 already agreed with raw-data-out — #1099 was a REGRESSION of it, not sanctioned by it; the code comments citing 'ADR-0032/#1099' were a miscitation. ADR-0032 amended (#1584) to record the regression→removal.

## Consequences

- status_label/status_tone/stage_label removed from signer_state.fbs, bunker_handshake.fbs, and signer_state_label.rs deleted
- iOS AccountsView/SignerStateTone and Android SignInScreen derive labels via shared token→label mapping (no Rust precompute)
- P4 F3 signerKind→rowLabel inline switch folded into the same shell mapping
- ADR-0032 amended with dated note recording #1099 regression and #1580 removal, preventing re-introduction
- Sets precedent: aim.md overrides ADRs when they conflict; ADRs are subordinate and amendable

## Open Tail

- format_sats_display was identified as an existing ADR-0032 exemption and left alone — may need future evaluation

## Evidence

- transcript lines 1605-1612
- transcript lines 2024-2034
- transcript lines 2083-2093

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-1-aim-2-overrides-adr-precompute-raw.json`](transcripts/2026-06-19-1-aim-2-overrides-adr-precompute-raw.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-1-aim-2-overrides-adr-precompute-raw.json`](transcripts/raw/2026-06-19-1-aim-2-overrides-adr-precompute-raw.json)
