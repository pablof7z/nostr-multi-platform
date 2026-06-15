---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - d10-private-fail-closed
  - publish-policy-one-door
  - dispatch-due-gate
supersedes:
  - 2026-06-15-3-publish-policy-one-door-with-universal
related_claims: []
source_lines:
  - 2978-2984
  - 3103-3115
  - 3137-3157
  - 3257-3275
  - 3378-3383
  - 3394-3404
captured_at: 2026-06-15T15:14:58Z
---

# Episode: D10 private fail-closed enforced at universal dispatch-emit site

## Prior State

Publish policy for private kinds (1059/14) was declared PrivateFailClosed but not structurally enforced; the old literal guard at actor/commands/publish.rs:400 covered only kind:1059 (missed kind:14) and was not the single door; resume-from-store and manual retry paths bypassed the entry gate entirely, allowing a persisted private row to leak on restart/retry

## Trigger

Codex review caught that PublishRaw { kind:1059, target:Auto } would sign and route via Auto to public relays — a real D10 privacy leak; subsequent review rounds found resume/retry paths bypass the run_publish_engine_at entry gate, and the reintroduction gate was near-vacuous (missed the old guard in publish_cmd.rs)

## Decision

Enforce at the universal dispatch_due emit site where initial publish, resume-from-store, manual retry, and availability-redispatch all converge; policy::relay_emit_is_sanctioned(kind, relay_reasons) gates every frame immediately before dispatcher.dispatch; a PrivateFailClosed kind may emit only to a relay whose relay_reasons includes Explicit; refused rows terminally finalized via the same finalize_completed_rows path that tick/on_ack use (no new logic); policy.rs is the sole home for kind→policy decisions; old scattered guard deleted

## Consequences

- No publish path (initial/resume/retry/availability-redispatch) can Auto-route a private envelope to public relays
- Refused private rows are terminally deleted from the durable store — never left Pending, never re-refused on next resume
- Reintroduction gate scans the full routing surface (publish/action.rs, actor/commands/publish.rs, kernel/publish_cmd.rs, kernel/publish_engine.rs, engine/helpers.rs, engine/dispatch.rs) and catches ==/match/matches!/.contains evasion shapes
- Gate non-vacuity proven: re-injecting the old raw.kind == KIND_GIFT_WRAP guard or a kind==<int> into publish_cmd.rs causes the gate to FAIL

## Open Tail

*(none)*

## Evidence

- transcript lines 2978-2984
- transcript lines 3103-3115
- transcript lines 3137-3157
- transcript lines 3257-3275
- transcript lines 3378-3383
- transcript lines 3394-3404
