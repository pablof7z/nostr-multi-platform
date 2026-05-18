# 14 — Subscription lifecycle + relay manager + NIP-42

> Status: **SHIPS** · Audience: **agents** · Doctrine: **D3** (routing
> verbatim), D4, D6, D7, D8.
>
> This is **M8-subs** (subscription lifecycle), *distinct from* M8
> multi-account (session scope — that is [11 — Sessions + signers + identity scopes](11-sessions-signers.md)). The
> plan that governs this section is `docs/plan/m8-subscription-lifecycle.md`
> — not `m8-multi-account.md`. Conflating them is anti-pattern #5.

`CompiledPlan` (from the M2 planner — see
[07 — Subscription planner — Interest → CompiledPlan → wire](07-subscription-planner.md)) is a deterministic
per-relay `SubShape` mapping. M8-subs is everything *between* that plan and
the wire. It ships **seams**, not protocol logic: M4 (negentropy), M5
(NIP-42), M7 (publish) each plug in instead of growing parallel
connection-pool plumbing.

## The four M8-subs seams

Per `docs/plan/m8-subscription-lifecycle.md:15-32` and the module doc
(`crates/nmp-core/src/subs/mod.rs:6-20`):

1. **`InterestRegistry`** — single writer of the active-interest set (D4).
   View/action modules `push`; the planner only reads via `iter_active()`.
   Backed by `BTreeMap<InterestId, _>` so snapshots are deterministically
   id-ordered — required for plan-id stability
   (`crates/nmp-core/src/subs/registry.rs:22-47`). Pushing the same
   `InterestId` replaces; that is the *only* legal way to mutate an
   interest's filter.
2. **`TriggerInbox`** — FIFO fan-in + per-tick coalescing (D8). N enqueued
   triggers between two `drain_tick()` calls fold into **one** compile pass;
   an empty inbox is a zero-allocation no-op
   (`crates/nmp-core/src/subs/inbox.rs:23-61`). The eleven canonical
   triggers are in `crates/nmp-core/src/subs/trigger.rs:71-132`
   (A1–A11). `RelayReconnected` is pure-replay: `requires_recompile()`
   returns `false` for it (`trigger.rs:143-145`).
3. **Wire-emitter** — `plan_diff(prior, next, interests)` →
   `Vec<WireFrame>` (`crates/nmp-core/src/subs/wire.rs:51-89`). Computes the
   minimal `REQ`/`CLOSE` delta. `plan_diff(P, P)` is empty (idempotence
   contract). Sub-ids derive from the shape's `canonical_filter_hash`,
   **not** the plan-id (`wire.rs:133-135`) — including the plan-id would
   force a CLOSE+REQ on every plan-id churn, defeating the diff.
4. **`ConnectionPool`** — uniform send-path shared by M2/M4/M5/M7
   (`crates/nmp-core/src/subs/pool.rs:34-54`). D7: it reports, the actor
   decides. It never spawns workers, never retries, never sets reconnect
   policy. Send to a disconnected relay → `PoolSendOutcome::Deferred` into a
   bounded (cap 64) per-relay FIFO; reconnect → actor calls
   `drain_deferred` and re-sends (no implicit retry).

Two further gates compose with the seams inside `SubscriptionLifecycle`
(`subs/mod.rs:75-92`): the **`LifecycleGate`** (OneShot CLOSE on EOSE,
BoundedTime CLOSE on deadline — `subs/lifecycle_gate.rs:26-101`) and the
**`AuthGate`** (NIP-42 REQ pause/flush — `subs/auth_gate.rs:19-73`).

> Module-visibility note: `subs::{registry,inbox,trigger,wire,pool,auth_gate,lifecycle_gate}`
> are `pub(crate) mod`; the public surface is the re-exports in
> `subs/mod.rs:56-60` (`SubscriptionLifecycle`, `plan_diff`, `WireFrame`,
> `InterestRegistry`, `CompileTrigger`, `ConnectionPool`, …). Cite the
> module files for behaviour; reach the types via `subs::`.

## Connection-state diagram

