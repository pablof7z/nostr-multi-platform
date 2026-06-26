# Store-First Layering Investigation

**Date:** 2026-06-17  
**Question:** Is store-first (ADR-0045) truly universal and intrinsic, or is it bolted-on
per-call-site? Where is the chokepoint gap, and what is the minimal "do less" fix?

> Status note, 2026-06-26: this investigation predates #2092 M5. Mentions of
> `sync_follow_feed_interests` describe the retired follow-feed implementation;
> active follows now compile through the ReducedSource/dependent-interest path.

---

## 1 — How was the original store-first bug fixed?

### PR #1237 (commit `2c62d2d2`) — E1 chokepoint, but with explicit exclusions

This PR established the two install front-doors that are the *intended* ADR-0045 chokepoints:

```
crates/nmp-core/src/kernel/cache_serve/mod.rs
```

- `push_interest_and_serve` (line 196): wraps `push_if_changed` → on shape-change, enqueues `InvalidateCompile` and calls `enqueue_interest_cache_serve`.
- `ensure_interest_and_serve` (line 224): wraps `ensure_sub` → on newly-installed, enqueues `InvalidateCompile` and calls `enqueue_interest_cache_serve`.
- `open_interest_sub` at `kernel/mod.rs:2494` delegates to `ensure_interest_and_serve`.

The `ActorCommand::PushInterest` arm (`actor/dispatch.rs:1326`) and `ActorCommand::EnsureInterest` arm (`actor/dispatch.rs:1346`) and the `open_uri` resolver (`kernel_action.rs:115`) all route through these two front-doors.

Crucially, PR #1237 also added **explicit ADR-0045 carve-out comments** to `startup.rs`
saying the bootstrap interests "intentionally does NOT route through `enqueue_interest_cache_serve`"
because "serving a possibly-stale store copy would defeat the bootstrap."

```
// startup.rs (state BEFORE PR #1490):
// register_oneshot_discovery_interest: set_sub, NO cache-serve
// register_tailing_self_kinds_interest: set_sub, NO cache-serve
```

The fix for the follow-feed path in the same PR was a **batch variant**:
`sync_follow_feed_interests` (`ingest/contacts.rs:171-173`) iterates follows, calls
`enqueue_interest_cache_serve_deferred` once per author, then calls `run_cache_serve_step()`
once for the whole batch — a deliberate O(1)-drain design for 300–500-follow cold starts.

### PR #1490 (commit `db597edc`, today) — startup.rs bootstrap interests

The carve-out in PR #1237 turned out to be wrong doctrine. The active account's own
kind:3 follow list is the INPUT that drives follow-feed interest registration; if it isn't
served from the store the in-memory ContactsCache stays empty until a relay delivers it.

Fix: in both `register_oneshot_discovery_interest` and `register_tailing_self_kinds_interest`,
after the `set_sub` call, add an explicit `enqueue_interest_cache_serve`:

```rust
// startup.rs:195–199 (register_oneshot_discovery_interest):
self.lifecycle.registry_mut().set_sub(identity, interest);
self.enqueue_interest_cache_serve(&sub_key, &shape);   // ← added in #1490

// startup.rs:250–255 (register_tailing_self_kinds_interest):
self.lifecycle.registry_mut().set_sub(identity, interest);
self.enqueue_interest_cache_serve(&sub_key, &shape);   // ← added in #1490
```

**Verdict:** The fix was **per-call-site** (two explicit `enqueue_interest_cache_serve` calls
added to `startup.rs`). This perpetuates the pattern: the caller is responsible for remembering
to cache-serve after calling `set_sub`.

---

## 2 — Call-graph of every cache-serve enqueue site

### Production code (non-test, non-support)

| File | Line(s) | How | Notes |
|---|---|---|---|
| `kernel/cache_serve/mod.rs` | 205 | `push_interest_and_serve` calls `enqueue_interest_cache_serve` | Front-door for `PushInterest` command; gated on `push_if_changed` returning true |
| `kernel/cache_serve/mod.rs` | 238 | `ensure_interest_and_serve` calls `enqueue_interest_cache_serve` | Front-door for `EnsureInterest`, `OpenInterest`, `open_uri`; gated on `newly_installed` |
| `kernel/ingest/contacts.rs` | 171–173 | `enqueue_interest_cache_serve_deferred` per author + one `run_cache_serve_step` | Batch path for follow-feed sync; bypasses front-doors, uses legacy `push` directly |
| `kernel/requests/startup.rs` | 199 | `enqueue_interest_cache_serve` after `set_sub` | `register_oneshot_discovery_interest` (kind:10050); added in PR #1490 |
| `kernel/requests/startup.rs` | 255 | `enqueue_interest_cache_serve` after `set_sub` | `register_tailing_self_kinds_interest` (kinds 0/3/10002/…); added in PR #1490 |

