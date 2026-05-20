# ADR-0022 — NMP owns its relay transport; it does not use `nostr-sdk`'s relay pool

**Date:** 2026-05-20
**Status:** Accepted
**Doctrines invoked:** D4 (single writer per fact; caches derive), D8
(reactivity contract — idle-tick emit gated on `changed_since_emit`),
"No polling — ever" (`AGENTS.md` §"No polling — ever"), RMP bible actor
pattern (`docs/aim.md` §2 — one OS thread owns all mutable state).
**Supersedes the claim in:** `docs/aim.md` §3 / §8 (see "Decision" below).

## Context

`docs/aim.md` describes the protocol foundation as a set of `rust-nostr`
crates the framework "wraps and orchestrates." Two passages name a relay
pool we do not use:

- **§3** lists "A **client/SDK crate** providing `Client`, relay pool
  management, subscription routing, async streaming over tokio."
- **§8** (the `rust-nostr` dependency manifest under References) stated:
  "We depend on its `nostr`, **`nostr-sdk`**, `nostr-database`, …" —
  `nostr-sdk` listed inline as a consumed dependency.

The code says otherwise. `nostr-sdk` does not appear anywhere in the
workspace:

```
$ grep -rn "nostr-sdk\|nostr_sdk" Cargo.toml crates/*/Cargo.toml
(no matches)
```

`crates/nmp-core/Cargo.toml` depends on `nostr` for protocol types only
(`Event`, `Filter`, `Keys`, NIP-44, bech32) and on **`tungstenite`** —
a raw, synchronous, blocking WebSocket library — for the wire:

```toml
nostr = { workspace = true, features = ["nip44"] }
tungstenite = { workspace = true, default-features = false, features = [
    "handshake", "rustls-tls-webpki-roots" ] }
```

The relay transport itself is hand-written and lives entirely inside
`nmp-core`: `crates/nmp-core/src/relay_worker/` (`mod.rs` 452 LOC +
`tests.rs` 627 LOC = ~1079 LOC), driven by the actor in
`crates/nmp-core/src/actor/` and fed by the subscription planner in
`crates/nmp-core/src/planner/`.

This is the textbook "documentation says one thing, code does another"
defect. The discrepancy is architecturally invisible: a new reader of
`aim.md` would design against a dependency that is not there, and would
not discover the custom transport until they grepped the manifest. The
arch-review queue flagged it. This ADR records the decision the code
already embodies, and `aim.md` is corrected in the same change.

The transport reimplementation was **not** an oversight or a fork-drift
accident. The custom worker is purpose-built around four structural
constraints that `nostr-sdk`'s `RelayPool` cannot satisfy without
defeating its own design.

## Decision

**NMP maintains its own relay transport layer. It depends on the `nostr`
crate for protocol types and crypto, and on `tungstenite` for the raw
WebSocket. It does not depend on `nostr-sdk`, and does not use
`nostr-sdk`'s `RelayPool`, subscription router, or async streaming.**

`docs/aim.md` is corrected to match: §3 notes that NMP does not consume
the SDK's relay pool, and §8's `rust-nostr` dependency list drops
`nostr-sdk` and points here.

The transport contract is:

- `relay_worker/mod.rs` — one `tungstenite` socket per relay URL, each
  on its own OS thread, communicating with the actor over
  `std::sync::mpsc` channels (`RelayEvent` inbound, `RelayCommand`
  outbound). No tokio, no `async`/`await`, no executor.
- `actor/mod.rs` — the single synchronous owner thread. It blocks on
  `relay_rx.recv_timeout(compute_wait(…))` and is the only writer of
  kernel state (D4).
- `planner/` — interest lattice + compiler that coalesces logical
  interests into the minimal set of wire REQs.

## Rationale

Each constraint below is satisfied by the custom worker and is in
tension with `nostr-sdk`'s `RelayPool`.

### 1. The RMP actor pattern is a synchronous OS thread; `nostr-sdk` is tokio-async

`docs/aim.md` §2 (the RMP bible) makes the execution model
non-negotiable: "A dedicated OS thread owns `AppState` and runs a
synchronous event loop." NMP's actor is exactly that — a `std::thread`
that blocks on `recv_timeout` (`actor/mod.rs:498`,
`actor/tick.rs::compute_wait`). It is not a tokio task.

`nostr-sdk`'s `Client` / `RelayPool` is tokio-native: relays are
`tokio::spawn`ed tasks, subscriptions are async streams, and reconnect
is driven by the tokio reactor. Adopting it would either (a) pull a
tokio runtime into the kernel and split state ownership across async
tasks — directly contradicting the bible's "single actor owns all
mutable state, no locks, no concurrent mutation" — or (b) force a
sync↔async bridge (`block_on` / channel hops) on the hot path for no
benefit. NMP stays tokio-free in the kernel **on purpose**: every
relay frame already arrives over an `mpsc` channel the synchronous
actor owns, which is the property the bible asks for, achieved without
an executor.

### 2. "No polling — ever" — `recv_timeout(compute_wait)` with idle-tick gating

