---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - publish-policy-one-door
  - private-fail-closed
  - gift-wrap-leak
  - d10-privacy
supersedes:
  - 2026-06-15-1-d10-private-fail-closed-enforced-at
related_claims: []
source_lines:
  - 2978-2984
  - 3056-3061
  - 3107-3115
  - 3137-3151
  - 3173-3183
  - 3257-3275
  - 3378-3406
captured_at: 2026-06-15T15:26:03Z
---

# Episode: D10 private-event leak closed at universal dispatch-emit site

## Prior State

PrivateFailClosed was declared as a policy but not structurally enforced: gift-wrap (1059) and sealed DM (14) events with Auto routing could reach public relays. Old literal guard existed only at actor/commands/publish.rs covering kind:1059 but not 14. Resume-from-store and manual retry bypassed even that guard entirely.

## Trigger

Codex review of initial Workstream C implementation found 3 blockers: (1) PrivateFailCentral declared but unenforced, (2) old guard not single-door and misses kind:14, (3) reintroduction gate vacuous. Second review round found resume/retry paths bypass the entry gate entirely.

## Decision

Enforcement moved to the universal dispatch-emit site: `dispatch_due` in publish/engine/helpers.rs, where initial publish, resume-from-store, manual retry, and availability re-dispatch all converge. Gate checks persisted per-relay `relay_reasons` — PrivateFailClosed kinds may emit only to relays with `RelaySelectionReason::Explicit`. Refused rows are terminally finalized and deleted from durable store (extracted `finalize_completed_rows` reused from tick/on_ack). Entry gates kept as defense-in-depth fail-fast.

## Consequences

- No publish path can Auto-route a private envelope to a public relay, including after restart/resume
- Persisted private rows with stale Auto targets are dropped on replay rather than leaked
- Refused rows do not linger Pending in durable store (no re-refusal loop on restart)
- Old scattered kind==N guards deleted; policy.rs is the sole kind→policy decision home
- Broadened reintroduction gate scans match/matches!/.contains evasion shapes across full routing surface

## Open Tail

- Raw-event forwarder (raw_event_forwarder.rs) emits EVENT frames separately but default policy only forwards replaceable kinds — out of scope but should be documented as an independent path

## Evidence

- transcript lines 2978-2984
- transcript lines 3056-3061
- transcript lines 3107-3115
- transcript lines 3137-3151
- transcript lines 3173-3183
- transcript lines 3257-3275
- transcript lines 3378-3406