### Call-graph to the front-doors

```
ActorCommand::PushInterest
    └─ actor/dispatch.rs:1326 → push_interest_and_serve
           └─ registry.push_if_changed → [if changed] → enqueue_interest_cache_serve

ActorCommand::EnsureInterest
    └─ actor/dispatch.rs:1346 → ensure_interest_and_serve
           └─ registry.ensure_sub → [if newly_installed] → enqueue_interest_cache_serve

ActorCommand::OpenInterest
    └─ actor/dispatch.rs:1382 → open_interest_sub (kernel/mod.rs:2494)
           └─ ensure_interest_and_serve
                  └─ [same as above]

open_uri (kernel_action.rs:115)
    └─ ensure_interest_and_serve
           └─ [same as above]

KernelReducer::open_interest (kernel_reducer/feed_verbs.rs:35)
    └─ open_interest_sub
           └─ ensure_interest_and_serve
                  └─ [same as above]

sync_follow_feed_interests (kernel/ingest/contacts.rs:84)
    └─ registry.push (legacy) — bypasses front-doors
    └─ enqueue_interest_cache_serve_deferred × N authors
    └─ run_cache_serve_step() [once for whole batch]

register_oneshot_discovery_interest (startup.rs:172)
    └─ registry.set_sub — bypasses front-doors
    └─ enqueue_interest_cache_serve ← explicit call added in PR #1490

register_tailing_self_kinds_interest (startup.rs:216)
    └─ registry.set_sub — bypasses front-doors
    └─ enqueue_interest_cache_serve ← explicit call added in PR #1490

register_profile_claim_interest (profile.rs:222)  ← BUG
    └─ registry.set_sub — bypasses front-doors
    └─ [NO cache-serve call]  ← MISSING
```

### Absence confirmed

`register_profile_claim_interest` (`kernel/requests/profile.rs:274`) calls
`self.lifecycle.registry_mut().set_sub(identity, interest)` and then enqueues only a
`CompileTrigger::ViewOpened` (lines 277–279). There is no `enqueue_interest_cache_serve` call
anywhere in profile.rs. This is the bug: a cold-start `claim_profile` for a pubkey whose
kind:0 is already in the local store will never be served from the store.

---

## 3 — The single chokepoint: is there one, and why isn't cache-serve intrinsic to it?

### The candidate chokepoints

**Chokepoint A: the registry mutation methods.**

`InterestRegistry` (`subs/registry.rs`) has three mutation entry points:
- `ensure_sub` (line 68): returns `bool` (newly-installed). Callers can act on the return value.
- `set_sub` (line 86): always upserts; returns `()`. No "is-new-or-changed" signal.
- `push_if_changed` (line 149): returns `bool` (absent-or-changed); used only by `push_interest_and_serve`.

The registry is a data structure inside `SubscriptionLifecycle`; it has no access to the
Kernel's `pending_cache_serves` queue, `served_interest_shapes` set, or store. It cannot
call `enqueue_cache_serve` directly. To hook cache-serve here would require the registry to
either (a) hold a mutable reference to the Kernel (circular), (b) return a change descriptor
that the Kernel caller acts on, or (c) take a callback. Option (b) is what the two existing
front-doors already do implicitly — they check the return value of `ensure_sub`/`push_if_changed`
and conditionally call `enqueue_interest_cache_serve`.

**Chokepoint B: the recompile/diff path.**

`SubscriptionLifecycle::recompile_and_diff` (`subs/recompile.rs`) compares the new `CompiledPlan`
against the current plan and produces a `Vec<WireFrame>` diff. `WireFrame::Req` entries represent
newly-needed subscriptions. One could imagine the Kernel looping over newly-added `Req` frames
and enqueuing a cache-serve for each.

