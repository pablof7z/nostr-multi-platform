# ADR-0052: Instance-scoped extension seams — register values, not types

Status: PROPOSED

> Numbering note: `0051` is the highest decision in `docs/decisions/`. This
> takes the next free number, `0052`, per the single-source-of-truth /
> no-duplicate-id discipline.

## Context

NMP's host-extension seams are currently **type-registered**, not
value-registered. `ActionModule` (`crates/nmp-core/src/substrate/action.rs`)
declares its lifecycle methods as associated functions with no receiver:

```rust
fn start(ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection>;
fn execute(action: Self::Action, correlation_id: &str, send: &dyn Fn(ActorCommand)) -> Result<(), String>;
```

The registrar mirrors this:

```rust
fn register_action<M: ActionModule + 'static>(&mut self);  // a TYPE, not a value
```

The registry stores a zero-sized `ActionModuleAdapter<M>(PhantomData<M>)` — it
holds no per-module state because there is nowhere to put it. Consequently any
**stateful** module must reach its state through a **process-global**. The
codebase has accreted **five** such ambient-authority globals to work around
the static seam — two FFI-side handle holders and three `nmp-core`/crate hook
or runtime statics they drive:

1. `ACTIVE_WALLET_RUNTIME: OnceLock<WalletRuntimeHandle>`
   (`crates/nmp-nip47/src/runtime.rs:217`) — the NIP-47 wallet runtime. The
   three wallet `ActionModule`s (`action.rs:89,125,180`) read it via
   `active_wallet_runtime()`; a test in `action.rs` self-admits it races
   sibling tests through the `OnceLock` (`action.rs:341`).
2. The bunker `HOOK: OnceLock<RwLock<Option<BunkerHookFn>>>`
   (`crates/nmp-core/src/bunker_hook.rs:44`), written by `register_bunker_hook`,
   driven by
3. `GLOBAL_BROKER: OnceLock<Arc<BunkerBroker>>`
   (`crates/nmp-ffi/src/signer_broker.rs:23`).
4. The NIP-55 external-signer `HOOK: OnceLock<RwLock<Option<ExternalSignerHookFn>>>`
   (`crates/nmp-core/src/external_signer_hook.rs:38`) — the **structural twin**
   of (2), written by `register_external_signer_hook`, exported from
   `nmp-core/src/lib.rs:199`, invoked from the actor at
   `actor/commands/identity.rs:1484`; driven by
5. `GLOBAL_DRIVER: OnceLock<Arc<Nip55Driver>>` +
   `register_external_signer_hook`
   (`crates/nmp-ffi/src/external_signer.rs:49`).

### Consequences of ambient authority

- **Cross-instance crosstalk.** Two `NmpApp` instances in one process share one
  wallet runtime, one bunker broker, and one NIP-55 driver. A second app
  instance silently rides the first app's wallet/signer; a freed-and-recreated
  app dead-ends because the `OnceLock` is already fired (the Android
  process-reuse failure mode).
- **Capability leak.** Because a boxed third-party `ProtocolCommand` cannot
  capture its dependencies at composition time, `ProtocolCommandContext`
  exposes `kernel_mut() -> Option<&mut Kernel>`
  (`crates/nmp-core/src/substrate/protocol.rs:450`), handing the entire kernel
  to any command body — defeating the narrow capability traits (`clock()`,
  `signers()`, `dms()`, `errors()`, `stages()`, `recipients()`) that the same
  context already offers. Its sole production caller is the NIP-47 protocol
  command (`crates/nmp-nip47/src/protocol.rs:54`).
- **Two write seams for one concern.** `ActorCommand::Protocol(Box<dyn
  ProtocolCommand>)` and `ActorCommand::DispatchHostOp { … }` are two open doors
  to the same "host runs a write op on the actor thread" capability —
  fragmentation (AGENTS.md §"Zero-tolerance", D-corollary "one canonical path").
