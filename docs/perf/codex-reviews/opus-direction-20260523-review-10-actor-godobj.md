# Opus direction review #10 — the actor as god object (code-grounded)

Date: 2026-05-23. Reviewer angle: distributed-systems / framework-API.
Scope: `crates/nmp-core/src/actor/` and the action / substrate seams that the
prompt claims it bottlenecks.

This review is code-grounded; every claim has a file:line. Where the prompt's
framing did not hold under inspection, I say so explicitly.

---

### 1. The god-object signal

`ActorCommand` (`crates/nmp-core/src/actor/mod.rs:203-769`) is **48 variants**
(I listed them; the prior `grep -c` of `49` double-counted a `#[cfg(test)]`
attribute line). Of those:

- **Protocol-generic substrate** — 14 variants: `Start`, `Configure`, `Stop`,
  `Reset`, `Shutdown`, `OpenTimeline`, `OpenAuthor`, `OpenThread`,
  `OpenFirehoseTag`, `CloseAuthor`, `CloseThread`, `LifecycleEvent`, `Kernel`,
  `ShowToast`, plus the three publish-engine primitives (`PublishUnsignedEvent`,
  `PublishUnsignedEventToRelays`, `PublishSignedEvent`) and the three planner /
  ack primitives (`PushInterest`, `WithdrawInterest`, `AckActionStage`).
- **Social-shaped / protocol-named** — 11 variants: `SignInNsec`,
  `SignInBunker`, `CreateAccount`, `SwitchActive`, `RemoveAccount`,
  `AddRemoteSigner`, `BunkerHandshakeProgress`, `PublishNote`, `PublishProfile`,
  `PublishRawEvent`, `React`, `Follow`, `Unfollow`, `SendGiftWrappedDm`.
- **App-noun feature-gated** — 3 wallet variants
  (`WalletConnect`/`WalletDisconnect`/`WalletPayInvoice`) plus `FetchLnurlInvoice`.

The structural signal is not LOC; it is the **closed-enum lock-in**. Every new
protocol verb requires editing `nmp-core/src/actor/mod.rs` AND adding a match
arm in `actor/dispatch.rs` (the dispatch fn ends at line 1043; 47 arms today).
An external crate (`nmp-marmot`, a hypothetical `nmp-nip02`) cannot extend the
verb set; the closed enum forecloses extension at the language level.

The contradiction the framework has not named: **`DispatchMlsOp`
(`mod.rs:744-755`, `dispatch.rs:862`) is already the proof-of-concept that
JSON-payload op dispatch works against host-owned state through the
`MlsOpHandler` slot (`crates/nmp-core/src/substrate/mls_op_handler.rs:80`).**
Yet `Follow`, `React`, and `SendGiftWrappedDm` remain first-class variants in
the same enum. The asymmetry is unprincipled — Marmot escapes the closed enum,
NIP-02 / NIP-25 / NIP-17 do not.

### 2. What the actor should own vs. delegate

aim.md §6 doctrine 1-7 lists what the *kernel* must own: event store,
replaceable-event invariants, outbox routing, subscription planner,
sessions-as-state, snapshot semantics. None of those say the kernel must own
the *kind:3 list merge* or *kind:7 reaction tag shape*.

What belongs in the actor (kernel substrate): publish engine + correlation_id
ledger, the planner (`PushInterest`/`WithdrawInterest`), identity slot writes,
relay control, snapshot tick, lifecycle, capability bridges.

What is currently in the actor that belongs in a protocol crate: the kind:3
merge logic (`commands/publish.rs:701-779` — `current_follows` + tag rebuild +
sign + publish) is exactly the NIP-02 reducer; the kind:7 reaction tag shape
(`React`); the NIP-17 seal-and-wrap routing (`SendGiftWrappedDm`,
`mod.rs:460-472`). Each is a 20-30-line "build unsigned event, hand to publish
engine" body — the existing `PublishAction::PublishRaw` path
(`publish/action.rs:137`) already proves this works without an actor arm.

What belongs in an app layer: `BunkerHandshakeProgress` is broker-internal
plumbing leaking into the kernel-shared enum; the wallet variants are
correctly feature-gated but `WalletPayInvoiceModule`
(`wallet/action.rs:78`) already exists as an `ActionModule` — the variant
duplicates it.

### 3. The testability cost — the prompt's framing was wrong

I expected `commands/wallet.rs` tests to need a full actor. They don't:
`wallet.rs:670` constructs only `Kernel::new(DEFAULT_VISIBLE_LIMIT)` +
`WalletRuntime::new(new_wallet_status_slot())` and calls the pure handlers
`wallet_connect` / `wallet_pay_invoice` / `handle_nwc_text` directly. No
channel, no actor thread, no observer slots. The handlers take
`&mut WalletRuntime, &mut Kernel` and return `Vec<OutboundMessage>` — they are
the unit-testable seam. The actor is a thin routing shell over them.

So the real cost is not "tests are hard" — it is **the dispatch match in
nmp-core owns the routing decision**: an external crate cannot register a new
verb because it cannot add a `match` arm to `dispatch.rs`. That is the
god-object cost. The substrate is mostly clean; the *enum* is the bottleneck.

