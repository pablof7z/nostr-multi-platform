---
type: episode-card
date: 2026-05-26
session: 64f3e239-c4c1-4c32-82de-458516b28418
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/64f3e239-c4c1-4c32-82de-458516b28418.jsonl
salience: architecture
status: active
subjects:
  - bootstrap-self-kinds
  - interest-lifecycle
  - is-indexer-discovery
  - planner-gate
supersedes: []
related_claims: []
source_lines:
  - 1-50
  - 1198-1236
  - 1293-1483
  - 1767-1803
captured_at: 2026-06-18T05:42:58Z
---

# Episode: Tailing self-kind subscriptions replace OneShot bootstrap

## Prior State

All four bootstrap interests (kinds 0, 3, 10002, 10050) used `InterestLifecycle::OneShot` with `limit: Some(1)`, closing after EOSE. External kind:3 publishes from other clients (e.g. Primal) were invisible — the subscription had already closed. The planner gate in `case_a_authors.rs` inferred discovery routing via `OneShot && Global`, a structural check that would break if interests became Tailing.

## Trigger

User observed chirp-tui not receiving follow-list updates from other clients; diagnosed that `bootstrap:self-contacts` is OneShot — it fetches kind:3 once then closes. User directed: 'kinds = 0, 3, 10002, 10000, 10006 — keep the subscription open. We don't need limit:1; relays will automatically send us the right thing.'

## Decision

Replace OneShot+limit:1 bootstrap with a single Tailing subscription over kinds [0, 3, 10002, 10000, 10006] (kind:10050 remains OneShot as a discovery probe). Add `is_indexer_discovery: bool` sentinel field (with `#[serde(default)]`) to `LogicalInterest` so the planner gate uses an explicit flag rather than structural lifecycle inference. Make the self-kinds set configurable via `bootstrap_self_kinds_override` slot on `NmpApp`. Remove `limit: Some(1)` — replaceable-event semantics make it unnecessary.

## Consequences

- External kind:3 publishes now arrive on the open sub → `ingest_contacts` on Replaced → `sync_follow_feed_interests` → `FollowListChanged` → planner closes removed subs and opens new ones — full reactivity without any app code knowing
- The `is_discovery_oneshot` gate in `case_a_authors.rs` replaced by `is_indexer_discovery` flag check — Tailing interests can now also use the bootstrap indexer lane when flagged
- Account-switch safety: `drop_owner`+`set_sub` pattern replaces `ensure_sub` so switching accounts replaces the author in-place rather than silently keeping the old pubkey
- Plan-identity hashes change due to the new field on `LogicalInterest` — existing subscription keys will recompile on upgrade
- Kind:10050 (NIP-17 DM relay list) excluded from Tailing set — NIP-17 runtime owns it via a separate OneShot discovery path

## Open Tail

- Existing `register_bootstrap_interest` helper still exists but is now used only for kind:10050; the Tailing path uses `register_tailing_self_kinds_interest` — consider whether to rename or unify

## Evidence

- transcript lines 1-50
- transcript lines 1198-1236
- transcript lines 1293-1483
- transcript lines 1767-1803

