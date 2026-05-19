# T100 — closeout & residuals

**Status as of HB48 (2026-05-18):** Part 1 CLOSED-by-T128; Part 2 OPEN with executable
spec landed, fix proposal documented, implementation deferred.

T100 originally bundled two iOS Pulse items that were held behind T105 + T128. T128
has since landed (`1486eed`); this note records the audit outcome.

---

## Part 1 — Per-relay OK correlation (iOS publish UX) — **CLOSED-by-T128**

**Verdict:** No further work in T100. T128 fully absorbed this scope.

**Evidence (read inline at `ios/NmpPulse/NmpPulse/Views/ComposeView.swift`):**

- `PublishQueueEntry.relayOutcomes: [RelayAckOutcome]` decoded from the kernel
  snapshot (KernelBridge, T128).
- `publishStatus` body switches on terminal status (`"ok"` / `"failed"` /
  in-flight) — see lines 103–121.
- Partial-success rendering: `publishStatusOk` (lines 123–144) computes
  `accepted < target` and renders **"Published to N of M relays"** with an
  orange checkmark, vs **"Published to M relays"** (green) on full success.
- Failure rendering: `publishStatusFailed` (lines 146–176) surfaces the
  first failure reason from the per-relay outcomes map and exposes a Retry
  button that re-publishes the same draft.
- Auto-dismiss waits for terminal `"ok"` (not the pre-T128 `accepted_locally`
  race), so failed publishes keep the sheet open for retry.

**Closure bar agreed in T100 spec:** "If it already shows per-relay outcomes
(partial success 'N of M relays'), then Part 1 is CLOSED." That bar is met by
the partial-success rendering above. A stricter per-relay-row breakdown (URL
+ OK/error/retry per relay) is **out of scope for T100** — if a future task
wants the granular view, it should be filed as a fresh Pulse ticket against
DiagnosticsView.

**No file changes required from T100 for Part 1.**

---

## Part 2 — kind:3 follows timeline fan-out — **OPEN (residual, fix proposed)**

**Verdict:** Empirically RED. The kernel does NOT re-fan-out the timeline when
the active account's kind:3 follow list expands to include authors whose
NIP-65 write relays differ from the existing subscription set.

### Empirical finding

Executable spec landed at
`crates/nmp-core/src/kernel/contacts_fanout_tests.rs::kind3_arrival_fans_out_timeline_onto_new_follows_write_relays`,
marked `#[ignore]` so the workspace baseline stays GREEN (1091/0/18 — was
1091/0/17 pre-T100; +1 ignored). Run it explicitly with:

```
cargo test -p nmp-core kind3_arrival_fans_out_timeline -- --include-ignored
```

Captured failure trace:

```
NMP_CORE opening seed timeline: 4 authors fanned out over 3 relay(s)
NMP_CORE REQ seed-timeline-…@content (wss://alice.write/): seed union timeline kinds:1,6 (NIP-65 outbox)
NMP_CORE REQ seed-timeline-…@content (wss://nos.lol): seed union timeline kinds:1,6 (NIP-65 outbox)
NMP_CORE REQ seed-timeline-…@content (wss://relay.damus.io): seed union timeline kinds:1,6 (NIP-65 outbox)
NMP_CORE contacts aaaa…aaaa -> 3 followees

panicked: T100/P2: post-kind:3 emission must route to BOB's resolved write
relay (he was just added to the follow set); got urls = {}
```

The first `maybe_open_timeline()` correctly fans out across ALICE's resolved
relay + the cold-start bootstrap seeds (for the built-in `seed_accounts`
cohort). The kind:3 ingest then records the new follows (`contacts aaaa…
-> 3 followees`), but the **second** `maybe_open_timeline()` returns an
empty set of timeline REQs because:

- `ingest_contacts` (`crates/nmp-core/src/kernel/ingest/contacts.rs`) calls
  `lifecycle.enqueue_trigger(CompileTrigger::FollowListChanged { … })` and
  updates `self.seed_contacts`, but
- the comment on lines 21–25 of that file is explicit: *"the compile /
  registry machinery is dormant until M11 migrates view modules onto
  `LogicalInterest`"*, and
