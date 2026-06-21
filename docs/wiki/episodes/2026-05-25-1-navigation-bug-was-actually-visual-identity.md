---
type: episode-card
date: 2026-05-25
session: 1231660f-79c1-4b38-9651-9111cc20afb0
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1231660f-79c1-4b38-9651-9111cc20afb0.jsonl
salience: root-cause
status: active
subjects:
  - registry-routing
  - screenshot-display
supersedes: []
related_claims: []
source_lines:
  - 1-860
  - 967-970
captured_at: 2026-06-18T05:27:43Z
---

# Episode: Navigation bug was actually visual identity — not routing

## Prior State

User reported navigation links broken: URL changes but page content stays the same, implying a client-side routing failure in @solidjs/router

## Trigger

Browser agent testing confirmed routing works correctly; all three nav links produced distinct page content. Investigation revealed all component pages render identically because screenshots were placeholder-only (empty public/screenshots/, then stale dist build), making navigation appear non-functional visually

## Decision

No routing code changes needed; the real problem is that component pages lack visual distinctness because screenshots were missing/not-built-into-dist. Fix: rebuild dist to include screenshots, clear browser cache

## Consequences

- Extensive router source-code investigation was a red herring — routing was never broken
- Pages become visually distinct once real screenshots are served
- Shifted focus to screenshot generation and display quality

## Open Tail

*(none)*

## Evidence

- transcript lines 1-860
- transcript lines 967-970

