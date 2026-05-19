# T140 — M2 Planner Hot-Path Audit

**Ticket**: T140  
**Status**: Pre-work only — no code changes in this document  
**Blocked on**: T141 (substrate-types extract, on `origin/t141-rescue`)  
**Companion doc**: `docs/design/drain-tick-driver-spec.md` (T142)

---

## 1. What "M1 hand-rolled req()" means today

The live follow-feed is built and maintained by a single method:

```
crates/nmp-core/src/kernel/ingest/timeline.rs
  timeline_sub_id_for()     — line 15   — generates "seed-timeline-<8-char-hash>"
  maybe_open_timeline()     — line 240  — THE authoritative M1 follow-feed builder
```

`maybe_open_timeline()` does the following:

1. Guards on `self.timeline_requested` (line ~248) — bails if already open.
2. Collects `timeline_authors`: the active account pubkey + `seed_contacts` (line ~252-260).
3. Calls `self.partition_authors_by_write_relays()` to group pubkeys by NIP-65 write relay.
4. For each relay bucket, calls `self.req_for_relay(role, relay_url, sub_id, ...)` directly (line ~267-280).
5. Sets `self.timeline_requested = true` (line ~290).

Supporting M1 plumbing:

```
crates/nmp-core/src/kernel/requests/mod.rs
  req()          — line 117 — routes to role.bootstrap_url(); used for bootstrap/indexer fetches
  req_for_relay() — line 134 — builds a WireSub + OutboundMessage for any URL
```

The M1 path owns `wire_subs` insertion and is the only mechanism that actually opens timeline subscriptions in production today.

### M1 workarounds for follow-list changes

When the active account's kind:3 (follow list) changes at runtime:

```
crates/nmp-core/src/kernel/ingest/contacts.rs  — lines 63-84
```

The ingest handler flips `timeline_requested = false` and closes existing `seed-timeline-*` subs via `defer_outbound` so that the next tick's `maybe_open_timeline()` re-opens with the updated follow set. This is the M1 equivalent of a planner recompile.

Similarly when NIP-65 relay lists change for timeline authors:

```
crates/nmp-core/src/kernel/ingest/relay_list.rs — lines 60-86
```

Same pattern: clear `timeline_requested`, close `seed-timeline-*` subs, let M1 rebuild.

---

## 2. What the M2 planner provides (and why it is dormant)

The planner machinery is fully implemented:

```
crates/nmp-core/src/subs/mod.rs
  SubscriptionLifecycle         — the top-level lifecycle struct
  InterestRegistry              — registry of LogicalInterest entries
  TriggerInbox                  — CompileTrigger queue (A11 FollowListChanged, etc.)
  recompile_and_diff()          — line 318  — full 4-stage compile: resolve→fallback→merge→plan-id
  drain_tick()                  — line 400  — drains inbox, calls recompile_and_diff, returns Vec<WireFrame>
  enqueue_trigger()             — line 302  — queues a CompileTrigger
```

The kernel struct carries `lifecycle: SubscriptionLifecycle` (kernel/mod.rs line 223) with an explicit comment at lines 214-222:

> "M5+M2+M8 wiring: subscription lifecycle. Today the kernel uses ONLY `handle_auth_state_change`... the compile / registry / wire-diff machinery stays dormant because the kernel's M1 hand-rolled `req()` path is still authoritative..."

The `ingest/contacts.rs` file even documents the seam gap at lines 13-24:

> "Seam-gap note: the actor loop must call `lifecycle.drain_tick()` at each tick boundary for this trigger to produce wire frames in production."

So `FollowListChanged` triggers ARE being enqueued (contacts.rs lines 48-53) but nothing drains them.

---

## 3. Where the surgery must happen (line-level)

### 3.1 Actor command — OpenTimeline

```
crates/nmp-core/src/actor/dispatch.rs — line 204-208
crates/nmp-core/src/actor/commands/publish.rs — open_timeline() at lines 194-203
```

`open_timeline()` currently calls `kernel.open_author(pk, relays_ready)` which opens a PROFILE subscription for the active account — NOT a follow-feed. This is incorrect semantics.

**Required change**: `open_timeline()` should:
1. Register one `LogicalInterest` per followed pubkey in `kernel.lifecycle_mut().registry_mut()`:
   - Shape: kind 1+6, `InterestLifecycle::Tailing`, author = pubkey, relay hints from known mailbox
2. Enqueue `CompileTrigger::FollowListChanged` into `lifecycle.inbox`.
3. NOT call `maybe_open_timeline()` directly.

### 3.2 Actor idle loop — drain_tick() wiring

```
crates/nmp-core/src/actor/mod.rs — lines 405-447
```

The idle work section currently calls:
- `pending_view_requests()` (line ~416) — M1 dispatcher
- `tick_publish_engine_for_now()`
- `flush_due` / `emit_now`

`drain_tick()` must be called here, after `pending_view_requests()`, passing the `KernelMailboxes` adapter built from `kernel.author_relay_lists`. The returned `Vec<WireFrame>` must be converted to `Vec<OutboundMessage>` before passing to `send_all_outbound()`.

See companion spec (`drain-tick-driver-spec.md`) for the exact call site and conversion.

### 3.3 contacts.rs — remove M1 workaround

```
crates/nmp-core/src/kernel/ingest/contacts.rs — lines 63-84
```

The M1 workaround (flip `timeline_requested = false`, close subs via `defer_outbound`) can be removed once `drain_tick()` is live. The `FollowListChanged` trigger already enqueued at lines 48-53 is sufficient — the planner will see the updated follow set on the next compile and emit the correct CLOSE + REQ diff.

### 3.4 relay_list.rs — remove M1 workaround