- `maybe_open_timeline` (`crates/nmp-core/src/kernel/ingest/timeline.rs`
  line 225) short-circuits on `!self.timeline_requested && …`; nothing
  flips `timeline_requested` back to `false` on kind:3 arrival.

This is also acknowledged in
`crates/nmp-core/src/actor/commands/identity.rs:146` — the comment on
`retarget_timeline` reads *"kind:3 follow fan-out is a documented
follow-up."* T100/P2 is that follow-up.

### Fix proposal (NOT implemented in T100 — scope guard)

**Mirror the kind:10002 direct-flip pattern.**
`ingest_relay_list` (`crates/nmp-core/src/kernel/ingest/relay_list.rs`
lines 71–86) already handles the analogous problem when an
already-timeline author's NIP-65 list arrives:

1. flip `timeline_requested = false` so the next `maybe_open_timeline()`
   re-plans, and
2. enqueue CLOSE frames for the prior `seed-timeline-*` subs via
   `close_subscriptions_with_prefixes` + `defer_outbound`, so the next
   emission CLOSEs the stale subs alongside the new resolved-relay REQs.

For `ingest_contacts`, the same shape applies, gated on **the active
account's** kind:3 only (so a random peer's kind:3 echo doesn't churn our
timeline):

```rust
// Inside ingest_contacts, after `self.seed_contacts.insert(...)`:
let is_active_account = self.active_account.as_deref() == Some(&event.pubkey);
let follows_changed = previous_follows != follows;   // diff against prior cache
if is_active_account && follows_changed && self.timeline_requested {
    self.timeline_requested = false;
    let closes = self.close_subscriptions_with_prefixes(&["seed-timeline-"]);
    for close in closes {
        self.defer_outbound(close);
    }
}
```

(Capture `previous_follows` from `self.seed_contacts.get(&event.pubkey).cloned()`
before the `insert`.)

**Why not wire the dormant `CompileTrigger::FollowListChanged`?**
The lifecycle trigger lands in `lifecycle.inbox` but nothing drains it back
into the kind-3 fan-out path today. The compile/registry machinery is M11
surface area — implementing it from inside T100 would be scope creep. The
direct-flip pattern is the production seam used by kind:10002 since T105;
T100/P2 should adopt the same pattern, and M11 can later reroute both
ingest paths through the compiler when ViewModules land on `LogicalInterest`.

**Why active-account gate?**
The kernel's `seed_contacts` is keyed by `event.pubkey`, so it stores
follow lists for *all* observed kind:3 events, not just the active
account's. Fan-out should only fire on the active account's follow list —
echo-back of a self-publish via the engine is the canonical trigger.
Without the gate, a kind:3 from any peer in the user's social graph would
churn the timeline.

**Test coverage:** the `#[ignore]`d test in `contacts_fanout_tests.rs`
becomes the green sentinel once the fix lands. Remove the `#[ignore]`
attribute, run `cargo test -p nmp-core kind3_arrival_fans_out_timeline`,
confirm PASS.

### Pulse-side observer — NOT NEEDED

The original T100 brief asked whether an iOS Pulse-side observer would need
to watch follow-list changes and trigger a UI refresh. **Answer: no.** The
timeline UI already reacts to snapshot changes via the existing kernel
snapshot reactivity; once the kernel-side fix re-fans-out and new events
arrive on the resolved relays, they flow into `TimelineSnapshot.items` and
SwiftUI updates the view automatically. Part 2 is **purely a kernel-side
residual**.

---

## Summary

| Part | Status | Notes |
|------|--------|-------|
| Per-relay OK correlation (iOS) | **CLOSED-by-T128** | ComposeView already renders partial-success per-relay outcomes; no T100 changes needed. |
| kind:3 follows timeline fan-out | **OPEN — fix proposed** | Executable spec landed (`#[ignore]`d sentinel); fix is the kind:10002 direct-flip pattern, gated on active account. |

**Workspace baseline:** 1091 passed / 0 failed / 18 ignored (HB48 was
1091/0/17; +1 sentinel from T100).

**Residual ticket to file:** *T100/P2 — kind:3 follow expansion re-fans
timeline.* One-file kernel change in `ingest/contacts.rs` (≈ 15 LOC) +
remove `#[ignore]` from the existing test. Estimated ≤ 1 hour.
