# T142 — drain_tick() Driver Spec

**Ticket**: T142  
**Status**: Pre-work only — no code changes in this document  
**Blocked on**: T141 (substrate-types extract, on `origin/t141-rescue`)  
**Companion doc**: `docs/design/m2-hot-path-audit.md` (T140)

---

## 1. drain_tick() contract

```rust
// crates/nmp-core/src/subs/mod.rs — line 400
pub fn drain_tick(
    &mut self,
    mailboxes: &dyn MailboxCache,
) -> Vec<WireFrame>
```

**Behavior** (per current implementation):

1. Drains `self.inbox` (`TriggerInbox`) — coalesces all pending `CompileTrigger` variants into a single compile pass. Per-tick discipline (D8): N triggers → at most 1 compile, never N compiles.
2. For `CompileTrigger::RelayAuthStateChanged { url, state }` — applies side effects to `AuthGate` before the compile pass.
3. If no triggers are queued (inbox empty): returns `Vec::new()` immediately — **zero-cost no-op**. This is the common case on a quiet tick.
4. Calls `recompile_and_diff()` with the provided `mailboxes` snapshot.
5. Returns `Vec<WireFrame>` — one entry per wire action (REQ to open, CLOSE to retire).

**Guarantee**: Idempotent across ticks when the interest registry and mailbox state are unchanged. Repeated calls with an empty inbox produce no frames and no recompile.

**NOT a guarantee**: `drain_tick()` does not check `wire_subs` itself — it relies on the `plan_diff()` inside `recompile_and_diff()` to compare the new compiled plan against the previously emitted plan. The emitted-plan state lives inside `SubscriptionLifecycle`, not in `Kernel.wire_subs`. This means:

- The `wire_subs` map (owned by `Kernel`) and the lifecycle's emitted-plan record can drift if frames are lost or the actor resets subs externally. Post-T140 work should audit this invariant.

---

## 2. Current call sites

**Production code**: `drain_tick()` has **zero production call sites**.

Confirmed by exhaustive search across all crates:

```
crates/nmp-core/src/actor/mod.rs        — no call
crates/nmp-core/src/actor/dispatch.rs   — no call
crates/nmp-core/src/kernel/**           — no call
```

**Test-only call sites**:

```
crates/nmp-testing/tests/m8_subscription_lifecycle.rs — lines 242, 252, 358
crates/nmp-testing/tests/c5_c8_c13.rs                — lines 126, 165, 169
```

The seam gap is explicitly documented in `kernel/ingest/contacts.rs` lines 13-24:

> "Seam-gap note: the actor loop must call `lifecycle.drain_tick()` at each tick boundary for this trigger to produce wire frames in production."

---

## 3. Where drain_tick() must be called

### 3.1 Call site: actor idle loop

**File**: `crates/nmp-core/src/actor/mod.rs`  
**Approximate line**: 416 (after the `pending_view_requests()` call in the idle work block, lines 405-447)

The idle work block currently:

```rust
// actor/mod.rs ~405-447
let outbound = kernel.pending_view_requests();   // M1 dispatcher
self.send_all_outbound(outbound);
self.tick_publish_engine_for_now(&mut kernel);
// ... flush_due, emit_now ...
```

The new call should be inserted after `pending_view_requests()` drains M1 requests:

```rust
// NEW: M2 planner tick
let mailboxes = KernelMailboxes::from(&kernel);
let wire_frames = kernel.lifecycle_mut().drain_tick(&mailboxes);
if !wire_frames.is_empty() {
    let outbound = wire_frames_to_outbound(wire_frames, &kernel);
    self.send_all_outbound(outbound);
}
```

**Rationale for placement after `pending_view_requests()`**: The M1 and M2 paths must not race for the same sub-id space. M1 reserves `seed-timeline-*` prefixes; M2 will use planner-derived content-addressed sub-ids. Placing M2 after M1 ensures any M1 CLOSE frames (e.g., from the M1 workaround for follow-list changes in contacts.rs) are already enqueued before M2 opens new subs.

### 3.2 WireFrame → OutboundMessage conversion

`drain_tick()` returns `Vec<WireFrame>`. The actor's `send_all_outbound()` accepts `Vec<OutboundMessage>`.

`WireFrame` carries `relay_url: String` and `text: String` (the JSON-encoded REQ/CLOSE frame). It does NOT carry `RelayRole`.

`OutboundMessage` requires `role: RelayRole`.

**Option A — Role lookup from relay URL** (preferred):

```rust
fn wire_frames_to_outbound(
    frames: Vec<WireFrame>,
    kernel: &Kernel,
) -> Vec<OutboundMessage> {
    frames.into_iter().map(|f| {
        let role = kernel.role_for_relay_url(&f.relay_url)
            .unwrap_or(RelayRole::Content);  // fallback: content lane
        OutboundMessage { role, relay_url: f.relay_url, text: f.text }
    }).collect()
}
```

`Kernel::role_for_relay_url()` does not currently exist. It must be added — a lookup over `kernel.relay_health` (or the relay-role index already maintained by `relay_lifecycle.rs`). This is a small addition.

**Option B — Thread role through SubShape**:

`SubShape` is constructed during `recompile_and_diff()` and has access to `RoutingSource` which knows the relay URL's role. Adding a `role: RelayRole` field to `WireFrame` eliminates the lookup at conversion time. This is cleaner but requires changing `WireFrame`'s definition (cross-cutting impact on all test call sites).

**Recommendation**: Option A for the initial wiring (least churn); Option B when `WireFrame` is stabilized for M11.

### 3.3 KernelMailboxes adapter