```
crates/nmp-core/src/kernel/ingest/relay_list.rs — lines 60-86
```

Same: the `RelayHealthChanged` trigger (or mailbox-snapshot staleness detection at compile time) makes the explicit `timeline_requested = false` + close dance unnecessary.

### 3.5 timeline.rs — retire maybe_open_timeline()

```
crates/nmp-core/src/kernel/ingest/timeline.rs — lines 240-~295
```

Once the planner owns the follow-feed, `maybe_open_timeline()` should be removed (or kept only for a feature-flag fallback during transition). The fields `timeline_requested: bool` and `timeline_authors: HashSet<Pubkey>` on `KernelState` can be retired.

---

## 4. Type impedance: WireFrame vs OutboundMessage

The M1 path produces `OutboundMessage { role, relay_url, text }`.  
The M2 path (`drain_tick()`) produces `Vec<WireFrame>`.

The actor's `send_all_outbound()` takes `Vec<OutboundMessage>`.

A conversion function is needed:

```rust
fn wire_frame_to_outbound(frame: WireFrame) -> OutboundMessage { ... }
```

`WireFrame` already carries `relay_url` and `text`; `role` must be derived from the relay URL using the kernel's relay-role index (or passed through `SubShape`). This is non-trivial — `drain-tick-driver-spec.md` §3.2 covers the options.

---

## 5. Test coverage gaps

### Currently tested (M2 planner in isolation)

```
crates/nmp-testing/tests/m8_subscription_lifecycle.rs — lines 242, 252, 358
crates/nmp-testing/tests/c5_c8_c13.rs — lines 126, 165, 169
```

`drain_tick()` is only called in test code. The planner compiles correctly; plan diffs work.

### Missing coverage (must be added before this PR merges)

| Gap | What to test | Test location |
|-----|-------------|---------------|
| Follow-list → wire frames end-to-end | Register N interests, `drain_tick`, assert N REQ frames emitted | `nmp-testing/tests/` or `kernel/integration_tests.rs` |
| FollowListChanged trigger → recompile | Enqueue trigger, drain, assert plan changes | Same |
| M1 workaround removal | After contacts.rs M1 code is removed, follow-list update still produces new subs | `kernel/ingest/contacts_tests.rs` |
| relay_list change → re-route | NIP-65 update → new relay URL → planner emits CLOSE on old + REQ on new | `kernel/ingest/relay_list_tests.rs` |
| WireFrame → OutboundMessage conversion | Role derivation correct for outbox relays vs bootstrap | Unit test in `actor/` |
| `open_timeline()` → interest registration | Actor command results in registry entries, not `open_author` call | `actor/commands/` test |
| Empty drain tick | No triggers queued → no compile, no frames (regression guard) | Already exists (`empty_tick_does_not_compile`) |

---

## 6. T141 rebase risk

T140 touches:
- `kernel/ingest/contacts.rs` (remove M1 workaround)
- `kernel/ingest/relay_list.rs` (remove M1 workaround)
- `kernel/ingest/timeline.rs` (retire `maybe_open_timeline()`)
- `actor/mod.rs` (add `drain_tick()` call)
- `actor/commands/publish.rs` (`open_timeline()` redirect)
- `kernel/mod.rs` (retire `timeline_requested`/`timeline_authors` fields)

T141 (`origin/t141-rescue`) touches:
- `kernel/ingest/mod.rs` (M7 explicit dispatch arms for kinds 7/1111/9735)
- `crates/nmp-substrate-types/` (new crate, trait seam)
- New files: `kernel/ingest/reactions.rs`, `kernel/ingest/comments.rs`, `kernel/ingest/zaps.rs`

**Overlap assessment**:
- `kernel/ingest/contacts.rs`, `relay_list.rs`, `timeline.rs` — NOT touched by T141. Rebase risk: **LOW**.
- `kernel/ingest/mod.rs` — T141 adds M7 arms; T140 does not add arms (it removes a `maybe_open_timeline` call). Risk: **LOW** (no textual conflict expected if T141 lands first).
- `actor/mod.rs` — T141 does NOT touch this file. Risk: **NONE**.

**Recommended merge order**: Land T141 first (substrate-types extract), then T142 (drain_tick wiring), then T140 (M1→M2 migration). T140 must NOT land before T142 since it depends on `drain_tick()` being called.

---

## 7. Effort estimate

| Sub-task | Estimated effort |
|----------|-----------------|
| `open_timeline()` redirect + interest registration | 2-3h |
| `drain_tick()` call in actor idle loop + WireFrame→OutboundMessage conversion | 2-4h (conversion is the hard part) |
| Remove M1 workarounds in contacts.rs + relay_list.rs | 1h |
| Retire `maybe_open_timeline()` + kernel fields | 1h |
| New test coverage (table in §5) | 4-6h |
| **Total** | **10-14h** |

The largest unknown is the `WireFrame → OutboundMessage` role-derivation problem. If `SubShape` can carry the `RelayRole` directly from the compile context (which it has access to via `MailboxSnapshot`), this collapses to a simple map. If not, a relay-URL-to-role lookup table must be built from `kernel.relay_health`.

---

## 8. Recommended implementation order

1. (T141 must land first — `origin/t141-rescue` → master)
2. T142: wire `drain_tick()` into actor idle loop with no behavior change (empty registry = no-op compile)
3. T140 Step A: redirect `open_timeline()` to register interests + enqueue trigger; keep M1 path as fallback
4. T140 Step B: verify wire frames are equivalent to M1 output; run integration tests
5. T140 Step C: remove M1 workarounds (contacts.rs, relay_list.rs, timeline.rs)
6. Test coverage sweep (§5 gap table)