- **`lnurl_for_pubkey` on the generic context.**
  `ProtocolCommandContext::lnurl_for_pubkey` (`protocol.rs:620`) is a
  zap-specific read bolted onto the seam every command sees, rather than a
  capability the zap command carries.

The right fix already exists in-tree and is documented as law: `nmp_app_new`
threads ~20 per-app `Arc` slots (`ActiveAccountSlot`, `EventStoreSlot`,
`MlsLocalNsecSlot`, …) into the actor with the invariant **"no global aliasing
across `nmp_app_free`"** (`crates/nmp-ffi/src/lib.rs`). K1 (ADR-0050) applied
exactly this shape to the signer-session port. K2 applies it to the extension
seams.

## Decision

Make every host-extension seam **instance-scoped**: register **values that
carry their dependencies**, captured at composition time, behind per-app slots
created in `nmp_app_new` and dropped in `nmp_app_free`. Delete the five
process-globals. No public namespace, JSON wire shape, FFI symbol, or snapshot
projection changes — this is a mechanical re-seating of where state lives, not a
protocol change.

### D1 — `ActionModule` becomes value-registered

`register_action` takes a **value**:

```rust
fn register_action<M: ActionModule>(&mut self, module: M);
```

`ActionModule::start` / `execute` gain `&self`. The registry stores
`Box<dyn ErasedActionModule>` built from the concrete module value (not a ZST
`PhantomData` adapter). The erased adapter holds the module by value, so a
module may own an `Arc<WalletRuntimeHandle>`, an `Arc<DmRelayCache>`, etc.,
captured when the host registers it. Stateless modules (`PublishAction`, browse)
register a unit-shaped value and are unaffected at the call site beyond passing
`Default::default()` (or the existing ZST) as the value.

This is mechanical: the registry already boxes its adapters and round-trips
typed actions through serde at the boundary; only the adapter's storage changes
from `PhantomData<M>` to `M`, and the four trait methods gain a receiver.

### D2 — `ProtocolCommand` instances carry their dependencies

A `ProtocolCommand` is already a boxed value (`Box<dyn ProtocolCommand>`) on
`ActorCommand::Protocol`. It therefore **already can** capture its dependencies
in its fields at construction time. K2 makes this the *only* way a command
reaches host/app state: the NIP-47 protocol command captures its
`WalletRuntimeHandle` (and any `Arc<DmRelayCache>`) when the composition root
builds it, instead of reading a global inside `run`. No new mechanism — we stop
using the escape hatch.

### D3 — per-app capability ports replace the three FFI globals

`nmp_app_new` constructs per-app slots for the bunker broker hook and the
NIP-55 driver hook (mirroring `signer_session` slot wiring from ADR-0050) and
hands `Arc::clone`s to the actor. `nmp_signer_broker_init(app)` /
`nmp_external_signer_init(app)` write the per-app slot instead of a global
`OnceLock`. **Both `nmp-core` hook statics are deleted together** — they are
twins: the bunker `HOOK` (`bunker_hook.rs:44`) AND the NIP-55 external-signer
`HOOK` (`external_signer_hook.rs:38`). Each is replaced by a hook slot owned by
the kernel/actor and set through the existing capability-slot plumbing. This is
reachable without a new global because the actor already holds per-app
`&IdentityRuntime` + `&mut Kernel` at both hook call sites
(`identity.rs` bunker handshake / `identity.rs:1484` NIP-55), exactly as
ADR-0051 wired the relay-connected hook slot off the actor
(`actor/mod.rs:1412`). Result: two apps in one process have two independent
brokers/drivers, and a freed-then-recreated app re-initialises cleanly.

A **correlation token** is threaded through `BunkerHookRequest` /
the signer-ready return so a broker response is routed to the originating app
instance (the token does not exist on `BunkerHookRequest` today —
`bunker_hook.rs:32` — K2 adds it). If a separate session lands the
`make_active=0`-honoring bunker hygiene work first, K2 reuses its token rather
than adding a second one (single canonical path).

