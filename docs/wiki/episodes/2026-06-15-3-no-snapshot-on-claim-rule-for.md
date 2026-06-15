---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-wasm-runtime
  - web-feed-snapshot-loop
supersedes:
  - 2026-06-15-2-wasm-no-snapshot-on-claim-rule
related_claims: []
source_lines:
  - 2459-2465
captured_at: 2026-06-15T04:48:56Z
---

# Episode: No-snapshot-on-claim rule for wasm/web SolidJS dispatch

## Prior State

The wasm runtime's handle_action for claim_profile took a snapshot of the projection, which triggered SolidJS <For> component remounting.

## Trigger

CI failure on the PR branch: the web feed entered an infinite loop — claim → snapshot → SolidJS <For> remount → re-claim → re-snapshot — because the snapshot update caused the reactive UI to re-render and re-issue the claim.

## Decision

Established the claim_no_snapshot rule: when handling a claim action in the wasm/web dispatch path, do NOT take a projection snapshot. Stated as a general rule for future dispatch arms in SolidJS contexts.

## Consequences

- Web feed infinite loop resolved
- General principle: never snapshot on claim in reactive/SolidJS contexts to avoid remount loops
- Extracted dispatch arm to runtime/dispatch.rs to satisfy file-size gate (582→518 LOC)

## Open Tail

*(none)*

## Evidence

- transcript lines 2459-2465
