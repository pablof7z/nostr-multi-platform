---
type: episode-card
date: 2026-05-21
session: 4f37753c-0654-4478-9c19-e799f1b10d39
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/4f37753c-0654-4478-9c19-e799f1b10d39.jsonl
salience: architecture
status: active
subjects:
  - chirp-tui-dependencies
  - compose-input-widget
supersedes: []
related_claims: []
source_lines:
  - 901-962
  - 1021-1032
captured_at: 2026-06-18T05:00:59Z
---

# Episode: Adopt ratatui 0.30 with tui-input, rejecting tui-textarea

## Prior State

tui-textarea is the maturity leader for Rust multiline TUI input (489k downloads, undo/redo, search, selection) and would be the default choice for a compose editor

## Trigger

Crate sweep agent found tui-textarea is pinned to ratatui 0.29 + crossterm 0.28 (last release Oct 2024). The rest of the ecosystem has rotated to ratatui 0.30. Staying on 0.29 blocks the entire dependency tree from upgrading.

## Decision

User explicitly chose ratatui 0.30 + tui-input 0.15 (single-line) with a custom multiline compose textarea — no undo/redo, no search, no selection in compose. The whole tree stays on 0.30.

## Consequences

- Compose widget must be built from scratch: custom multiline buffer, cursor movement, backspace across lines
- No undo/redo in compose — acceptable for short-form Nostr notes but limiting for long-form NIP-23 articles
- @-mention autocomplete must be wired manually (tui-textarea's cursor API is gone; use tui-input cursor position + tui-popup + tui-widget-list)
- Future tui-textarea 0.30 release would allow upgrading to get undo/redo; until then, custom textarea is tech debt

## Open Tail

- Whether to vendor a tui-textarea 0.29 → 0.30 patch (one-line crossterm bump) as an interim solution
- Whether custom compose textarea should support NIP-23 long-form (kind:30023) or only short notes (kind:1)

## Evidence

- transcript lines 901-962
- transcript lines 1021-1032

