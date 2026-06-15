---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: architecture
status: superseded
subjects:
  - wasm-runtime-snapshot
  - web-feed-rendering
  - solidjs-for-remount
supersedes:
  - 2026-06-15-2-wasm-dispatch-must-not-push-snapshot
related_claims: []
source_lines:
  - 2416-2465
  - 2781-2782
captured_at: 2026-06-15T04:42:02Z
---

# Episode: WASM no-snapshot-on-claim rule: SolidJS remount loop fix

## Prior State

The WASM runtime took a state snapshot on every claim_profile call. In the web feed (SolidJS), each snapshot triggered a reactive update that caused <For> components to remount, which triggered new claims, producing an infinite render loop that broke the web feed.

## Trigger

Web feed CI failure and infinite loop discovered during profile-claim testing on the fix branch.

## Decision

Claims must never trigger a snapshot in the WASM runtime. The no-snapshot-on-claim rule is established as a general invariant for all future dispatch arms in the WASM runtime (documented in docs/wiki/profile-resolution.md).

## Consequences

- Web feed renders without infinite loop after profile claims
- Any future dispatch arm added to WasmRuntime::handle must follow the no-snapshot-on-claim rule
- claim_no_snapshot path added alongside the existing snapshot path, with dedicated tests

## Open Tail

*(none)*

## Evidence

- transcript lines 2416-2465
- transcript lines 2781-2782