```text
                relay_worker (real WebSocket; relay_worker.rs)
                ┌───────────────────────────────────────────┐
   open_socket  │                                           │
  ──────────────▶ Connecting ──Connected──▶  Live           │
                │     │  ▲                     │  ▲          │
                │  Failed │ wait 3s (RELAY_    │  │ Message  │
                │     │   │  RECONNECT_DELAY)  │  │          │
                │     ▼   │                    ▼  │          │
                │   Reconnect ◀──Failed/IO────  read loop    │
                └───────────────────────────────────────────┘

ConnectionPool view (subs/pool.rs):  Connected ⇄ Disconnected
   send(Connected)    → Sent      (recorded in sent_log)
   send(Disconnected) → Deferred  (bounded FIFO, cap 64)
                       → DroppedOverflow when the cap is exhausted
   reconnect          → actor calls drain_deferred() → re-send (no auto-retry, D7)
```

The actor side (`crates/nmp-core/src/actor/relay_mgmt.rs`) owns spawn /
close / route. `send_all_outbound` (`relay_mgmt.rs:77-93`) is the **single
choke point**: every view-open path routes through it, and it runs
`kernel.partition_auth_paused` before the wire so AUTH-paused REQs are
diverted regardless of which kernel method built them. The live worker still
uses the two hardcoded constants `wss://relay.primal.net` +
`wss://purplepag.es` (`crates/nmp-core/src/relay.rs:1-2`) — the planner can
route to mailboxes but is not yet wired into the actor's REQ path (see
[27 — Doc/code discrepancies](27-discrepancies.md) for this wiring gap).

## NIP-42: challenge → response → re-emit

The canonical state machine is in `crates/nmp-nip42/src/state.rs:23-46`
(mirrors ADR-0007 §1). The placeholder `subs::trigger::RelayAuthState`
(`subs/trigger.rs:34-47`) is the seam type; `nmp-nip42` owns the canonical
one and translates one-way via `relay_auth_state_to_subs`
(`state.rs:87-95`).

```text
NotRequired
   │  relay sends ["AUTH", <challenge>]  (frame.rs:parse_auth_frame)
   ▼
ChallengeReceived              ── Nip42Driver::on_auth_frame (flow.rs:133-140)
   │  caller invokes signer; build_auth_event → kind:22242 template
   │  (builder.rs:19-34, tags = [["relay",url],["challenge",val]])
   │  deliver_signed[_for] validates structural shape (builder.rs:43-74)
   ▼
Authenticating                 ── wire_frame ["AUTH", <signed kind:22242>]
   │  relay returns ["OK", <event_id>, <accepted>, <reason>]
   │  on_ok_frame matches event_id against pending kind:22242
   ▼                                  ▼
Authenticated                       Failed   (OK false, signer error, bad sig)
  subs::AuthGate flushes held REQs   stays held until reconnect resets
```

Sequence across the seams:

1. Relay → `["AUTH", challenge]`. Kernel parses (`frame.rs:33-45`), feeds
   `Nip42Driver::on_auth_frame` → `ChallengeReceived`, no wire frame yet.
2. Caller invokes the signer (sync `LocalKeySigner`, or async NIP-46 via
   `deliver_signed_for` with the challenge for race-safety —
   `flow.rs:186-200`). `deliver_signed` → `Authenticating` and emits the
   `["AUTH", <event>]` wire frame.
3. Relay → `["OK", id, true/false, reason]`. `on_ok_frame` is a **no-op**
   unless `state == Authenticating` *and* the id matches the pending
   kind:22242 (`flow.rs:240-260`) — publish OKs from the M7 engine are not
   this driver's concern.
4. On `Authenticated`, the auth-state transition fans a
   `CompileTrigger::RelayAuthStateChanged` into the inbox;
   `AuthGate::record_transition` drains the per-relay pending REQ buffer
   (`auth_gate.rs:42-54`). CLOSE frames **always** pass through the gate
   even while paused (`auth_gate.rs:59-73`) — you must be able to close
   stale subs on a paused relay (e.g. logout mid-connection).
5. Disconnect → `Nip42Driver::reset_on_disconnect` → `NotRequired`
   (`flow.rs:119-123`); a fresh challenge on the next connect re-runs the
   handshake. Logical subscriptions are **not** re-issued by the handshake
   (M5 exit gate, `docs/plan/m5-nip42.md:20`).

## What happens on reconnect to live REQs

