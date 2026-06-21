---
type: episode-card
date: 2026-05-26
session: f26050da-6d8a-4128-9179-4088a9df94b9
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/f26050da-6d8a-4128-9179-4088a9df94b9.jsonl
salience: architecture
status: superseded
subjects:
  - d6-doctrine
  - publish-outbox
  - display-separation
supersedes: []
related_claims: []
source_lines:
  - 4025-4063
captured_at: 2026-06-18T06:04:59Z
---

# Episode: D6 display-separation doctrine enforced — display helper removed from kernel projection

## Prior State

The `publish_outbox.rs` kernel projection imported and called `display::short_npub` to format relay-reason strings, violating D6 doctrine which bans display helpers from kernel/projection/FFI code — backend must emit raw data and let the shell render it.

## Trigger

Codex architectural assessment identified a P1 D6 violation: `use crate::display::short_npub` and its call site in `publish_outbox.rs` line 181 (`format!("Inbox relay for {}", short_npub(pubkey))`).

## Decision

Removed the `display::short_npub` import and replaced the formatted call with raw pubkey interpolation (`format!("Inbox relay for {pubkey}")`). Added D6-explanatory doc comments at the projection struct to clarify why display helpers are banned.

## Consequences

- Kernel projections now emit raw hex pubkeys; display formatting is exclusively a shell/UI concern
- D6 doctrine grep gate passes (zero production `use display::` imports in kernel; doc comments excluded by refined check pattern)
- Doc comments at types.rs and identity_state.rs explain the D6 boundary for future contributors
- PR #655 merged to master

## Open Tail

*(none)*

## Evidence

- transcript lines 4025-4063

