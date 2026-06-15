---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: architecture
status: superseded
subjects:
  - wasm-dispatch
  - solidjs
  - snapshot-rule
  - web-feed
supersedes:
  - 2026-06-15-3-no-snapshot-on-claim-rule-for
related_claims: []
source_lines:
  - 2566-2568
  - 2777-2782
captured_at: 2026-06-15T09:21:42Z
---

# Episode: Wasm dispatch no-snapshot-on-claim rule

## Prior State

The wasm dispatch arm sent a snapshot on every claim_profile call, which caused SolidJS <For> components to remount in an infinite loop when the web feed loaded.

## Trigger

During the kernel migration, CI caught a web-feed regression: the wasm feed went into an infinite snapshot/remount loop because claims triggered snapshots that caused SolidJS re-renders that triggered more claims.

## Decision

The wasm dispatch arm must NOT send snapshots on claim — only on actual state changes. Stated as a general architectural rule for all future dispatch arms.

## Consequences

- Web feed infinite loop fixed
- General no-snapshot-on-claim rule established for all dispatch arms
- The force flag in claim_profile (from F-CR-00) enables OneShot re-verify without snapshot

## Open Tail

*(none)*

## Evidence

- transcript lines 2566-2568
- transcript lines 2777-2782