Problems with this:
1. `WireFrame::Req` carries `interest_id`, not the full `InterestShape`. The `sub_key` (needed
   for `completion_key_for_interest`) is not in the wire frame; it would need a reverse lookup
   in the registry.
2. The diff fires on every `drain_lifecycle_tick` — including relay reconnects. The
   completion_key idempotency guard (`served_interest_shapes`) would make re-serves no-ops, but
   the per-recompile overhead of iterating new frames and attempting lookups would be unnecessary
   churn.
3. The diff is AFTER compile; the compile is AFTER registry mutation. Cache-serve is designed
   to fire synchronously at interest-open time (D1: "the first snapshot after install carries
   store data"). Deferring to the recompile pass means the first snapshot might fire before
   store data is served.

**Chokepoint C: Kernel-level install front-doors (current approach).**

The three Kernel methods `push_interest_and_serve`, `ensure_interest_and_serve`, and
`open_interest_sub` ARE the chokepoints — for the `push_if_changed` and `ensure_sub` paths.
The gap is that the `set_sub`-based paths (`startup.rs` and `profile.rs`) bypass them.

### Why is cache-serve currently per-caller?

The registry mutation model has three semantically distinct operations:
- **ensure** (register-if-absent): cache-serve only when truly new.
- **push** (replace-or-insert, shape-gated): cache-serve only when shape changed.
- **set** (unconditional upsert): cache-serve whenever called (idempotency guard handles dedup).

The two existing front-doors already centralize the first two. The third (`set_sub`) was used
by startup.rs and profile.rs **without a corresponding front-door**, so callers must add
explicit cache-serve calls manually. PR #1490 added the two explicit calls to startup.rs but
left profile.rs untouched.

---

## 4 — Profile claims: the chokepoint gap in detail

### What `register_profile_claim_interest` does (profile.rs:222–279)

1. Derives `key = profile_claim_sub_key(pubkey)` → `SubKey::new(("profile-claim", pubkey))`.
2. Builds a `LogicalInterest` with shape `{authors: {pubkey}, kinds: {0}, limit: None}`.
3. Determines lifecycle: `OneShot` if `CacheOk`, `Tailing` if `Live`. "Tailing wins" logic
   upgrades an existing `CacheOk` slot to `Tailing` via `set_sub`.
4. Calls `self.lifecycle.registry_mut().set_sub(identity, interest)` (line 274).
5. Calls `self.lifecycle.enqueue_trigger(CompileTrigger::ViewOpened { .. })` (lines 277–279).
6. **Does NOT call `enqueue_interest_cache_serve`.** — the bug.

### Why `set_sub` not `ensure_sub`?

The liveness upgrade semantics require it: a second `claim_profile` for the same pubkey with
`liveness = Live` must upgrade the slot's lifecycle from `OneShot` to `Tailing`. `ensure_sub`
is idempotent register-if-absent — it would leave the `OneShot` lifecycle untouched on the
second call. Only `set_sub` (unconditional replace) achieves the upgrade.

### Would centralizing fix it for free?

If a `set_interest_and_serve` Kernel method existed (wrapping `set_sub` + trigger + cache-serve),
`register_profile_claim_interest` would call it instead of bare `set_sub` + trigger. The
completion_key is derived from `(sub_key, shape)` — see `queries.rs:186–207`. The shape for a
profile interest is `{authors: [pubkey], kinds: [0], limit: None}` regardless of liveness level
(lifecycle is NOT part of the completion key). So:

- First `CacheOk` claim: new slot → `set_interest_and_serve` queues a cache-serve → store data
  served.
- Second `Live` claim on same pubkey: `set_sub` replaces lifecycle but the shape is unchanged →
  same completion_key → `enqueue_cache_serve` no-ops (already in `served_interest_shapes`) →
  no redundant re-serve. Safe.
- `claim_profile_inner` already guards `want_register = !resident || liveness == Live`
  (profile.rs:200), so for warm-resident `CacheOk` claims `register_profile_claim_interest` is
  not called at all — the guard limits the surface area.

**Yes, centralizing would fix the profile bug for free.**

### `set_sub` vs `ensure_sub` vs `push_if_changed` — which does the profile path use?

The profile path uses `set_sub` (registry.rs:86): attaches the owner AND replaces the interest
unconditionally. The existing front-doors use `ensure_sub` (register-if-absent) and
`push_if_changed` (replace-if-changed). There is no Kernel-level `set_interest_and_serve`
equivalent for the unconditional-upsert case.

---

## 5 — The "do less" design

### What "do less" means here

Currently there are FIVE distinct install patterns, each remembering to call cache-serve in a
slightly different way:

| Pattern | Registry call | Cache-serve call | Status |
|---|---|---|---|
| `push_interest_and_serve` | `push_if_changed` | `enqueue_interest_cache_serve` if changed | ✓ centralized |
| `ensure_interest_and_serve` | `ensure_sub` | `enqueue_interest_cache_serve` if newly installed | ✓ centralized |
| follow-feed batch | `push` (legacy) | `_deferred` × N + `run_cache_serve_step` | legitimate exception (see below) |
| startup.rs bootstrap | `set_sub` | explicit `enqueue_interest_cache_serve` (added in #1490) | ✗ per-call-site |
| profile claims | `set_sub` | **NOTHING** | ✗ BUG |

The "do less" move is: introduce a third Kernel-level front-door for the `set_sub` path —
`set_interest_and_serve` — and route startup.rs and profile.rs through it.

### What to add (one place)

In `crates/nmp-core/src/kernel/cache_serve/mod.rs`, add alongside `push_interest_and_serve`
and `ensure_interest_and_serve`:

```rust
/// Upsert install recipe — register/replace via `set_sub`, enqueue a recompile
/// trigger, and enqueue a store-cache serve.
///
/// For interest slots that use `set_sub` semantics (unconditional upsert, e.g.
/// account-switch replacement, liveness upgrade). The `set_sub` always fires the
/// cache-serve; the completion-key idempotency guard inside
/// [`Kernel::enqueue_interest_cache_serve`] no-ops if the shape has not changed
/// since the last completed serve.
///
/// Replaces the per-call-site pattern of `set_sub` + manual trigger + manual
/// `enqueue_interest_cache_serve` used in `startup.rs` and (currently missing
/// from) `profile.rs`.
pub(crate) fn set_interest_and_serve(
    &mut self,
    identity: crate::subs::SubIdentity,
    interest: crate::planner::LogicalInterest,
    reason: &'static str,
) {
    let serve_key = identity.key;
    let serve_shape = interest.shape.clone();
    self.lifecycle.registry_mut().set_sub(identity, interest);
    self.lifecycle
        .enqueue_trigger(crate::subs::CompileTrigger::InvalidateCompile {
            reason: crate::subs::InvalidateReason::External(reason.to_string()),
        });
    self.enqueue_interest_cache_serve(&serve_key, &serve_shape);
}
```

### What to delete (scattered calls)

Once `set_interest_and_serve` exists:

1. **`startup.rs:195–199`** (`register_oneshot_discovery_interest`):
   - Replace `self.lifecycle.registry_mut().set_sub(identity, interest)` + explicit
     `self.enqueue_interest_cache_serve(&sub_key, &shape)` with
     `self.set_interest_and_serve(identity, interest, "bootstrap-oneshot-discovery")`.
   - The `CompileTrigger::ViewOpened` at the call-site in `active_account_bootstrap_requests`
     (startup.rs:129) can remain as a coalesced trigger for the full bootstrap batch.

2. **`startup.rs:250–255`** (`register_tailing_self_kinds_interest`):
   - Same pattern: replace `set_sub` + explicit `enqueue_interest_cache_serve` with
     `set_interest_and_serve(identity, interest, "bootstrap-self-kinds")`.

3. **`profile.rs:271–279`** (`register_profile_claim_interest`):
   - Replace `self.lifecycle.registry_mut().set_sub(identity, interest)` +
     `self.lifecycle.enqueue_trigger(CompileTrigger::ViewOpened { .. })` with
     `self.set_interest_and_serve(identity, interest, "profile-claim")`.
   - The `ViewOpened` trigger is absorbed into `set_interest_and_serve`'s
     `InvalidateCompile` — these are semantically equivalent at the planner level
     (both unconditionally recompile; `InvalidateCompile` is strictly more expressive).

Net change: -2 explicit `enqueue_interest_cache_serve` calls (startup.rs), -3 manual
registry/trigger lines in profile.rs (set_sub + enqueue_trigger), +1 method added, profile
cache-serve bug fixed for free.

### The follow-feed batch exception

`sync_follow_feed_interests` (`ingest/contacts.rs:84`) legitimately differs: it registers
one interest per followed pubkey (up to ~500) using the legacy `push` path (which triggers
shape-change detection via `push_if_changed` internally), enqueues `_deferred` serves for
all of them, then drains ONCE. This is the ADR §5 anti-burst design — N serves under ONE
synchronous chunk rather than N separate `run_cache_serve_step` calls.

Routing this through `set_interest_and_serve` per author would be wrong: each call would
trigger a synchronous `run_cache_serve_step` drain. It must keep the deferred-batch pattern.
This is a legitimate structural reason it cannot be collapsed into the single-interest
front-doors. **Do not attempt to centralize the follow-feed batch path.**

### Safety analysis of centralization

| Concern | Assessment |
|---|---|
| **Timing (D1 — first snapshot carries store data)**: `set_interest_and_serve` drains synchronously via `enqueue_interest_cache_serve` (which calls `run_cache_serve_step`). Same timing as `ensure_interest_and_serve`. | Safe. |
| **Dedup — same shape called twice**: completion_key is `stable_hash64((sub_key, authors, kinds, tags, addresses))`. Same pubkey + kind:0 shape = same key = `served_interest_shapes` no-op on second call. | Safe. |
| **Liveness upgrade (CacheOk → Live)**: shape is identical (lifecycle is NOT part of completion_key). The slot is replaced by `set_sub` but the serve key/shape are the same → completion_key is the same → already-served is a no-op. If the prior serve was not yet finished (still queued), the dedup check in `enqueue_cache_serve` (`pending_cache_serves.iter().any(...)`) blocks re-enqueueing. | Safe. |
| **Account-switch (`startup.rs` uses `set_sub` to replace prior account's author)**: on switch, shape changes (new author). New completion_key. `clear_served_interest_shapes` is called on account-switch (kernel reset), so the old key is gone from `served_interest_shapes`. The new serve fires. | Safe. |
| **Batch risk (should `set_interest_and_serve` suppress the synchronous drain for startup.rs)?**: startup.rs registers only 2 interests (oneshot kind:10050 + tailing self-kinds). The per-tick budget is `2 × visible_limit` (default 160) visits. 2 interests × `min(shape.limit, 80)` depth is well within budget. No burst risk. | Safe. |
| **`is_indexer_discovery` flag**: only affects wire routing (planner's `bootstrap_indexer_relays` fallback). Does not affect store-serve. The completion_key derivation ignores it. | No interaction. |

### What this does NOT solve

`sync_follow_feed_interests` still uses the manually-coded batch deferred pattern. This is
correct and intentional. The "do less" design leaves it untouched.

The wider issue — why callers must know to call a front-door at all — could only be resolved
by making the registry return change-descriptors and having a single Kernel wrapper that always
calls cache-serve. That would require changing the registry's return types and restructuring
all callers. The three-front-door model (`push_interest_and_serve`,
`ensure_interest_and_serve`, `set_interest_and_serve`) is the practical minimally-intrusive
version of that design: it keeps each registry operation paired with its cache-serve in one
place without touching the registry's API surface or the batch path.

---

## Summary

| Question | Answer |
|---|---|
| Was PR #1490's fix intrinsic or per-call-site? | Per-call-site: two explicit `enqueue_interest_cache_serve` calls added after `set_sub` in `startup.rs`. |
| Is there a single chokepoint today? | For `ensure_sub`/`push_if_changed` paths: yes — `ensure_interest_and_serve` / `push_interest_and_serve`. For `set_sub` paths: no — there is no `set_interest_and_serve` front-door. |
| What is the profile claim bug? | `register_profile_claim_interest` (`profile.rs:274`) calls `set_sub` + trigger but not `enqueue_interest_cache_serve`. |
| Minimum "do less" fix? | Add `set_interest_and_serve` in `cache_serve/mod.rs`. Route `register_profile_claim_interest`, `register_oneshot_discovery_interest`, and `register_tailing_self_kinds_interest` through it. Delete 2 explicit `enqueue_interest_cache_serve` calls and 3 manual lines in profile.rs. |
| Is centralization safe? | Yes for the single-interest `set_sub` paths. The follow-feed batch path is a legitimate exception and must not be touched. |
