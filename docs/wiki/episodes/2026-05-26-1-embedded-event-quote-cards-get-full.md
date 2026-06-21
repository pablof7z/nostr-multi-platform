---
type: episode-card
date: 2026-05-26
session: fa300009-e498-4c80-a2d3-64d1531a09d4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/fa300009-e498-4c80-a2d3-64d1531a09d4.jsonl
salience: product
status: active
subjects:
  - nostr-quote-card
  - embedded-event-rendering
supersedes: []
related_claims: []
source_lines:
  - 1-360
  - 454-498
captured_at: 2026-06-18T05:46:49Z
---

# Episode: Embedded event quote cards get full visible box border across all platforms

## Prior State

TUI quote card used Borders::LEFT only (looked like a blockquote, not a distinct embedded event). iOS and Android quote cards used a 0.5pt/0.5dp hairline border that was nearly invisible, making it unclear that an embedded event was a self-contained object inside a parent event.

## Trigger

User provided a screenshot and explicit directive: the embedded event widget should render with clear boxing so it's obvious this is an event inside some other event.

## Decision

TUI: changed Borders::LEFT to Borders::ALL and recalculated preferred_height to account for top/bottom border rows and correct inner width. iOS: lineWidth increased from 0.5 to 1.5. Android: border width increased from 0.5.dp to 1.5.dp across all three card variants (collapsed, compact/frame, missing).

## Consequences

- Embedded events now have an unambiguous visual boundary distinguishing them from parent content on all three platforms
- TUI height calculation now correctly subtracts 2 for top/bottom borders and 2 for body indentation, preventing overflow
- Cross-platform registry files (crates/nmp-cli/registry/{tui,swiftui,compose}) and their app copies (Chirp, gallery) were all updated in lockstep

## Open Tail

*(none)*

## Evidence

- transcript lines 1-360
- transcript lines 454-498

