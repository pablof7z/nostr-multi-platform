---
type: episode-card
date: 2026-05-21
session: 4f37753c-0654-4478-9c19-e799f1b10d39
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/4f37753c-0654-4478-9c19-e799f1b10d39.jsonl
salience: architecture
status: active
subjects:
  - chirp-tui-image-pipeline
  - terminal-graphics-protocol
supersedes: []
related_claims: []
source_lines:
  - 238-316
  - 769-846
captured_at: 2026-06-18T05:00:59Z
---

# Episode: Image rendering adopts ratatui-image fallback ladder with VHS exclusion

## Prior State

No image rendering pipeline existed for chirp; user requirement specified iTerm2-capable avatar display and inlined images in the console

## Trigger

Media research and image protocol research agents converged on ratatui-image's canonical fallback chain: Kitty (unicode placeholders) → iTerm2 → Sixel → Unicode halfblocks. VHS was evaluated for end-to-end testing but found to use a headless ttyd that strips all image protocols.

## Decision

Adopt ratatui-image 11.0 with Picker::from_query_stdio() for runtime protocol detection. Image-heavy demos use real iTerm2 + QuickTime screencapture, not VHS GIFs. CI uses TestBackend + insta for layout, expectrl for PTY-driven keyboard flows.

## Consequences

- Avatar and inline image rendering degrades gracefully across terminal emulators — no single-protocol dependency
- VHS is excluded from image-protocol testing — it will only show halfblock fallback in recordings
- Picker::from_query_stdio() requires a real TTY; must guard with IsTerminal check to avoid CI deadlocks
- tmux users must set allow-passthrough for iTerm2/Kitty image sequences to work
- Animation (Kitty frame protocol) is iTerm2-incompatible — static fallback only on macOS primary target

## Open Tail

- Whether to ship image previews as opt-in per-message (tut model) or always-on by default
- Whether Sixel support warrants a Linux CI matrix with foot + Xvfb

## Evidence

- transcript lines 238-316
- transcript lines 769-846