### 4. What to move or delete

Concrete, in priority order:

- **`ActorCommand::Follow` / `Unfollow` (`mod.rs:496-510`, dispatch.rs:628-671).**
  Already namespaced `"nmp.follow"` (`kernel/update.rs:626`); the body
  (`commands::publish::follow`) builds an `UnsignedEvent { kind: 3, … }` and
  hands it to `publish_signed_with_correlation`. Replace with a
  `Nip02FollowSetModule: ActionModule` that calls `kernel.current_follows`,
  rebuilds the tag list, and dispatches `ActorCommand::PublishUnsignedEvent`.
  Two dispatch arms (~44 LOC) deleted from `nmp-core`; one new crate
  `nmp-nip02` (does not exist today: `ls crates/ | grep nip02` returns
  nothing).
- **`ActorCommand::React` (`mod.rs:482-493`, dispatch.rs:605-627).** Same
  shape; namespace `"chirp.react"` is already a hint it belongs in
  `nmp-relations` (which already exists). Move the dispatch body into a
  `ReactionModule::execute` that enqueues `PublishUnsignedEvent`.
- **`ActorCommand::WalletPayInvoice` (`mod.rs:569-573`, dispatch.rs:749-772).**
  Pure duplication: `WalletPayInvoiceModule` already implements the
  `ActionModule` contract (`wallet/action.rs:78`). The variant survives only
  because the dispatch arm has the `WalletRuntime` reference. Give the
  module access to the wallet runtime via a `WalletOpHandler` slot
  (mirror of `MlsOpHandlerSlot` at `substrate/mls_op_handler.rs:113`) and the
  variant deletes outright.

What the actor gets to delete: ~110 LOC of dispatch arms, three protocol-
shaped enum variants from `nmp-core`, and — the strategic win — proof that
the same pattern generalizes to every remaining protocol-named arm.

### 5. 30-day call

**Migrate `ActorCommand::Follow` and `ActorCommand::Unfollow` to a new
`crates/nmp-nip02/` crate behind a `Nip02FollowSetModule: ActionModule`,
delete both `dispatch.rs` arms, and confirm Chirp's follow button still
works.** Independently verifiable:

1. `grep "ActorCommand::Follow\|ActorCommand::Unfollow" crates/nmp-core/`
   returns 0 hits.
2. `ls crates/nmp-nip02/src/` exists; the crate impls `ActionModule`.
3. `app.register_action::<Nip02FollowSetModule>()` is called from
   `apps/chirp/.../nmp_app_chirp.rs` (mirroring `register_action` calls in
   `ffi/action.rs:808/833/850`).
4. Chirp's follow integration test (the AccountsView follow button) still
   green.

If this works for kind:3 it works for kind:7 (react) and kind:9734 (zap) the
same week — the migration template is the same. If it doesn't, the framework
thesis that "protocol nouns belong in protocol crates" is empirically refuted
on the simplest possible case and we learn that early.

### 6. What NMP is genuinely good at — do not throw out

When the next reviewer slims the actor, these are load-bearing and substrate-
generic; they must survive:

- **Dual-channel command/relay priority** (`mod.rs:991-998`). Separate
  `relay_rx` / `command_rx` so UI commands cannot be dropped under relay
  flood. This is the bible's "no high-frequency FFI loops" applied inward.
- **`action_stages` correlation_id triad**: `record_action_stage` /
  `record_action_failure` (`dispatch.rs:808-819`) /
  `record_action_success` (`dispatch.rs:820-828`) /
  `ack_action_stage` (`dispatch.rs:829-833`). Spinner-closing is invariant
  across every protocol verb; this is the *real* moat the prompt asked
  about, and it IS accessible to app developers via `dispatch_action` plus
  the namespaced `ActionModule` impls — `apps/longform/.../ffi.rs:103`
  drives `AddRelay` through `app.actor_sender()` without touching nmp-core.
- **Substrate seams already in place**: `register_snapshot_projection`,
  `register_event_observer`, `MlsOpHandlerSlot`, `push_interest` /
  `withdraw_interest`. These are *the* generalisation points; section 4's
  migration leans on them. Do not slim them; widen them (add
  `WalletOpHandler`, `SignerOpHandler`, etc., mirroring `MlsOpHandler`).
- **Outbox + planner**: `crates/nmp-core/src/planner/` + `kernel/outbox.rs`
  ARE the moat the prior review named, and the `LogicalInterest` push API
  is accessible from app crates (`apps/longform/.../ffi.rs:180`,
  `crates/nmp-nip17/src/inbox.rs:61`).

---

**Bottom line.** The actor *runtime* is not the god object; the
`ActorCommand` *enum* is. The substrate to fix it (`MlsOpHandler`,
`PublishAction::PublishRaw`, `register_snapshot_projection`, the
correlation_id ledger) is already built and exercised by app crates. The
30-day call — port NIP-02 follows to an `ActionModule` and delete two
dispatch arms — is the cheapest empirically verifiable proof that the
asymmetry between Marmot (escapes the enum) and Follow/React (locked in) is
an artifact, not a constraint.
