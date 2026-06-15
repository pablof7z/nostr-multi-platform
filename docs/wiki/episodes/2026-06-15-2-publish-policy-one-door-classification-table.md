---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - publish-policy-one-door
  - d10-privacy-fail-closed
  - publish-routing-classification
supersedes:
  - 2026-06-15-2-publish-policy-one-door-replaces-scattered
related_claims: []
source_lines:
  - 2520-2546
  - 2978-2984
  - 3056-3069
captured_at: 2026-06-15T14:27:03Z
---

# Episode: Publish policy one-door classification table (Workstream C)

## Prior State

Publish-kind routing used scattered raw literal guards (`if kind == 0`, `if kind == 3` in `publish/action.rs`; `raw.kind == KIND_GIFT_WRAP && Auto` in `actor/commands/publish.rs`). No single source of truth for kind→policy decisions. Initial Workstream C implementation declared `PrivateFailClosed` in `classify_publish_behavior` but did not enforce it — a gift-wrap/sealed DM with `Auto` target could pass through and leak to public relays. The old D10 literal guard in `actor/commands/publish.rs` only covered kind:1059, not kind:14 (sealed DMs). The reintroduction gate only scanned `action.rs` for `kind==N` substrings — near-vacuous.

## Trigger

Codex review caught three blockers in initial Workstream C: (1) `PrivateFailClosed` declared but not enforced — real D10 privacy leak (gift-wrap + Auto routes to public relays), (2) old literal guard still lived at `actor/commands/publish.rs:400` — policy.rs was not the single door, (3) reintroduction gate near-vacuous — only scanned `action.rs`, missed the old guard entirely.

## Decision

`classify_publish_behavior(kind)` in `policy.rs` is the sole declared policy table — the only function permitted to compare a publish kind to a named KIND_* constant. Four typed behaviors: `ReservedBuilderOnly` (kind:0/3), `PrivateFailClosed` (kind:1059/14), `DiscoveryIndexable` (relay lists + 10000–19999), `PublicRoutable` (default). `validate_publish_routing(kind, is_explicit_nonempty)` structurally enforces PrivateFailClosed at both the action boundary AND the publish-engine chokepoint (`publish_engine.rs::run_publish_engine_at`) — every signed publish path funnels through this single engine entry. Old literal guards deleted. Reintroduction gate scans the full routing surface (action.rs, actor/commands/publish.rs, kernel/publish_cmd.rs, kernel/publish_engine.rs) and bans any `kind ==/!= <int>` guard outside policy.rs — non-vacuity proven by re-injection test.

## Consequences

- D10 privacy leak structurally closed: gift-wrap/sealed DM with Auto or empty Explicit is rejected at both action boundary and publish-engine chokepoint
- kind:14 sealed DMs now covered (old guard only covered kind:1059)
- No scattered kind literals permitted in the publish routing surface — single-door invariant enforced by gate
- Gate non-vacuity proven: re-injecting `signed.unsigned.kind == 1059` into `publish_cmd.rs` fails the gate
- Rejection messages byte-preserved from old guards (behavioral equivalence for reserved kinds)

## Open Tail

- Codex re-review of reworked Workstream C in flight — must confirm no bypass path for private-event leak
- Workstream F (doctrine gates) still queued — runtime enforcement is structural, lint/CI gates pending
- Local publish intent kind:0/3 literal at `local_publish_intent.rs:52` correctly left out of scope (PR 1 deletes that file wholesale)

## Evidence

- transcript lines 2520-2546
- transcript lines 2978-2984
- transcript lines 3056-3069
