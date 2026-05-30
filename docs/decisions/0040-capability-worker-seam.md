# ADR-0040 — Capability-worker seam: take remote-signer and capability I/O off the actor thread

- **Status:** Proposed (2026-05-30)
- **Relates to:**
  - **Resolves V-90** (actor thread blocking during remote-signer operations,
    HIGH · D8 violation · GH #612 #613).
  - **Resolves V-54** (NIP-46 onboarding still blocks the actor thread —
    `identity.rs:825,863,1018` cold-start signs; GH #611).
  - **ADR-0024** (async capability protocol) — that ADR proposed a two-phase
    `CapabilityResultReady` re-entry but was never implemented and only ever
    scoped the *HTTP* capability. This ADR supersedes its mechanism for the
    *synchronous-native* (Keychain-class) capability and explains why a single
    serialized worker — not the per-executor saga of ADR-0024 — is the right
    shape for that class.
  - **ADR-0023** (`HttpCapability` over the synchronous capability socket) —
    the ADR that first documented the synchronous capability socket as a
    deliberate, time-boxed MVP. This ADR retires that "blocks the actor"
    sanction for the in-actor call sites.
  - **ADR-0031** (`nmp-signer-broker` owns the NIP-46 relay transport) — the
    canonical precedent for *worker-feeds-actor*: a worker thread reaches back
    through the `bunker_hook` indirection and re-enters the actor via
    `ActorCommand::AddRemoteSigner` / `BunkerHandshakeProgress`, never blocking
    or mutating kernel state from the worker.
  - **ADR-0026** (signer NIP-44 seal seam) — defines `SignerForSeal` and the
    `SignerOp` (`Ready` / `Pending`) shape the gift-wrap path returns.
  - **ADR-0028** (actor-liveness probe FFI) — the liveness probe exists
    precisely because actor stalls are observable; this ADR removes two of the
    stalls it was built to detect.
- **Scope:** the two confirmed-live in-actor blocking sites (NIP-17 DM
  gift-wrap `op.wait`; the synchronous-native capability dispatch) plus the
  V-54 cold-start signs. It ratifies **one new primitive** — the serialized
  capability-worker thread — and confirms that the other two paths reuse
  *existing* precedented mechanisms (`PendingSign`; the nmp-nip57 lnurl worker).
  Out of scope: the HTTP/LNURL capability (already non-blocking via the lnurl
  worker), Android `nativeNextUpdate` polling (V-91), and any change to the
  `bunker_hook` handshake transport.

---

## Context

The kernel runs as a single-writer actor on one OS thread
(`crates/nmp-core/src/actor/mod.rs`). The loop drains `command_rx` via
`try_recv` at the top of every tick and otherwise blocks on
`recv_timeout(compute_wait(…))` against the relay-event channel, so emit-hz
cadence is preserved. **The single-writer invariant (D3/D4) is the load-bearing
property: all mutable kernel state is owned by this thread, and the only legal
way to mutate it is to dequeue an `ActorCommand`.** Any code path that *blocks*
this thread freezes everything — no command is dequeued, no snapshot tick is
emitted, no relay frame is serviced, and the UI freezes (the actor-liveness
probe of ADR-0028 will report the stall).

The codebase already solved this for the common remote-signer publish path. A
remote (NIP-46) sign returns a `SignerOp::Pending(rx)`; instead of blocking, the
actor parks it as a `PendingSign` (`crates/nmp-core/src/actor/pending_sign.rs`)
and `retain_mut`-polls every parked op once per idle tick with a non-blocking
`try_recv` (`actor/mod.rs:1849`). A completed op publishes its signed event and
is removed; a timed-out op (`PENDING_SIGN_TIMEOUT`, 5 s) surfaces a D6 toast and
records a terminal action failure. This is the canonical "actor never blocks"
pattern for signing. **Three paths bypass it and block the actor anyway.**

### Blocking site 1 — NIP-17 DM gift-wrap (`op.wait`) [GH #612, V-90]

`crates/nmp-nip17/src/dm_send.rs:221`, inside
`SendGiftWrappedDmCommand::run` (a `ProtocolCommand` the actor runs on its own
thread):

```rust
let op = nmp_nip59::gift_wrap_with_signer(&signer, receiver_pk, &nostr_rumor, tweaked);
let envelope = match op.wait(nmp_nip59::GIFT_WRAP_TOTAL_TIMEOUT) { … };
```

`gift_wrap_with_signer` (`crates/nmp-nip59/src/signer_seal.rs:234`) is already
half-correct: for a remote signer it spawns a short-lived per-invocation
*driver* thread and returns `SignerOp::Pending(rx)` immediately, so the
multi-step bunker chain (`nip44_encrypt` → `sign_seal` → wrap) runs off-actor.
**But the caller then throws that non-blocking property away** by calling
`op.wait(GIFT_WRAP_TOTAL_TIMEOUT)` — a `recv_timeout` of up to **12 s** — *on
the actor thread*. The DM send is a two-envelope loop (recipient + self-copy),
so a slow bunker can wedge the actor for up to ~24 s. **Confirmed live.**

### Blocking site 2 — synchronous-native capability dispatch [GH #613, V-90]

`crates/nmp-ffi/src/capability.rs:56` (`nmp_app_dispatch_capability`) →
`dispatch_capability(&app.capability_callback, &request)` invokes the registered
platform callback **synchronously**. The in-actor call sites are
`crates/nmp-ffi/src/lib.rs:1547` (`sign_in_local_nsec_with_keyring` →
`self.dispatch_capability(&req)`) and the sibling persist/forget paths reached
from the identity reducer (session persistence on `SwitchActive` / `RemoveAccount`
/ `AddRemoteSigner` calls `session_persistence::*` which dispatch the keyring
capability). The iOS `KeychainCapability.handleJSON(_:)` runs on the calling
thread; a Keychain read/write blocks the actor for **hundreds of ms** — short
of the liveness threshold, but a visible hitch on every account
persist/forget/switch. ADR-0023 explicitly scoped this synchronous socket as an
MVP that "blocks the actor thread — a deliberate, documented" choice; this ADR
retires that sanction for the in-actor sites. **Confirmed live.**

### Blocking site 3 — cold-start onboarding signs [V-54]

`crates/nmp-core/src/actor/commands/identity.rs:825,863,1018` publish the
initial kind:0 metadata, kind:10002 relay list, and kind:3 follows during
`create_account` via the synchronous `sign_active` path
(`REMOTE_SIGN_TIMEOUT`, 5 s). For a bunker account each of the three is a
blocking actor stall during onboarding. This is in V-90's cluster as V-54.

### Why this is one ADR

The 2026-05-29 `open-backlog-resolution` workflow produced a single off-actor
design for the V-54 + V-90 cluster (recorded as the BACKLOG "DESIGN PRODUCED,
ADR-pending" note). It rests on **three precedented primitives, no ad-hoc
copies**. Two of the three reuse mechanisms that already ship; **only the
serialized capability-worker thread is genuinely new and needs ratification** —
hence this ADR.

---

## Forces / constraints

1. **D8 — no polling, ever.** Every off-actor worker MUST advance by *blocking
   recv* or an OS readiness callback, never a `sleep`-and-check loop
   (`docs/wiki/d8-no-polling-ever.md`). The actor's own re-entry poll is the
   already-sanctioned `retain_mut` + `try_recv` once per tick (not a busy
   loop): it runs only when there is at least one parked op and otherwise
   degrades to a zero-cost empty `Vec` scan.
2. **D3/D4 — single-writer invariant.** A worker thread MUST NOT touch kernel
   state. Its only legal output is to send an `ActorCommand` back through the
   actor's `command_tx`; the result is applied *inside a normal actor tick*.
   This is exactly how the lnurl worker (`nmp-nip57`) and the signer broker
   (ADR-0031) re-enter today.
3. **Account-switch correctness (the hard subtlety).** A capability operation
   carries an `account_id` (`KeyringRequest::{Store,Retrieve,Delete}`). Between
   the moment a capability op is enqueued and the moment its result re-enters
   the actor, the user may `SwitchActive` / `RemoveAccount`. The seam MUST NOT
   apply a stale result to the wrong account, and persist/forget ordering for a
   single account MUST be preserved (a `forget(acct-A)` issued after a
   `persist(acct-A)` must not execute before it). **Per-op thread spawn breaks
   this** — two spawned threads racing the Keychain can reorder a persist and a
   forget for the same account, corrupting at-rest secret state. Ordering is a
   first-class requirement, not an afterthought.
4. **Cancellation / timeout.** A wedged native handler (Keychain prompt left
   open; a bunker that never answers) MUST NOT strand the seam. Each op needs a
   wall-clock deadline; on overrun the seam surfaces a D6 toast / terminal
   action failure and moves on, exactly as `PendingSign` does.
5. **iOS Keychain reality.** `KeychainCapability.handleJSON(_:)` is a
   *synchronous blocking C callback* — there is no async host affordance to
   lean on (unlike `URLSession` for HTTP). The off-actor-ness must therefore be
   produced *on the Rust side*, by running the synchronous callback on a
   dedicated Rust thread; we cannot ask the host to "return immediately."
6. **D6 — errors are data.** Every failure (missing handler, NULL return,
   timeout, malformed request) crosses the boundary as a populated error
   envelope / action failure, never a panic. The existing `dispatch_capability`
   already guarantees this for the call itself; the worker must preserve it
   across the thread hop.
7. **D0 — substrate purity.** No NIP-specific or platform noun may enter
   `nmp-core`. The capability worker traffics only in the existing
   substrate-generic `CapabilityRequest` / `CapabilityEnvelope` types; the
   `dm_send` worker lives in `nmp-nip17` and re-enters via the already-public
   `ActorCommand::PublishSignedEvent`.

---

## Options considered

### Site 1 (DM `op.wait`) and Site 3 (cold-start signs) — settled by reuse

These two are **not** new design and are recorded here only to fix their
mechanism by reference:

- **Site 1 → worker-thread re-entry, reusing the nmp-nip57 lnurl pattern.**
  `gift_wrap_with_signer` already returns `SignerOp::Pending(rx)` and spawns
  its own off-actor driver. The fix is to stop calling `op.wait` on the actor
  and instead hand the op to a short-lived worker that blocks on it off-actor
  and re-enters with `ActorCommand::PublishSignedEvent` per envelope — the
  *exact* shape of `nmp-nip57/src/lnurl/mod.rs:244-296` (`ctx.command_sender_clone()`
  → `std::thread::spawn` → blocking work → `worker_tx.send(ActorCommand::…)`).
  (Note: the lnurl worker re-enters via `ActorCommand::Protocol(WalletPayInvoiceCommand)`;
  the DM worker re-enters via `PublishSignedEvent` — same structural pattern, different
  command.)
  No new primitive. Account-switch is a non-issue here: the worker re-enters via
  `PublishSignedEvent`, which carries a fully-signed, self-contained kind:1059
  envelope bound to no mutable account slot — applying it after an account
  switch publishes an already-built envelope, it cannot corrupt account state.
- **Site 3 → `PendingSign` park/poll/settle.** The three cold-start signs move
  from `sign_active` to the existing `sign_active_nonblocking` /
  `PendingSign` settlement path (`PublishTarget::Explicit` cold-start relays
  preserved, D6 "no cold-start relay" toast preserved). Verbatim reuse of the
  V-54 mechanism that already ships for normal publishes.

The remainder of this section weighs the genuinely-new piece: **Site 2, the
synchronous-native capability seam.**

### (a) Per-op spawned worker thread — REJECTED

For each in-actor capability dispatch, spawn a `std::thread`, run
`dispatch_capability` on it, re-enter via an `ActorCommand`.

- *Account-switch:* **broken.** Two ops for the same account (e.g.
  `persist(acct-A)` then `forget(acct-A)` from a rapid import-then-remove) race
  the Keychain on independent threads with no ordering guarantee; the forget can
  land before the persist, leaving a secret at rest that should have been
  deleted. This is the precise hazard the BACKLOG note flags
  ("per-op spawn forget/persist would race").
- *Complexity:* low per call, but unbounded thread churn under burst.
- *D8:* compliant (each thread blocks-recv), but ordering violation makes it
  unacceptable regardless.

### (b) Single long-lived serialized capability-worker thread — CHOSEN

One dedicated OS thread owns an `mpsc` work queue; the actor enqueues
`(CapabilityRequest, re-entry intent)` items; the worker drains them **in FIFO
order** via blocking `recv`, runs the synchronous native callback for each, and
re-enters the actor with the result via a typed `ActorCommand`.

- *Account-switch:* **correct by construction.** FIFO serialization means a
  `persist(acct-A)` enqueued before a `forget(acct-A)` executes before it.
  Results carry their originating `account_id` (already in `KeyringRequest`); a
  result whose account is no longer present when it re-enters is *applied to its
  own account or dropped*, never misapplied to whatever account happens to be
  active. The actor's re-entry arm checks the account against current identity
  state — a result for a removed account is a no-op + D6 trace, not a
  cross-account write.
- *D8:* compliant — the worker blocks on `recv()` (no poll); the actor's
  result re-entry is a normal `ActorCommand` dequeue (no poll).
- *Complexity:* one thread, one channel, one drain loop. Bounded resource use.
- *Cancellation/timeout:* each item carries a deadline; the worker abandons an
  overrun item with a D6 error result re-entry. A queue-shutdown signal (sender
  drop) terminates the thread on app teardown.

### (c) Make the capability callback itself async/non-blocking on the host — REJECTED for this class

This is the ADR-0024 shape (host runs the work on its own thread, calls
`nmp_app_deliver_capability_result` to re-enter). It is correct for HTTP, where
the host *has* an async affordance (`URLSession`). For the Keychain it is the
wrong tool: it pushes the serialization/ordering burden onto every host
implementation (each platform would have to build its own FIFO queue to avoid
the account-switch race), and the iOS Keychain API is synchronous anyway — the
host would just spawn a thread to wrap a blocking call, which is exactly what
the Rust-side worker does once, centrally, for all hosts. Keeping the host
callback synchronous and serializing on the Rust side is simpler, uniform across
platforms, and keeps ordering correctness in one auditable place.

### (d) Per-account worker thread — REJECTED

A worker per `account_id` would also preserve per-account ordering. Rejected as
over-engineered: it multiplies threads for no benefit (cross-account ordering
is irrelevant; a single FIFO already gives per-account ordering for free) and
complicates teardown. One serialized worker dominates it on every axis.

---

## Decision

Ratify the **three-primitive off-actor design**; introduce **one new seam** —
the serialized capability-worker thread.

### 1. DM gift-wrap moves off-actor via the lnurl-worker pattern (Site 1)

`SendGiftWrappedDmCommand::run` stops calling `op.wait` on the actor. It clones
the actor command sender (`ctx.command_sender_clone()`), spawns a short-lived
`std::thread` that blocks on the `SignerOp` returned by `gift_wrap_with_signer`
(off-actor; the 12 s budget now burns on the worker, not the actor), and for
each completed envelope re-enters the actor with
`ActorCommand::PublishSignedEvent { raw, target, correlation_id }`. On
gift-wrap failure/timeout the worker re-enters with `ShowToast` +
`RecordActionFailure` (D6). **No new primitive — verbatim reuse of
`nmp-nip57/src/lnurl/mod.rs`.**

### 2. Cold-start signs move onto `PendingSign` (Site 3 / V-54)

The three `create_account` cold-start publishes switch from `sign_active` to
`sign_active_nonblocking` and are parked as `PendingSign` with their explicit
cold-start relay targets (`PublishTarget::Explicit`) and D6 "no cold-start
relay" toasts preserved. **No new primitive — verbatim reuse of the existing
`PendingSign` path.**

### 3. The capability-worker seam (Site 2) — the new, ratified piece

Introduce a **single, long-lived, serialized capability-worker thread** owned
by the FFI app handle (`NmpApp`), created at app init alongside the existing
capability-callback slot:

- **Work queue.** An `mpsc::Sender<CapabilityWorkItem>` held by the actor side;
  the worker owns the `Receiver` and drains it with blocking `recv()` (D8 — no
  poll). A `CapabilityWorkItem` carries the substrate-generic
  `CapabilityRequest`, the originating `account_id` (already present in the
  request payload), a wall-clock `deadline`, and a typed re-entry intent
  (which `ActorCommand` to post on success/failure).
- **Dispatch from the actor.** Where the actor currently calls
  `self.dispatch_capability(&req)` synchronously (the keyring persist / recall /
  forget sites reached from `sign_in_local_nsec_with_keyring`,
  `restore_local_nsec_from_keyring`, and `session_persistence::*`), it instead
  *enqueues* the work item and returns immediately. The actor never blocks on a
  Keychain call again.
- **Worker execution.** The worker runs the existing
  `dispatch_capability(&slot, &request_json)` (unchanged — it already routes to
  the registered native callback and guarantees D6 error-as-data) on its own
  thread, honoring the per-item deadline.
- **Re-entry via a typed `ActorCommand`.** On completion the worker posts a new
  `ActorCommand::CapabilityResultReady { account_id, namespace, correlation_id,
  result_json }` (the typed re-entry; this supersedes ADR-0024's
  string-keyed `CapabilityResultReady` proposal for the native class). The
  actor applies it inside a normal tick: it routes the `CapabilityEnvelope` to
  the issuing wiring (e.g. `KeyringIdentityWiring`) **after** confirming the
  `account_id` still resolves to a known identity. A result for a
  since-removed account is dropped with a D6 trace — never applied to the
  now-active account.
- **Ordering = account-switch safety.** Because one worker drains one FIFO
  queue, persist/forget for any single account execute in enqueue order. The
  account-switch race of per-op spawn (option a) cannot occur.
- **Cancellation / teardown.** Dropping the sender (app teardown) closes the
  channel; the worker's blocking `recv()` returns `Err(Disconnected)` and the
  thread exits cleanly. A per-item deadline overrun yields a D6 error
  re-entry (`CapabilityResultReady` with an error envelope), clearing any host
  spinner.

The synchronous `nmp_app_dispatch_capability` C-ABI symbol **remains** for the
microsecond-class, *non-actor-thread* callers and for tests; this ADR only
moves the **in-actor** call sites onto the worker. No host migration churn for
callers already off the actor thread.

---

## Consequences

**What changes**

- `crates/nmp-nip17/src/dm_send.rs` — the `op.wait` loop becomes a
  worker-thread spawn that re-enters via `PublishSignedEvent` / `ShowToast` /
  `RecordActionFailure`. The actor no longer blocks up to ~24 s on a two-leg
  bunker DM.
- `crates/nmp-core/src/actor/commands/identity.rs:825,863,1018` — the three
  cold-start signs move to `sign_active_nonblocking` + `PendingSign`.
- `crates/nmp-ffi/src/lib.rs` (in-actor capability call sites) — synchronous
  `dispatch_capability` calls become enqueues to the capability worker.
- `crates/nmp-ffi/src/capability.rs` — gains the worker thread + work-queue
  plumbing on `NmpApp`; the synchronous symbol is retained for off-actor
  callers.

**New seams introduced**

- `ActorCommand::CapabilityResultReady { account_id, namespace,
  correlation_id, result_json }` — the typed re-entry for native capability
  results (supersedes ADR-0024's untyped proposal for this class).
- A `CapabilityWorkItem` type + a serialized capability-worker thread owned by
  `NmpApp`.
- (For Site 1) no new `ActorCommand` — `PublishSignedEvent` already exists.

**Migration path**

Ratify → land in three independently-shippable PRs (Site 1, Site 3, Site 2),
each green on its own; Site 2 is the only one that adds a thread/command and so
should land last with the fullest test coverage. ADR-0024 is marked
**Superseded (native class)** for its capability re-entry mechanism; its HTTP
saga framing is untouched (the lnurl worker already realizes it).

**Test strategy**

- *Site 1:* a `SendGiftWrappedDmCommand` test with a `Pending` remote signer
  asserts the actor tick returns *without* blocking and that two
  `PublishSignedEvent`s re-enter; a timeout test asserts a D6 toast +
  `RecordActionFailure` and **no actor stall** (assert the loop served other
  commands while the gift-wrap was in flight).
- *Site 2:* an account-switch ordering test — enqueue
  `persist(acct-A)` then `forget(acct-A)`, interleave a `SwitchActive`, and
  assert (i) the two keyring ops execute in FIFO order, and (ii) a result for a
  removed account is dropped, never applied to the active account. A timeout
  test asserts the worker abandons a wedged native handler with a D6 error
  re-entry. A "no poll" assertion: the worker advances only via blocking
  `recv` (covered by the `doctrine_lint` no-polling smoke + a unit test that
  the worker makes zero progress with an empty queue and no spin).
- *Site 3:* a bunker `create_account` test asserts the three cold-start signs
  park as `PendingSign` and the actor does not block.

**Risks**

- *Re-entry latency.* Moving Keychain off-actor adds one channel hop + one
  actor tick of latency to persist/recall/forget. This is intended (the actor
  no longer stalls) and bounded by emit-hz cadence; acceptable for a
  persist/forget that was previously a UI hitch.
- *Worker liveness.* A wedged worker (native handler hung past deadline) must
  still make forward progress on the *next* queued item. The per-item deadline
  + abandon-on-overrun guarantees this; the worker never blocks indefinitely on
  one item.
- *Ordering vs. parallelism.* Serialization trades capability throughput for
  ordering correctness. Capability ops are low-frequency (account
  persist/forget/switch), so single-threaded drain is not a throughput
  concern; correctness dominates.

---

## Implementation sketch (NOT code — file-level plan for the follow-up PRs)

**PR 1 — Site 1 (DM off-actor), smallest, no new seam.**
- `crates/nmp-nip17/src/dm_send.rs`: replace the `op.wait` loop body with a
  `ctx.command_sender_clone()` + `std::thread::spawn` worker that blocks on the
  `SignerOp` off-actor and re-enters via `PublishSignedEvent` (success) /
  `ShowToast` + `RecordActionFailure` (D6 failure/timeout), mirroring
  `crates/nmp-nip57/src/lnurl/mod.rs:244-296`.
- Tests: `crates/nmp-nip17/src/dm_send/tests.rs` — pending-signer non-block +
  timeout-no-stall cases.

**PR 2 — Site 3 (cold-start signs), no new seam.**
- `crates/nmp-core/src/actor/commands/identity.rs`: route the three
  `create_account` cold-start publishes (`:825,:863,:1018`) through
  `sign_active_nonblocking` + `PendingSign::with_target` (explicit cold-start
  relays), preserving the D6 "no cold-start relay" toast.
- Tests: bunker `create_account` non-block assertion in the identity command
  tests.

**PR 3 — Site 2 (capability-worker seam), the ratified new piece.**
- `crates/nmp-core/src/actor/mod.rs`: add
  `ActorCommand::CapabilityResultReady { account_id, namespace,
  correlation_id, result_json }` and its dispatch arm in
  `crates/nmp-core/src/actor/dispatch.rs` (route the envelope to the issuing
  wiring after an account-presence check; drop-with-trace for removed accounts).
- `crates/nmp-ffi/src/capability.rs`: add `CapabilityWorkItem` + the serialized
  worker thread (owned by `NmpApp`, blocking-`recv` drain, per-item deadline),
  and the enqueue entry point. Retain `nmp_app_dispatch_capability` for
  off-actor callers.
- `crates/nmp-ffi/src/lib.rs`: switch the in-actor capability call sites
  (`sign_in_local_nsec_with_keyring`, `restore_local_nsec_from_keyring`, and the
  `session_persistence::*` persist/forget/active sites reached from
  `SwitchActive`/`RemoveAccount`/`AddRemoteSigner`) from synchronous
  `dispatch_capability` to worker enqueue.
- Tests: `crates/nmp-ffi/src/capability.rs` tests — FIFO ordering under
  account-switch, removed-account result drop, deadline-abandon, and the
  no-poll worker-advances-only-on-recv assertion; plus
  `cargo test -p nmp-testing --test doctrine_lint_smoke`.
- Doc: mark ADR-0024 **Superseded (native capability class)**; update the V-90 /
  V-54 BACKLOG entries to **DONE** as each PR lands.

---

## Validation

Documentation only — no code changes in this PR. This ADR is the ratifiable
deliverable; the three PRs above gate the implementation work that follows.