`AGENTS.md` §"No polling — ever" forbids `sleep`+check loops at every
layer and prescribes blocking `recv()` / `recv_timeout()`. The actor
honors this: it blocks on `recv_timeout(compute_wait(…))`, where
`compute_wait` (`actor/tick.rs`) returns the exact duration until the
next *scheduled* emit, and the idle-tick emit path is gated on
`kernel.changed_since_emit()` so a wake that produced no state change
emits nothing (D8 regression guard, `doctrine.md` §D8).

The worker side is equally non-polling within its constraints: it uses
a 50 ms socket read timeout purely as the tungstenite blocking-read
quantum (`RELAY_READ_TIMEOUT`), drains its control channel with
`try_recv` *without* a sleep, and `wait_before_reconnect` blocks on
`recv_timeout` against a deadline. `nostr-sdk`'s reconnect/backoff is
tokio-timer-driven; wiring its model into the synchronous actor would
mean either a second runtime or a polling shim — both forbidden.

### 3. Generational relay handles — stale-event rejection without locks

Every `RelayEvent` carries a `generation: u64`
(`relay_worker/mod.rs:21-75`). When the actor (re)spawns a worker for a
URL it stamps a fresh, monotonically increasing generation
(`actor/relay_mgmt.rs:86-95`) and records it in `RelayControl`. On every
inbound event the actor compares generations:

```rust
// crates/nmp-core/src/actor/mod.rs:501-507
let generation = event.generation();
if relay_controls
    .get(&relay_url)
    .is_none_or(|control| control.generation != generation)
{
    // Stale event from a disposed worker — ignore.
}
```

This makes reconnect races structurally harmless: a frame still in
flight from a worker the actor already disposed is silently dropped
because its generation no longer matches the live `RelayControl`. The
property is delivered with zero locks and zero coordination — it is a
plain integer compare on the single owner thread. `nostr-sdk` has no
equivalent generational-handle concept; its relay objects are
reference-counted and shared, so it solves the same race with async
task lifecycle and shared-state synchronization. NMP's approach is a
better fit for the single-writer actor and is load-bearing for D4.

### 4. Subscription coalescing — the interest lattice planner

NMP compiles logical interests into wire REQs through a custom
**interest lattice** (`planner/lattice/`, `planner/compiler/`,
`planner/interest.rs`, `planner/plan.rs`). It coalesces overlapping
interests, partitions filters per relay/lane, and emits the minimal REQ
set — a property the kernel depends on for D8 (bounded false-wakeups,
≤60 Hz/view recompute). `nostr-sdk`'s subscription router does not
implement this lattice. Routing to `nostr-sdk` would mean either losing
the coalescer or running it *above* the SDK and then fighting the SDK's
own routing — strictly worse than owning the whole path.

## Consequences

**Accepted costs:**

- NMP carries ~1079 LOC of WebSocket transport (`relay_worker/`) plus
  the actor/planner glue it integrates with. This is code we own,
  test (`relay_worker/tests.rs`, 627 LOC), and must maintain.
- Transport-layer improvements landing in `nostr-sdk` (e.g. new
  reconnect heuristics, NIP-77 negentropy in the pool, gossip
  integration) do **not** flow in automatically. Each must be
  evaluated and, if wanted, ported by hand into `relay_worker/` or the
  planner.
- We carry our own correctness burden for the wire: keepalive FSM
  (`KeepaliveState`, 30 s idle / 30 s pong), jittered backoff against
  thundering-herd reconnect (`jittered_backoff`), permanent-failure
  detection (HTTP 401/403), and write buffering across reconnects.

**Accepted benefits:**

- The kernel stays **tokio-free**. The execution model is exactly the
  RMP bible's single synchronous actor — no async task graph, no
  `block_on` bridges, no runtime to configure or shut down. Async
  would complicate the single-writer model for no protocol-correctness
  gain (the protocol crate `nostr` already supplies correctness).
- The transport is shaped to the doctrines (D4 generational handles,
  D8 idle-tick gating, no-polling `recv_timeout`) rather than the
  doctrines being bent to fit a general-purpose pool.
- Full visibility and control of the wire: every reconnect, backoff,
  keepalive, and stale-frame decision is local code with local tests.

**Boundary that does NOT change:**

- NMP still depends on the `nostr` crate for all protocol types,
  crypto, NIP-44, and bech32 — the protocol-correctness work is still
  the upstream authors'. This ADR is narrowly about the *transport and
  relay-pool* layer, not about reimplementing the Nostr protocol.
- The database crates (`nostr-database`, the `nmp-nostr-lmdb` fork per
  ADR-0011/0012) and signer crates are unaffected.

## Alternatives considered

- **Adopt `nostr-sdk`'s `RelayPool` and bridge sync↔async.** Rejected:
  pulls tokio into the kernel, splits state ownership across async
  tasks, and contradicts the RMP bible's single-actor invariant.
  Reconnect/backoff would become tokio-timer-driven, requiring either a
  second runtime or a polling shim ("No polling — ever").
- **Fork `nostr-sdk` and strip the async layer.** Rejected: a fork is
  a permanent maintenance liability, and what remains after removing
  the pool, router, and streams is roughly the ~1079 LOC we already
  have — without the doctrine-shaped design (generational handles,
  lattice planner). `aim.md` §3 explicitly states the protocol crates
  are "dependencies, not forks."
- **Status quo with no record.** Rejected: that is the defect this ADR
  closes. The decision must be visible in `aim.md` and here.