`drain_tick()` takes `&dyn MailboxCache`. The kernel owns `author_relay_lists: HashMap<Pubkey, RelayList>` (or similar — populated by NIP-65 ingest).

A thin adapter `KernelMailboxes` must implement `MailboxCache` over this map:

```rust
struct KernelMailboxes<'a> {
    relay_lists: &'a HashMap<Pubkey, RelayList>,
}

impl MailboxCache for KernelMailboxes<'_> {
    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot> {
        self.relay_lists.get(pubkey).map(MailboxSnapshot::from)
    }
}
```

This adapter was introduced conceptually in T132 when `SubscriptionLifecycle` stopped owning mailbox state. The exact field path in `Kernel` needs to be confirmed against current `kernel/mod.rs` — the audit found `author_relay_lists` referenced in `kernel/ingest/relay_list.rs` as the store target for kind:10002 events.

---

## 4. Test plan

### Unit tests (nmp-core)

| Test | Location | What it proves |
|------|----------|----------------|
| `drain_tick_empty_inbox_returns_no_frames` | `subs/mod.rs` or `subs/tests.rs` | Zero-cost no-op when no triggers queued |
| `drain_tick_follow_list_changed_emits_req_frames` | `subs/tests.rs` | A11 trigger + follow interests → REQ frames returned |
| `drain_tick_relay_auth_changed_applies_side_effect` | `subs/tests.rs` | `RelayAuthStateChanged` → `AuthGate` state updated before compile |
| `drain_tick_coalesces_multiple_triggers` | `subs/tests.rs` | N triggers → exactly 1 compile (not N compiles) |
| `wire_frames_to_outbound_role_lookup` | `actor/tests.rs` | `WireFrame` with known relay URL → correct `RelayRole` |
| `wire_frames_to_outbound_unknown_url_fallback` | `actor/tests.rs` | URL not in role index → `RelayRole::Content` fallback |

### Integration tests (nmp-testing)

| Test | File | What it proves |
|------|------|----------------|
| `t142_actor_idle_loop_drains_tick` | `nmp-testing/tests/` | Actor idle tick calls `drain_tick`; a queued trigger produces wire frames that reach the mock relay |
| `t142_follow_list_update_produces_wire_frames_e2e` | `nmp-testing/tests/` | Full path: contacts ingested → trigger queued → next idle tick → REQ on wire |
| `t142_empty_tick_no_recompile` | `nmp-testing/tests/` | Idle tick with no triggers → no compile (use mock planner with compile counter) |

### RED phase verification

Before implementation: run all 6 unit tests + 3 integration tests. They must ALL fail (or not compile). Only then proceed to GREEN phase.

---

## 5. Risk register

| Risk | Severity | Mitigation |
|------|----------|-----------|
| M1 and M2 open overlapping sub-ids for the same content | HIGH | M2 interests are keyed by content-addressed plan-id; M1 uses `seed-timeline-<hash>` prefix. Ensure no sub-id collision in the planner. Add a sub-id prefix assertion test. |
| `drain_tick()` called on every idle tick when registry has entries, even if no triggers | MED | `drain_tick()` already guards on inbox emptiness — zero-cost no-op. No risk to hot path (D8). |
| `KernelMailboxes` adapter borrow lifetime conflicts with `kernel.lifecycle_mut()` | MED | The adapter borrows `kernel.author_relay_lists` immutably; `lifecycle_mut()` borrows `kernel.lifecycle` mutably. These are different fields on `KernelState` — Rust's borrow checker allows simultaneous borrows of separate fields. Validate at compile time, not runtime. |
| WireFrame role-lookup misroutes frames to wrong relay lane | MED | Covered by `wire_frames_to_outbound_role_lookup` unit test. Fallback to `RelayRole::Content` is safe (content lane accepts generic fetches). |
| T141 rebase conflicts block T142 | MED | T141 does not touch `actor/mod.rs` (T142's primary landing zone). Risk is LOW for this specific file. Monitor `kernel/ingest/mod.rs` for any concurrent edits. |
| `drain_tick()` + `plan_diff()` emitted-plan state drifts from `wire_subs` | LOW (today), MED (post-M1-removal) | Audit the `wire_subs` ↔ lifecycle emitted-plan consistency invariant before M1 workarounds are removed (T140 Step C). |
| Actor command `OpenTimeline` continues calling `open_author` after T142 lands | LOW | T142 only wires the drain tick. `open_timeline()` redirect is T140 work. Both must land together. Enforce via integration test that verifies `open_timeline` command produces follow-feed frames, not just profile frames. |

---

## 6. Merge order

```
T141  — substrate-types extract + trait seam (origin/t141-rescue)  → master
  ↓
T142  — wire drain_tick() into actor idle loop (empty registry = no-op)
  ↓
T140  — redirect open_timeline(), register interests, remove M1 workarounds
```

T142 MUST land before T140. T142 with an empty interest registry is a safe no-op (zero compiled interests → zero wire frames → actor behavior unchanged). This phasing allows T142 to be verified in isolation before T140's behavioral change.

---

## 7. Verification checklist (pre-merge)

- [ ] `drain_tick()` call present in `actor/mod.rs` idle block
- [ ] `KernelMailboxes` adapter compiles and implements `MailboxCache`
- [ ] `wire_frames_to_outbound()` conversion function exists with role lookup
- [ ] All 6 unit tests pass (RED verified before implementation)
- [ ] All 3 integration tests pass
- [ ] `cargo test --workspace` green
- [ ] Existing `empty_tick_does_not_compile` test still passes (regression guard)
- [ ] `cargo test --features lmdb-backend` green (T141 gate)
- [ ] Doctrine lint: no new D6 violations introduced
- [ ] File size: no file exceeds 500 LOC hard cap