### D4 — unify `DispatchHostOp` into `Protocol` behind one seam

`ActorCommand::DispatchHostOp { … }` and `ActorCommand::Protocol(Box<dyn
ProtocolCommand>)` are two doors to the same *concern* — a host runs a write op
on the actor thread with a snapshot-projected result — but they are **not
behaviourally identical today**, and the merge MUST preserve both differences
rather than silently dropping them:

1. **Panic isolation asymmetry.** The `DispatchHostOp` arm wraps the *entire*
   `handler.handle()` call in `catch_unwind`, converting a panic to
   `{"ok":false,…}` (`actor/dispatch.rs:1205-1206`). The `Protocol` arm calls
   `cmd.run(&mut pctx)` **bare** (`dispatch.rs:1728`); panic isolation there is
   per-capability-accessor only (the D15 `catch_unwind` shortcuts at
   `protocol.rs:500+`), NOT whole-body. A `ProtocolCommand::run` that panics in
   its own non-capability logic unwinds the actor thread.
2. **Persistent handler vs one-shot command.** `DispatchHostOp` dispatches to a
   *persistent, app-installed* `Arc<dyn HostOpHandler>` (the Marmot MLS
   service, hot-swappable on account switch). A `ProtocolCommand` is a one-shot
   value **consumed** on `run(self: Box<Self>)`.

Therefore D4 is: **before** deleting `DispatchHostOp`/`HostOpHandler`, (a) add
**whole-body `catch_unwind`** to the `Protocol` dispatch arm so a panicking
third-party handler still yields `{"ok":false,…}`; and (b) express the
persistent installed handler as a **re-runnable command factory** held in a
per-app slot (the host installs the `Arc<dyn HostOpHandler>`-equivalent into a
slot; each host-op mints a fresh `ProtocolCommand` that captures an
`Arc::clone` of it — D2-style). Only once both guarantees are reproduced does
`DispatchHostOp` get deleted, leaving exactly one host-write seam. Oracle: a
**panicking-handler behavioural test** — a host op whose handler panics still
returns `{"ok":false,…}` and the actor survives, before AND after the merge.

### D5 — delete `ctx.kernel_mut()`; move `lnurl_for_pubkey` to a carried capability

Once D2 removes the wallet command's reliance on the kernel handle, the residual
`kernel_mut()` callers are enumerated; any genuine kernel service one of them
needs is promoted to a **narrow capability trait** on
`ProtocolCommandContext` (joining `clock`/`signers`/`dms`/…). Then
`kernel_mut()` is removed — success is type-level: the method no longer exists.
`lnurl_for_pubkey` moves off the generic context onto a capability the zap
command carries (D2-style), so only the zap path can read it.

### D6 — regression gate: doctrine-lint "no ambient authority"

A new doctrine-lint rule bans, in production `nmp-*` crate sources,
`OnceLock` / `lazy_static!` / `static … (Mutex|RwLock|AtomicPtr)` that hold
**non-const** state. A justification-required allowlist is seeded with any
known remaining global, each entry citing a tracking issue, with the documented
goal of an **empty** allowlist. The existing `active_pubkey` / pubkey-only
slot sweep (tactical PR #1191) is verified-not-redone if already landed.

**Rule number.** The highest doctrine rule on `master` is **D19**
(`crates/nmp-testing/bin/doctrine-lint/rules/d19.rs`); there is no D20 in the
rules dir yet. In-flight PR #1311 takes **D20** for an unrelated wasm-time lint
(bans `Instant::now`/`SystemTime::now`). K2 therefore takes **D21**. This is
resolved at rung 5.6 time, not now: if #1311 has merged, D20 is taken and K2 is
D21; if #1311 has died, K2 reclaims **D20**. (No durable doc reserves D21 for
another meaning — a reviewer claim of a "correlation-linearity" reservation in
`docs/wiki/episodes/…` was checked and the cited file does not exist.)

