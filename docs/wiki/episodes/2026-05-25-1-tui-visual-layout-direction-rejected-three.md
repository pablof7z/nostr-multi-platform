---
type: episode-card
date: 2026-05-25
session: 93c599f0-3aea-440a-9c42-1de6cd8771fe
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/93c599f0-3aea-440a-9c42-1de6cd8771fe.jsonl
salience: reversal
status: active
subjects:
  - chirp-tui
  - tui-mockups
  - visual-layout
supersedes: []
related_claims: []
source_lines:
  - 1-58
  - 60-95
captured_at: 2026-06-18T05:17:46Z
---

# Episode: TUI visual layout direction rejected, three alternatives explored

## Prior State

Existing chirp TUI visual design was the active implementation

## Trigger

User explicitly rejected it: 'the chirp tui looks complete shit, this is not at all what I have in mind for a social TUI app'

## Decision

Abandoned the old TUI visual direction; created three distinct mockup approaches for evaluation — A: wide feed + persistent right panel, B: master-detail split (38/62), C: full-width feed + modal overlays

## Consequences

- Three standalone Ratatui mockup binaries built in tui-mockups/approach-[a/b/c]/
- PRs opened for each approach (e.g. #492 for approach B)
- Standalone crates require empty [workspace] table to opt out of parent workspace (parent Cargo.toml has no exclude field)

## Open Tail

- No approach selected yet for integration into main chirp-tui app
- Mockups use fake data; real data wiring still needed

## Evidence

- transcript lines 1-58
- transcript lines 60-95

