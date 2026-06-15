---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - private-fail-closed
  - publish-policy-onedoor
  - d10-privacy-invariant
supersedes:
  - 2026-06-15-2-publish-policy-one-door-classification-table
related_claims: []
source_lines:
  - 2978-2985
  - 3098-3116
  - 3130-3157
captured_at: 2026-06-15T14:44:31Z
---

# Episode: PrivateFailClosed privacy gate enforced at universal dispatch-emit site

## Prior State

Privacy gate for private event kinds (gift-wrap kind:1059, sealed kind:14) was at individual entry points: scattered literal guard `raw.kind == KIND_GIFT_WRAP && Auto` at `actor/commands/publish.rs:400` (missed kind:14 entirely). `policy.rs` declared `PrivateFailClosed` but did not enforce it — `PublishRaw { kind:1059, target: Auto }` could pass. Resume-from-store and manual-retry paths bypassed `run_publish_engine_at` entirely, building `['EVENT',…]` frames directly from persisted rows.

## Trigger

Codex review caught three blockers: (1) PrivateFailClosed declared but not enforced — gift-wrap with Auto routing could leak to public relays (real D10 violation); (2) old literal guard still lived outside policy.rs, not the single door; (3) reintroduction gate was near-vacuous (scanned only action.rs for exact `kind==N` substrings). Second codex pass found `run_publish_engine_at` was not universal — `resume_from_store` and manual retry bypassed the gate, meaning a persisted private row could emit on restart/retry.

## Decision

The privacy gate moves to the universal emit site `dispatch_due` (in `publish/engine/helpers.rs`) where ALL paths converge: initial publish, resume-from-store, manual retry, and availability-redispatch all route through it. `policy::relay_emit_is_sanctioned(kind, relay_reasons)` checks that private kinds may emit ONLY to relays whose persisted `relay_reasons` includes `RelaySelectionReason::Explicit` (caller-pinned DM-inbox). Any other relay is refused (frame dropped, settled FailedAfterRetries, tracing::warn). Entry-point guards at `run_publish_engine_at` and action boundary remain as fail-fast defense-in-depth. Reintroduction gate broadened to catch `match`/`matches!`/`.contains` evasion shapes.

## Consequences

- No path (initial, resume, retry, redispatch) can auto-route a private envelope to public relays
- Persisted rows with pre-fix or Auto-resolved targets are dropped on replay/retry exactly as fresh ones would be
- The gate is non-vacuous: disabling the dispatch_due guard makes the resume-from-store test FAIL (persisted kind:1059 emits to public relay)
- Reintroduction gate now scans full routing surface (action.rs, commands/publish.rs, publish_cmd.rs, publish_engine.rs, engine/helpers.rs, engine/dispatch.rs) and catches ==-evasion shapes
- Entry-point guards serve defense-in-depth: clean dispatch-time errors for callers, but the dispatch-site gate is load-bearing

## Open Tail

- Broadened gate detector is still a line-scanner — crafty AST-level evasion (e.g. indirect kind comparison via variable) is not caught
- Integration with the stress harness (Area 11.2) to verify the dispatch-site gate end-to-end with a fixture relay

## Evidence

- transcript lines 2978-2985
- transcript lines 3098-3116
- transcript lines 3130-3157
