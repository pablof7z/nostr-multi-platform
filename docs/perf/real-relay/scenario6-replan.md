---
scenario: 6-replan
verdict: PASS
generated_at: 1779089088
relays: ["wss://relay.damus.io"]
---

# Scenario 6 — kind:3 change forces subscription re-plan

## Verdict: PASS

Fetched a **real live kind:3** for `jb55` from `wss://relay.damus.io`, extracted **695** `p`-tag followees, and ran them through `nmp_core::planner::SubscriptionCompiler::compile` (the in-process kernel subscription compiler).

## Planner API exercised

- `InterestShape::timeline_for(followees)` → tailing kind:[1,6] timeline interest over the real follow-set.
- `SubscriptionCompiler::new(&InMemoryMailboxCache, &indexer)` then `.compile(&[interest])` → `CompiledPlan`.
- Asserted on the union of `RelayPlan.sub_shapes[].shape.authors` and on `CompiledPlan.plan_id`.

## Filter delta observed

- original REQ author-set size: **695** (== followee count, exact).
- mutation applied: dropped `000000000652e452ee68a01187fb08c899496cb46cb51d1aa0803d063acedba7`, added `deadbeef00000000000000000000000000000000000000000000000000000000`.
- recompiled REQ author-set size: **695**.
- symmetric difference vs original: exactly `{-000000000652e452ee68a01187fb08c899496cb46cb51d1aa0803d063acedba7, +deadbeef00000000000000000000000000000000000000000000000000000000}` — no other author moved.
- `plan_id` changed: `ef09a10d32302c87` → `3671b3f760e1d9e5` (content-addressed identity correctly invalidated).

This proves the kernel re-plans subscriptions correctly when a real follow-graph (kind:3) changes: the compiled REQ filter set tracks the followee delta exactly, and the plan identity flips so the wire-emitter diff would see the change.

## Known gap (M11+) — live actor-side re-subscribe NOT validated here

The actor-side subscription **rewire** leg — driving a live CLOSE/REQ swap over the relay socket in response to a fresher kind:3 — is **not wired yet** (documented in `crates/nmp-testing/tests/real_relay_smoke.rs`). Driving that over a socket today would be a fabricated pass, so this scenario deliberately validates only the **planner-level re-plan** (the layer that exists). The end-to-end live re-subscribe leg remains an explicit M11+ gap: when the actor-side rewire lands, a follow-up scenario should assert the relay actually receives the new REQ / CLOSE frames derived from this same plan diff.
