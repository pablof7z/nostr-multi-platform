---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - d10-privacy-leak
  - publish-one-door
  - gift-wrap-sealed-routing
supersedes:
  - 2026-06-15-1-d10-private-event-leak-closed-at
related_claims: []
source_lines:
  - 3056-3075
  - 3103-3115
  - 3130-3157
  - 3250-3275
  - 3378-3405
captured_at: 2026-06-15T15:36:07Z
---

# Episode: Private-fail-closed publish enforcement moved to universal dispatch-emit site

## Prior State

PrivateFailClosed policy (kind:1059 gift-wrap, kind:14 sealed) was declared but unenforced — scattered literal guards existed at individual publish entry points, and resume-from-store / manual-retry paths bypassed them entirely, allowing private envelopes to emit to public relays on restart or retry.

## Trigger

Codex review (multiple rounds) found: (1) initial entry-point guard at run_publish_engine_at was NOT universal — resume_from_store and retry_now build and emit EVENT frames directly, bypassing the gate; (2) refused private rows lingered Pending in the durable store, re-refusing on every restart.

## Decision

Enforce at the single universal convergence point — dispatch_due in publish/engine/helpers.rs — where initial, resume, retry, and availability-redispatch ALL route through. The gate checks persisted per-relay RelaySelectionReason::Explicit before emitting; non-Explicit targets are refused, settled FailedAfterRetries, and terminally finalized via extracted finalize_completed_rows (reuses the same tick/on_ack finalization path). Entry-point guards retained as defense-in-depth fail-fast only.

## Consequences

- No publish path (initial, resume, retry, availability-redispatch) can emit a private envelope to a non-Explicit relay — verified by resume/retry leak-drop tests + non-vacuity proof
- Refused private rows are terminally deleted from the durable store exactly once — never re-refused on restart
- validate_publish_routing at action boundary + run_publish_engine_at guard serve only as fail-fast defense-in-depth; the dispatch_due gate is the load-bearing universal one
- Broadened reintroduction gate detector catches match/matches!/.contains evasion shapes, not just == literals

## Open Tail

*(none)*

## Evidence

- transcript lines 3056-3075
- transcript lines 3103-3115
- transcript lines 3130-3157
- transcript lines 3250-3275
- transcript lines 3378-3405
