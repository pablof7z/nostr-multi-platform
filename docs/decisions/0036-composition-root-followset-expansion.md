# ADR-0036 - Composition-root expansion of the follow-set timeline

Status: accepted

Date: 2026-05-28

## Context

The OP-centric home feed (BACKLOG V-80; full design in
[`docs/perf/op-centric-feed-architecture.md`](../perf/op-centric-feed-architecture.md))
needs two things from the active account's follow set:

1. **A membership predicate** the generic `RootIndexedFeed` engine
   ([ADR-0035](0035-generic-root-indexed-feed-engine.md), `nmp-feed`) consults
   to decide whether an incoming reply qualifies for attribution: "does the
   active account follow this reply's author?"
2. **A timeline-interest expansion**: the planner must REQ kind:1 / kind:6
   events for each followed author so those events actually arrive.

Two earlier shapes were considered and rejected:

- **v3: a `FollowSetLookup` trait in `nmp-feed`.** The engine would name a
  trait, and the producer would implement it. Codex flagged (finding B1/B4)
  that consuming the follow set *inside the planner* forces the planner to name
  the same trait, creating a `nmp-feed → nmp-core → nmp-planner` dependency
  cycle. The trait also adds a crate the producer must depend on.
- **A `LogicalInterest::SocialTimeline` planner variant.** The planner would
  carry a follow-set-aware interest variant and expand it internally against a
  follow-set capability. This bakes a social concept (`SocialTimeline`) into
  the substrate planner (D0 risk), forces the planner to consume a follow-set
  capability, and — per the user's "right not smallest" rule — churns
  `LogicalInterest` across 50+ call sites for a variant that exists only to
  re-derive what the kernel's `sync_follow_feed_interests` already derives.

The design's §3-D resolves the tension in codex's direction: **delete both.**

## Decision

The follow set is **produced** in `nmp-nip02`, consumed as a **closure** by the
engine, and **expanded into concrete planner interests at the composition
root** (`nmp-app-template`). There is no `FollowSetLookup` trait and no
`LogicalInterest::SocialTimeline` variant anywhere in the system.

### The producer — `nmp_nip02::ActiveFollowSet` (this rung, rung 4)

`ActiveFollowSet` (`crates/nmp-nip02/src/active_follow_set.rs`) owns an
`Arc<RwLock<BTreeSet<String>>>` of the active account's follow pubkeys (raw
hex) plus the active account's own pubkey (self-inclusion — see below). It
keeps the set current by:

- observing kind:3 ingest as a `KernelEventObserver` (author-gated to the
  active account); and
- exposing `notify_account_changed()`, the explicit account-switch / logout
  seam the composition root drives.

Its public API is **closures only**:

```rust
impl ActiveFollowSet {
    pub fn new(active_pubkey: ActiveAccountSlot) -> Arc<Self>;
    pub fn follows(&self) -> Vec<String>;
    pub fn predicate(&self) -> Arc<dyn Fn(&str) -> bool + Send + Sync>;
    pub fn on_change(&self, callback: Box<dyn Fn() + Send + Sync>);
}
```

`predicate()` captures a clone of the internal `Arc<RwLock<…>>`, so a predicate
handed to the engine *before* a kind:3 update (or an account switch) reflects
that update **live**. This is the load-bearing property of the closure-only
design: the engine asks, the producer mutates, the engine's view stays current
with zero re-wiring.

### Why the constructor takes `ActiveAccountSlot`, not `&NmpApp`

The design doc sketches `ActiveFollowSet::new(app: &NmpApp)`. That is
pseudocode. `NmpApp` lives in `nmp-ffi`, which `nmp-nip02` depends on only as a
*dev*-dependency. A production `&NmpApp` parameter would add a
`nmp-nip02 → nmp-ffi` edge — a stealth dependency-graph inversion. The
substrate-clean realization mirrors the sibling `FollowListProjection`: take
the `ActiveAccountSlot` (`Arc<Mutex<Option<String>>>`, re-exported through
`nmp_core::slots`) directly. The composition root registers the struct as a
`KernelEventObserver` separately, exactly as it already does for
`FollowListProjection`. Net effect: **no new crate edge in either direction.**
`cargo tree -p nmp-nip02` still carries only `nmp-core`, `nostr`, `serde`,
`serde_json`.

### Self-inclusion