## Sequencing (one PR per rung; TDD-first)

| Rung | Scope | Oracle |
|------|-------|--------|
| 5.1  | This ADR | review sign-off |
| 5.2  | D1 + migrate nmp-nip47; delete `ACTIVE_WALLET_RUNTIME`; fix the self-admitted test race | **two-instance interop test**: two `NmpApp`s, two wallets, zero crosstalk |
| 5.3  | D3 per-app bunker + NIP-55 ports; delete `GLOBAL_BROKER`, `GLOBAL_DRIVER`, **both** nmp-core hook statics (`bunker_hook`, `external_signer_hook`); correlation token | **free + new-app recreation test** (Android process-reuse) |
| 5.4  | D4 add whole-body `catch_unwind` to `Protocol` arm + persistent-handler factory slot, then delete `DispatchHostOp`/`HostOpHandler` | **panicking-handler behavioural test**: host op whose handler panics still returns `{"ok":false}`, actor survives |
| 5.5  | D5 delete `kernel_mut()`; move `lnurl_for_pubkey` to a carried capability | type-level: `kernel_mut` no longer compiles-exists |
| 5.6  | D6 doctrine-lint D21 (or D20 if #1311 died) + allowlist | lint fails on a planted global (pos fixture), passes clean tree (neg fixture) |

Each rung rebases on `origin/master` first and rechecks the live seam (the repo
moves fast; line refs above are 2026-06-13 snapshots and MUST be re-verified).

### Collision sequencing (open PRs as of 2026-06-13)

- **PR #1312 (`fix/issue-619-walletruntime-ordering`, OPEN, currently red)
  directly collides with rung 5.2.** It builds *more* on the exact global 5.2
  deletes: it lifts wallet wiring into `nmp-defaults::NmpAppBuilder::with_wallet`,
  calls the **type-registered** `register_action::<WalletConnectModule>()` form
  that D1 replaces with the value form, and type-enforces the
  `install_wallet_runtime` `OnceLock` ordering that 5.2 removes. They **cannot
  both merge as-is.** Resolution: if #1312 lands first, rung 5.2 rebases over it
  — converting its `register_action::<M>()` call sites to value form and
  replacing the `OnceLock`-ordering contract with builder-time composition
  (the `.with_wallet()` step composes the module *value* that owns the runtime
  handle, so install-before-dispatch is still type-expressed, now without a
  global). If #1312 is abandoned, 5.2 supersedes it.
- **PR #1311 (`d20-wasm-time-lint`, OPEN, red)** claims D20 — see the D6 rule-number note.
- **PR #1305 (`android-signer-push`)** touches only `nmp-android-ffi/src/external_signer.rs`,
  a different crate from K2's `nmp-ffi`/`nmp-core` seams — **not** a collision.

## Consequences

**Positive.** Ambient authority is eliminated; two app instances are fully
independent; the kernel-handle capability leak is closed; two write seams become
one; a lint prevents regression. The framework thesis (a host composes NMP from
values) is strengthened — composition is explicit and inspectable.

**Costs.** `ActionModule` gaining `&self` is a breaking trait change for any
external consumer (downstream apps pin by git rev, so this is a coordinated
bump, not a silent break). The `BunkerHookRequest` token is a wire-internal
addition behind the FFI boundary (no public schema change).

**Risks / non-goals.** K2 does **not** change which NIPs exist, any JSON action
shape, or any snapshot projection. If a rung proves unsafe to land in isolation
(e.g. a downstream interface needs a coordinated release), the safe prefix lands
and the blocker is reported precisely rather than papered over.

## Doctrine alignment

D0 preserved (no protocol leaks into `nmp-core`; the wallet runtime is **not**
threaded through `ActionModule` generically — it is owned by the concrete module
value). D4/D6/D8/D13 unaffected. AGENTS.md §"Zero-tolerance": this deletes
fragmentation (two write seams → one) and removes five undocumented "temporary"
globals rather than adding a sixth.
