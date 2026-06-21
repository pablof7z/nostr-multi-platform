---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: reversal
status: superseded
subjects:
  - signer-state-labels
  - status-label
  - status-tone
  - stage-label
  - adr-0032
  - signer-state-fbs
supersedes: []
related_claims: []
source_lines:
  - 1697-1700
  - 2083-2093
  - 2110-2114
captured_at: 2026-06-19T11:35:40Z
---

# Episode: Signer state labels moved to shells; #1099 identified as ADR-0032 regression

## Prior State

Precomputed status_label, status_tone, and stage_label were embedded in signer_state.fbs and bunker_handshake.fbs FlatBuffers schemas and produced by Rust-side signer_state_label.rs, supposedly sanctioned by ADR-0032/#1099. iOS/Android shells consumed these pre-rendered strings.

## Trigger

#1493 P9 and P4 F3 audit; user specified Direction A: remove labels from Rust, shells render from raw tokens. During implementation, p9 discovered that ADR-0032 already mandated raw-data-out — #1099's precompute was a regression of ADR-0032, not sanctioned by it. The code comments citing "ADR-0032/#1099" were a miscitation.

## Decision

Removed status_label, status_tone, and stage_label from signer_state.fbs and bunker_handshake.fbs; deleted signer_state_label.rs and stage_label_for(). status_tone was dropped entirely (1:1 derivable from the state enum). Shells (iOS AccountsView/SignerStateTone, Android SignInScreen/deriveStatusLabel/deriveStatusTone, desktop helpers) now derive labels from raw tokens (signer_kind, state, stage, is_* flags) via shared parity-consistent mappings. P4 F3 (SignInScreen inline signerKind→rowLabel switch) folded into the same shell mapping. ADR-0032 amended with a dated note recording the #1099 regression, its #1580 removal, and the miscitation correction.

## Consequences

- FlatBuffers schemas are now presentation-neutral (raw tokens only)
- Shell parity is enforced by shared mapping functions, not by Rust precomputation
- No one can re-add precomputed labels without contradicting an explicit ADR amendment (#1584)
- Desktop/gallery consumers that still referenced the removed fields were caught by CI and fixed with desktop_stage_label/desktop_signer_label_and_tone helpers
- Subagents must run full cargo build --tests, not scoped -p (lesson from PR3 CI catching misses the subagent missed)

## Open Tail

*(none)*

## Evidence

- transcript lines 1697-1700
- transcript lines 2083-2093
- transcript lines 2110-2114

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-5-signer-state-labels-moved-to-shells.json`](transcripts/2026-06-19-5-signer-state-labels-moved-to-shells.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-5-signer-state-labels-moved-to-shells.json`](transcripts/raw/2026-06-19-5-signer-state-labels-moved-to-shells.json)