- `SubscriptionLifecycle::handle_reconnect(relay_url)`
  (`subs/mod.rs:205-232`) is **pure replay**: it re-emits the *current
  plan's* sub-shapes as `REQ` frames to that relay only. The planner /
  compiler is **not** invoked (recompilation.md §4.2 — A5 is not a
  recompile).
- If there is no current plan or the relay is absent from it, the result is
  an empty frame vec — no error (D6).
- Sub-ids are recomputed from the same `canonical_filter_hash`, so a replay
  is wire-identical to what was live; the relay sees the same sub-ids.
- The deferred queue is orthogonal: the actor calls
  `ConnectionPool::drain_deferred` on reconnect and re-sends those frames
  FIFO (`pool.rs:130-135`). No implicit retry (D7).
- M4's negentropy gap-fill is a *separate* trigger
  (`RelayReconnected` into the `nmp-nip77` `TriggerEngine`) — see
  [13 — Sync engine](13-sync-engine.md). Reconnect = replay live REQ tail **plus**
  schedule a coverage-aware gap fill, not re-fetch from scratch.

## CLOSED / EOSE / lifecycle

- **EOSE** → `LifecycleGate::on_eose` closes only `OneShot` subs; `Tailing`
  and `BoundedTime` are no-ops (`lifecycle_gate.rs:64-81`). An EOSE for an
  unknown sub-id is a silent no-op.
- **BoundedTime** subs CLOSE when `tick_deadlines(now_ms)` crosses
  `until_ms`, EOSE or not (`lifecycle_gate.rs:84-101`).
- **CLOSED** (relay-initiated `["CLOSED", sub_id, reason]`) is *transient*,
  not fatal: the logical interest is unchanged in the registry, so the next
  recompile/replay re-issues the REQ. There is no app-visible error path —
  treating CLOSED as terminal (anti-pattern #2) drops a sub the framework
  would otherwise restore.

## The PlanCoverageHook seam (M4 ↔ M8-subs)

`SubscriptionLifecycle` owns a post-compile `PlanCoverageHook`
(`subs/mod.rs:40-54, 116-124`). The actor installs
`nmp_nip77::apply_coverage_filter` at startup; `nmp-core` never names the
hook (D0 — kernel grows no NIP-77 nouns). The hook runs between `compile()`
and `plan_diff()`. This is the seam [13 — Sync engine](13-sync-engine.md) describes from
the `nmp-nip77` side.

## Anti-patterns

1. **Re-sending REQs from app code on reconnect.** `handle_reconnect`
   replays the current plan automatically. App-side replay double-issues
   subs and races the deferred-queue drain.
2. **Treating CLOSED (or a transient disconnect) as fatal.** It is
   transient; the registry keeps the interest and the framework re-issues.
   Fatal handling silently loses a subscription.
3. **Opening per-view connections.** There is one pool, one send-path; views
   push `LogicalInterest`, they do not own sockets. Per-view connections
   defeat coalescing and the auth gate.
4. **Blind write replay after auth.** After `Authenticated`, only the
   *gated* pending REQ buffer is flushed (`AuthGate`). Replaying everything
   you queued during the pause re-sends frames that CLOSE'd in the interim.
5. **Confusing M8-subs with M8-multi-account.** This section is the
   connection-pool / trigger-fan-in concern. Account-scope rebuild-on-switch
   is [11 — Sessions + signers + identity scopes](11-sessions-signers.md). The plan files are
   `m8-subscription-lifecycle.md` vs `m8-multi-account.md`.
6. **Including plan-id in the wire sub-id.** It forces a full CLOSE+REQ on
   every plan-id change. Sub-ids must derive from `canonical_filter_hash`
   only (`wire.rs:133-135`).

## Deliverables recap

- **Connection-state diagram** — worker FSM + pool view (above).
- **NIP-42 challenge → response → re-emit sequence** — the 5-step sequence
  and the FSM diagram (above).
- **"What happens on reconnect to live REQs"** — the bullet list (above).
- **The four M8-subs seams** — registry / inbox / wire-emitter / pool
  (above).

See also: [07 — Subscription planner — Interest → CompiledPlan → wire](07-subscription-planner.md) · [12 — Publishing + the publish engine](12-publish-and-ledger.md) · [13 — Sync engine — `nmp-nip77` (NIP-77 first, REQ second)](13-sync-engine.md).
