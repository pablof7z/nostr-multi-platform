---
type: episode-card
date: 2026-05-26
session: 1ca92577-a656-4fd9-879e-0f2fd87f0ee7
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1ca92577-a656-4fd9-879e-0f2fd87f0ee7.jsonl
salience: product
status: active
subjects:
  - chirp-tui-compose-ux
  - keybinding-semantics
supersedes: []
related_claims: []
source_lines:
  - 1-1
  - 270-324
captured_at: 2026-06-18T05:59:28Z
---

# Episode: Compose modal with Enter-to-send semantics

## Prior State

Compose mode used an inline bottom-bar with Ctrl+Enter to publish and Enter for newline insertion

## Trigger

User directive requesting a proper ratatui modal overlay for compose, with Enter to send and Shift+Enter for newline

## Decision

Replaced inline compose bar with a centered modal overlay (Clear-backed, ACCENT_CYAN bordered, cursor + char count); swapped keybinding semantics so Enter publishes and Shift+Enter inserts a newline; dropped Ctrl+Enter binding entirely

## Consequences

- Help overlay keybinding labels updated: 'Ctrl+Enter' → 'Enter' for publish, 'Enter' → 'Shift+Enter' for newline
- Status hints in start_compose and start_reply updated to new keymap format
- layout_tests.rs assertion updated from 'Ctrl+Enter' to 'Shift+Enter'
- Compose-bar rendering falls through to idle hint when modal is active (Compose branch removed from render_compose)
- Modal renders after help in layout flow, so modal covers help overlay if both are visible

## Open Tail

- Whether start_compose should reset show_help so the help overlay doesn't sit underneath the modal — flagged but not implemented

## Evidence

- transcript lines 1-1
- transcript lines 270-324

