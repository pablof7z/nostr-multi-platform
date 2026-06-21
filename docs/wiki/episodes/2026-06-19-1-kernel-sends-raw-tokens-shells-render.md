---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: active
subjects:
  - kernel-shell-boundary
  - presentation-formatting
  - signer-state-labels
supersedes:
  - 2026-06-19-5-signer-state-labels-moved-to-shells
related_claims: []
source_lines:
  - 25-26
  - 1697-1700
  - 2024-2025
  - 2086-2093
captured_at: 2026-06-19T11:51:39Z
---

# Episode: Kernel sends raw tokens; shells render all presentation

## Prior State

SF-Symbol names ("person.fill", "heart"), English status/stage labels, and signer-state prose were baked into platform-neutral nmp-core and .fbs schemas — violating ADR-0032's own mandate. Code comments cited "ADR-0032/#1099" as authority for the precompute, but #1099 was actually a regression of ADR-0032 (a miscitation).

## Trigger

#1493 audit P1 (SF-Symbols in kernel) + P9 labels finding; user directive to fix P1 and P9 labels. Live session decision: aim §2 overrides stale ADR-0032/#1099 citations — then p9 discovered ADR-0032 already agreed; the code comments were a miscitation.

## Decision

nmp-core sends raw tokens only (signer_kind, state, stage, is_* flags). All presentation (SF-Symbol names, English labels, status_tone) removed from .fbs schemas and Rust. Shells (iOS AccountsView/SignerStateTone, Android SignInScreen/deriveStatusLabel, desktop helpers) render via shared parity-consistent token→label mapping. status_tone dropped entirely (1:1 derivable from state enum). ADR-0032 amended with dated note recording the #1099 regression and #1580 removal.

## Consequences

- Shells must maintain parity-consistent token→label mapping (kept identical across iOS/Android/desktop = the no-divergence guarantee)
- Desktop/gallery shell consumers that the subagent's scoped compile missed were caught by CI's full cargo test — exposed apps/* CI blind spot (#1553)
- ADR-0032 miscitation corrected; format_sats_display exemption left alone
- 5 P1 PRs + P9 PR3 (#1580) + ADR amendment (#1584) merged

## Open Tail

- #1553 — CI gap where cargo test doesn't compile apps/* tests

## Evidence

- transcript lines 25-26
- transcript lines 1697-1700
- transcript lines 2024-2025
- transcript lines 2086-2093

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-1-kernel-sends-raw-tokens-shells-render.json`](transcripts/2026-06-19-1-kernel-sends-raw-tokens-shells-render.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-1-kernel-sends-raw-tokens-shells-render.json`](transcripts/raw/2026-06-19-1-kernel-sends-raw-tokens-shells-render.json)