`crates/nmp-core/src/kernel/ingest/contacts.rs::sync_follow_feed_interests`
seeds the active account's own pubkey into `timeline_authors` (lines 162-164:
`authors.insert(me.clone())`) so the user's own notes appear in their home
stream. `ActiveFollowSet` mirrors that inclusion: the active account's own
pubkey is always a member (even before any kind:3 arrives), so the producer
agrees with the kernel's own follow-derived authorship set.

### Account switch / logout is an explicit seam, not an implicit observer

`ActiveAccountSlot` is plain shared state — `Arc<Mutex<Option<String>>>` — with
**no** push notification (no condvar, no channel), and neither `AppHost` nor
`NmpApp` exposes an observer for it. A kind:3 ingest cannot cover logout (there
is no logout-triggered kind:3). So account change is the explicit
`notify_account_changed()` seam: the composition root (rung 6) calls it from
the same identity-change path every other subsystem already uses. It re-reads
the slot, rebuilds for the new active account (clearing the set entirely on
logout), and fires `on_change`.

### The expansion — at the composition root (rung 6, `nmp-app-template`)

`nmp-app-template::register_op_feed_defaults` will:

1. construct the `ActiveFollowSet`;
2. expand `follow_set.follows()` into one concrete `LogicalInterest` per
   followed author (kinds host-declared, `Tailing`) and push them to the
   planner — reusing `planner::LogicalInterest` verbatim, **no enum
   conversion, no new variant**;
3. register an `on_change` callback that re-runs the expansion on every
   follow-set change (mirroring the kernel's existing
   `sync_follow_feed_interests` semantics, just driven from the composition
   root); and
4. wire `follow_set.predicate()` into `nmp_nip01::register_op_feed`.

Follow → interest expansion therefore happens **at the composition root**, not
in the planner and not in the kernel.

## Consequences

- **No dependency cycle.** The planner never consumes a follow-set capability;
  `nmp-feed` never names a follow-set trait; `nmp-nip02` gains no `nmp-feed`
  edge. The graph is strictly simpler than v3.
- **D0 clean.** No `SocialTimeline` social noun in the substrate planner; no
  NIP-02 token leaks into `nmp-core`. `nmp-nip02` is a NIP crate, so the
  NIP-02 follow-set concept lives there legitimately.
- **D7 honored.** The capability is a closure the wiring decides; the engine
  asks, it does not own a producer.
- **V-45 is closed by a different mechanism than originally named.** The
  original V-45 issue proposed the `LogicalInterest::SocialTimeline` substrate
  seam. This ADR delivers the same affordance — every composing app gets the
  follow-set-driven home feed through a one-line
  `register_op_feed_defaults(app, viewer)` — without the planner-side variant.
  The BACKLOG V-45 entry should record that V-45 is satisfied via
  composition-root expansion (rung 6), not via `SocialTimeline`.
- **The user's Q2 (LogicalInterest enum-vs-discriminator) is moot.** There is
  no `SocialTimeline` variant to convert.
- **Post-v1 mute-list (V-60) composes cleanly.** When NIP-51 mute lists land,
  `ActiveFollowSet::predicate()` AND-clauses with `!is_muted(pubkey)` at the
  adapter layer — no engine or planner change.

## Status of this rung

Rung 4 of the 7-rung V-80 ladder. Delivers the **producer only**, unwired, with
12 synthetic tests in `crates/nmp-nip02/src/active_follow_set/tests.rs`. No
consumer (rungs 5–6 consume it). Chirp unchanged; master green.

## Spec drift recorded during implementation

- The design doc cites `Kernel::active_account_handle()` at
  `crates/nmp-core/src/kernel/mod.rs:1265-1267`. On current master it is at
  `mod.rs:1334-1335` (the repo moved). It returns an `ActiveAccountSlot`
  (`Arc<Mutex<Option<String>>>`, a *pull*-shared slot), **not** a push
  observable. `AppHost` does not expose it — only `Kernel` does (and
  `nmp_core::slots` re-exports the type alias). Rung 6 will decide whether to
  grow `AppHost` with an account-slot accessor or thread the slot in at
  composition; that is not rung 4's call.
- The host-declared-kinds change (`fix(nmp-core): keep follow-feed kinds
  host-declared`, commit `2f06cc66`) touches only the follow-feed REQ kinds,
  not the kind:3 ingest fan-out `ActiveFollowSet` observes. The sibling
  `FollowListProjection` (untouched by that commit) is the living proof the
  kind:3 observer pattern still works.
