# ADR-0050 — Signer-session capability port: `sign | nip44_encrypt | nip44_decrypt`, mailbox-delivered completions

- **Status:** Implemented. The port grew `Nip44EncryptForAccount` /
  `Nip44DecryptForAccount`, `sign_timeout()` was renamed `op_timeout()`, and the
  `SignerForSeal` trait + `gift_wrap_with_signer` + the driver-thread execution
  model were deleted — `nmp-nip59` is now pure functions. Tracked on #961 / #960.
- **Date:** 2026-06-12
- **Issues:** #961 (V-08 — DM inbox silent failure for bunker accounts; its 2026-06-12 revision names this seam as the prerequisite), #960 (V-06 — NIP-42 AUTH via bunker rides this port), #1124 (F-13 — NIP-55 deadlines).
- **Related:**
  - **ADR-0026** — introduced `RemoteSignerHandle::nip44_encrypt` / `nip44_decrypt` and the `SignerForSeal` seal seam. The trait verbs survive; the `SignerForSeal` *execution model* (driver threads) is deleted by this ADR.
  - **ADR-0031** — NIP-46 broker transport. The broker's response hand-off changes from a direct cross-thread call to an actor-mailbox message (§D3).
  - **ADR-0043 Decision 2** — the `SignEventForAccount` backend-transparent sign port. This ADR generalizes it from one verb to three.
  - **ADR-0048 D3** — per-signer deadline self-description (`sign_timeout()`, NIP-46 = 5s, NIP-55 = 90s). This ADR extends the budget to all verbs and fixes the named-account park sites that ignore it.
  - **V-78** — signer backend (local vs bunker vs NIP-55) must be invisible to port consumers. Binding doctrine here.

---

## Context

### One port verb, three signing mechanisms

The actor's backend-transparent signing port (`ActorCommand::SignEventForAccount`,
`crates/nmp-core/src/actor/dispatch.rs:723`) supports exactly one verb: *sign*. Every
flow that needs the other two capabilities a signer self-describes
(`RemoteSignerHandle::nip44_encrypt` / `nip44_decrypt`,
`crates/nmp-core/src/remote_signer.rs:64,71`) has grown its own execution mechanism:

1. **The port + `PendingSignReturn`** (`crates/nmp-core/src/actor/pending_sign.rs`) —
   sign-only. Local keys resolve inline; remote ops park and are polled once per idle
   tick. Two sinks (`signed_events` projection, boxed continuation).
2. **The publish path's private park** — `PendingSign` (same file) plus a separate
   ~90-line inline drain in the actor loop (`crates/nmp-core/src/actor/mod.rs:2240-2329`)
   that duplicates the poll/timeout/error machinery with publish-specific terminal
   handling (outbox routing, toast, `correlation_id_override`).
3. **The `SignerForSeal` lash-up** (gift-wrap):
   - `nmp-nip59/src/signer_seal.rs` spawns a **per-invocation driver thread** that walks
     the encrypt→sign chain with blocking `recv_timeout` waits, governed by two parallel
     timeout constants (`DRIVER_STEP_TIMEOUT` = 5s, `GIFT_WRAP_TOTAL_TIMEOUT` = 12s) that
     shadow the ADR-0048 per-signer budget — a NIP-55 signer with a 90s budget is cut off
     at 5s inside the chain regardless.
   - `crates/nmp-core/src/actor/commands/remote_signer_for_seal.rs` is the
     **always-`Ready` adapter wart**: because `gift_wrap_with_signer` cannot accept a
     `Pending` `sign_seal` on its sync path, the adapter *blocks the calling thread up to
     5s* inside `sign_seal` and returns `Ready` — a synchronous wait dressed as a
     non-blocking op.
   - `nmp-nip17/src/dm_send.rs:290` spawns a **per-DM worker thread** whose only job is
     to block on `op.wait(GIFT_WRAP_TOTAL_TIMEOUT)` and re-enter the actor.

The receive side is a structural dead-end on top: `DmInboxProjection`
(`nmp-nip17/src/inbox.rs:179`) clones raw `nostr::Keys` per envelope out of a per-crate
`Arc<Mutex<Option<Keys>>>` slot. A bunker account has no `Keys`, so the inbox cannot
decrypt **at all** — V-08. The raw-keys slot is also the largest remaining D13 erosion:
key material held outside the identity runtime, cloned on every envelope.

### Completion delivery is tick-luck

When a NIP-46 response (kind:24133) arrives, the broker's own relay-dispatcher thread
decrypts it and calls `Nip46Signer::ingest_rpc_response` **directly on that thread**
(`crates/nmp-signer-broker/src/broker.rs:340` → `transport.rs:89` →
`crates/nmp-signers/src/signers/nip46/mod.rs:189`), which resolves the parked op's mpsc
channel. (The `RemoteSignerHandle::deliver_response` impl in
`crates/nmp-ffi/src/signer_broker.rs:186` is effectively dead for steady-state inbound —
the broker bypasses it.) NIP-55 resolves the channel from the host capability bridge
thread (`crates/nmp-ffi/src/external_signer.rs:160-187`). Nothing wakes the actor: the
parked op is only noticed on the next idle tick, paced by the hardcoded 250ms cap in
`compute_wait` (`crates/nmp-core/src/actor/tick.rs:27`).

Worse, **sending an `ActorCommand` does not wake the actor either**: the loop's only
blocking point is `relay_rx.recv_timeout` (`actor/mod.rs:1999`); commands are drained
with `try_recv` at the top of each iteration (`actor/mod.rs:1862-1875`). So even a
completion *message* would sit in the command channel for up to 250ms when no relay
traffic flows. The same latency afflicts the existing ADR-0040
`CapabilityResultReady` re-entry today. Any mailbox-completion design must therefore
also make command arrival a genuine wake.

### The named-account deadline defect

Both sign-and-return dispatch arms compute their park deadline from the **active**
account — `ctx.identity.active_sign_deadline()` at `dispatch.rs:709`
(`SignEventForReturn`) and `dispatch.rs:763` (`SignEventForAccount`) — even when
`account_pubkey` / `signer_pubkey` names a different roster key. The publish path already
does this correctly (`sign_deadline_for(signer_pubkey.as_deref())`,
`crates/nmp-core/src/actor/commands/publish.rs:173,268`). Concretely: signing with a
named NIP-55 roster key (90s budget) while a local or NIP-46 account is active parks with
the active account's 5s budget and times out before the user can approve in Amber.
(The `active_sign_deadline()` calls at `publish.rs:522,605,688` are correct — those paths
sign with `sign_active_nonblocking` and have no named-account parameter.)

### Why a bulk bunker decrypt cannot be "just do the RPCs"

Unsealing ONE kind:1059 envelope needs TWO **sequential** `nip44_decrypt` round-trips
(outer wrap → learn the kind:13 seal, then the seal). An N-envelope backfill is O(2N)
sequential interactive round-trips — minutes for a modest inbox, and the bunker may
prompt the user per operation. #961's revision already rejected the per-envelope-RPC
backfill plan; this ADR decides the destination (§D7).

## Decision

### D1 — The port grows two verbs

`ActorCommand` gains two siblings of `SignEventForAccount`, identical in shape and
backend transparency:

```rust
Nip44EncryptForAccount { peer_pubkey: String, plaintext: String,
                         signer_pubkey: Option<String>, continuation: CipherContinuation }
Nip44DecryptForAccount { peer_pubkey: String, ciphertext: String,
                         signer_pubkey: Option<String>, continuation: CipherContinuation }
```

`CipherContinuation` is the `String`-payload twin of `SignContinuation`
(`FnOnce(Result<String, String>)`, same newtype/Debug treatment,
`crates/nmp-core/src/actor/mod.rs:333`). `signer_pubkey: None` = active account, matching
the sign verb byte-for-byte. D0: `nip44_*` are capability verb names already present in
`nmp-core` since ADR-0026 (`remote_signer.rs`); they name a cryptographic capability, not
an app concept. Continuations never receive key material — only ciphertext/plaintext/
`SignedEvent` (D13: after Stage 4 the **DM inbox** no longer holds `nostr::Keys`. The
broader `active_local_keys` / `mls_local_nsec` slots that Marmot and the
protocol-context host bridge depend on remain and are out of scope here — V-55/#971
tracks the global property).

The identity runtime gains the local halves the verbs need (none exist today):
`nip44_encrypt_nonblocking` / `nip44_decrypt_nonblocking`, routing local accounts through
`nostr::nips::nip44` **inside the runtime** (keys never escape) and remote accounts
through the handle's existing trait methods. Same shape as `sign_with_account_nonblocking`.

### D2 — One park, one drain, three-plus-one sinks

`PendingSign` (publish) and `PendingSignReturn` (sign-and-return) collapse into a single
parked-op type with a sink enum:

- `SignedEventsProjection { correlation_id }` — unchanged.
- `SignContinuation` — unchanged.
- `CipherContinuation` — new (D1); parks `SignerOp<String>`.
- `Publish { p_tags, target, correlation_id_override }` — the publish path's terminal,
  migrated from the inline `mod.rs:2240-2329` drain. Resolution **returns** the outbound
  frames / emit obligation to the actor loop (the loop owns relay routing); the
  poll/timeout/error machinery exists once, in `pending_sign.rs` (split into sibling
  modules if the 500-LOC ceiling approaches).

The ~90-line inline publish drain in `actor/mod.rs` is **deleted**; the loop runs one
`retain_mut` over one `Vec`. All existing terminal behaviors are preserved exactly
(toasts, `record_action_failure`, `publish_signed_to_with_correlation`, immediate
`emit_now`).

### D3 — Completions are actor-mailbox messages, and the mailbox actually wakes

**D3a — one waking inbox.** The actor's two bare mpsc channels (commands; `PoolEvent`s)
are replaced by a single blocking inbox the loop `recv_timeout`s on, carrying
`Command(ActorCommand) | Relay(PoolEvent)`. Command-lane priority is preserved exactly
(the `CommandDrain` budget keeps governing how many commands run before relay/idle work —
classification into two local lanes happens after the one blocking receive). `nmp-network`
gains a narrow `PoolEventSink` seam for `Pool::new` with a blanket impl for the existing
`mpsc::Sender<PoolEvent>` so every other `Pool` consumer (`nmp-signer-broker`, tests)
compiles unchanged; the actor passes its inbox's relay-side handle. After this, *any*
`ActorCommand` send is a genuine wake — fixing the latent ≤250ms latency on
`CapabilityResultReady` (ADR-0040) and host commands too, not just signer completions.

**D3b — `DeliverSignerResponse`.** New
`ActorCommand::DeliverSignerResponse { response_json: String }`:

- **NIP-46**: the broker's steady-state inbound dispatcher
  (`nmp-signer-broker/src/broker.rs:340` / `transport.rs:89`) stops calling
  `ingest_rpc_response` directly. The nmp-ffi adapter — which constructs the broker and
  already owns the app's command sender — installs a **completion sink closure**
  (`Fn(String) + Send`) on the broker; the dispatcher hands the decrypted RPC body to the
  sink, which sends `DeliverSignerResponse`. `nmp-signer-broker` stays `nmp-core`-free
  (D0): it sees an opaque sink, never `ActorCommand`. The handshake path is untouched (it
  runs on its own worker with its own `await_response` loop and completes via the
  existing `AddSigner { RemoteHandle }` re-entry).
- **NIP-55**: `nmp-ffi/src/external_signer.rs::deliver` sends the command instead of
  fanning out to signer handles on the bridge thread.

The dispatch arm fans the JSON out to the identity runtime's remote handles (each
silently drops non-matching correlation ids — the existing trait contract, and exactly
what the NIP-55 registry fan-out does today), then the same loop iteration drains the
parked-op `Vec` (the drains run unconditionally after the command lane — no `continue`
skips them).

Consequences:

- The inbox wake **is** the completion path: latency = mailbox latency, not ≤250ms tick
  luck (Erlang deferred-`gen_server`-reply style).
- Signer pending-map mutation happens on the actor thread — one writer (D4).
- No park/response race exists: the signer creates and parks the op's channel before the
  RPC leaves, so a completion processed later at worst buffers a value the drain picks
  up.
- The residual 250ms idle sweep remains **solely as the deadline gate** (wall-clock-gated
  timeout detection, D8-compliant). It is no longer how completions are noticed. No new
  sleeps, no polling loops anywhere in the change (D8).

### D4 — Per-verb deadline = the signing account's self-described budget

Every park site computes its deadline via
`identity.sign_deadline_for(signer_pubkey.as_deref())` — including the two defective
arms (`dispatch.rs:709,763`). `RemoteSignerHandle::sign_timeout()` is renamed
**`op_timeout()`** (hard break, no compat alias — repo rule) because it now budgets all
three verbs; one budget per backend (NIP-46 = 5s, NIP-55 = 90s) applies uniformly.
Per-verb differentiation inside one backend is deliberately not provided until a real
backend demands it.

### D5 — Gift-wrap is a continuation chain through the port; `SignerForSeal`'s execution model is deleted

`nmp-nip59` reduces to **pure functions**: `build_seal_unsigned` and `wrap_signed_seal`
become `pub`; the unwrap side is split into pure halves (parse outer → ciphertext+peer;
parse seal → rumor) so Stage 4 can route decryption through the port. **Deleted:** the
`SignerForSeal` trait + `Keys` blanket impl, `gift_wrap_with_signer`, the driver thread,
`DRIVER_STEP_TIMEOUT`, `GIFT_WRAP_TOTAL_TIMEOUT`; in `nmp-core`:
`remote_signer_for_seal.rs`, `IdentityRuntime::active_signer_for_seal`,
`LocalSignerAccess::signer_for_seal` + the `ProtocolCommandContext` wrapper; in
`nmp-nip17`: the per-DM worker thread.

`SendGiftWrappedDmCommand` becomes a chain of port requests composed via the cloned
`Sender<ActorCommand>`:

1. `Nip44EncryptForAccount(receiver, rumor_json)` →
2. continuation builds the kind:13 seal `UnsignedEvent` (pure, on-actor) and sends
   `SignEventForAccount(seal)` →
3. continuation assembles the kind:1059 wrap locally (fresh ephemeral key, in-process —
   the NIP-59 unlinkability guarantee is untouched) and sends `PublishSignedEvent`.

Envelope order is sequential and failure-preserving: the recipient chain runs first; its
success continuation launches the self-copy chain; self-copy failures surface a toast
only (single-terminal contract — the action verdict is the recipient envelope's,
unchanged from today's semantics). Per-step deadlines are the port's (§D4) — the dual
timeout constants disappear rather than being renamed.

**Account pinning.** Today the signer is resolved once and both envelopes bind the same
`Arc`; a chain of port requests would re-resolve "active" at every step, so a mid-chain
account switch could sign the seal with a different key than the one that encrypted it.
The chain therefore resolves the active account's pubkey **once, at step 1**, and every
subsequent step passes `signer_pubkey: Some(hex)` — never `None`. Oracle: a DM-send
chain whose active account switches mid-flight signs the seal with the originating
account.

**Mailbox cost, stated honestly.** Each chain step issued via the `Sender` is one
mailbox enqueue/dequeue even for local accounts (~3 hops per DM send vs. today's zero).
This is accepted: DM sends are rare, the hops are processed in the same priority-lane
drain burst, and the per-hop cost is trivial next to the NIP-44 ECDH work itself. A
parallel inline-bypass path for local keys is rejected (one mechanism per concern); if
profiling ever shows the hops matter, that is the moment to revisit — not before.

**nmp-marmot.** Marmot gift-wraps MLS welcome rumors via
`gift_wrap_with_signer(Arc::new(self.keys.clone()), ..).wait(GIFT_WRAP_TOTAL_TIMEOUT)`
(`crates/nmp-marmot/src/service.rs:480-483`) — it consumes the trait, the `Keys` blanket
impl, the driver, and the timeout constant. Marmot is local-key-only by construction
(its MLS identity key never lives in a bunker), so it migrates to the **pure functions**
directly: NIP-44-encrypt with its own `Keys`, `build_seal_unsigned`, sign in-process,
`wrap_signed_seal` — synchronous, no port, no thread. `cargo test -p nmp-marmot` green
is a Stage 3 oracle.

### D6 — Gift-unwrap through the port; the raw-`Keys` slot is deleted

`DmInboxProjection` drops `Arc<Mutex<Option<nostr::Keys>>>`. Decryption becomes the same
two-step continuation chain (outer: peer = the wrap's ephemeral `event.pubkey`; seal:
peer = the seal's `pubkey`), issued via a `Sender<ActorCommand>` the projection is
constructed with; the active pubkey comes from the pubkey-only identity accessor
(#1191). Bunker accounts become *structurally* able to decrypt; whether and how much
they do is policy (§D7), not structure.

Like §D5, the chain is **account-pinned at envelope arrival** (`signer_pubkey:
Some(hex)` on both steps), and the terminal insert carries an **epoch guard**: the
projection keeps a generation counter bumped by `clear()` (account switch, #1138); a
continuation completing for a stale generation discards its message instead of leaking a
previous account's plaintext into the new account's snapshot.

**Backfill cost, stated honestly.** A local-account backfill of N envelopes becomes 2N
small `Nip44DecryptForAccount` commands instead of today's zero (inline synchronous
unwrap). Accepted for the same single-mechanism reason as §D5: the mailbox hop is cheap
next to the two ECDH+ChaCha operations per envelope that dominate either way, and the
command lane processes them in priority bursts. An inline local-keys bypass would be a
second decrypt mechanism and is rejected; profiling gates any future revisit.

### D7 — Bulk-decrypt destination: bounded interactive policy now; delegated session deferred

Unbounded per-envelope bunker backfill is rejected (O(2N) interactive round-trips).
Decision:

- **Land policy (b):** remote-signer accounts decrypt through a **bounded, strictly
  sequential per-account decrypt queue** (one envelope in flight; bounded depth,
  newest-first admission). Envelopes beyond the bound are **not silently dropped**: the
  inbox projection surfaces a policy state — count of undecrypted envelopes and a
  `decrypt_state` discriminator (e.g. `ok | limited | unavailable`) — replacing the
  structural `remote_signer_unsupported: bool` with policy-driven state (D6,
  errors-as-state). Exact field shape is fixed at Stage 5 with its host-decoder updates.
- **Defer capability (a):** a delegated decrypt-session capability (NIP-46 verb
  extension negotiating scoped conversation-key export / batch decrypt) is filed as a
  tracked post-v1 issue, not landed now — NMP owns its broker (ADR-0031) so the verb set
  is extensible, but interop with third-party bunkers (nsec.app, Amber) is unproven and
  the protocol-extension design deserves its own ADR.

### Supersessions

- **#961 Stage 3 (per-envelope RPC backfill)** — already rejected in the issue's
  2026-06-12 revision; this ADR is the prerequisite-seam spec that revision names.
- **#960 Stage 3 ("drive the broker RPC synchronously")** — NIP-42 AUTH signing for
  bunker accounts rides the §D1 port (sign verb + continuation) instead. The publish
  engine does not own a separate AUTH signer shim; AUTH-REQUIRED publish retries wait on
  the shared `nmp-nip42` authenticated-relay state.
- **ADR-0026 Phase 2 execution model** — `RemoteSignerForSeal` and the driver-thread
  chain are deleted (§D5). ADR-0026's *trait verbs* on `RemoteSignerHandle` are the
  foundation this ADR builds on and remain.

## Implementation record (the stages, as landed)

The work landed one PR per stage, TDD-first:

| Stage | Content |
|---|---|
| 1 | This ADR. |
| 2 | D1 verbs + D2 unified park/drain + D3 waking inbox & mailbox completions + D4 deadline fix & `op_timeout` rename. |
| 3 | D5 gift-wrap chain + deletions (incl. nmp-marmot pure-function migration). |
| 4 | D6 unwrap through the port + raw-`Keys` slot deletion. |
| 5 | D7 policy + projection state + host decoders; the delegated-session issue is filed post-v1. |

## Consequences

- **One signing mechanism.** Port verbs + one park/drain + mailbox completions replace
  three parallel execution models; every future signer backend (hardware, NIP-07) plugs
  in at `RemoteSignerHandle` and inherits the whole surface.
- **Bunker accounts stop being second-class:** DM send loses its thread zoo, DM receive
  becomes possible under explicit policy, NIP-42 AUTH gets its seam (#960).
- **Latency:** remote completions arrive at mailbox speed; the 250ms tick is demoted to
  a timeout sweep.
- **Risk:** Stage 2 touches the actor loop's hottest seam; the stage is sequenced first
  and gated on the existing sign suites passing unchanged. Stage 3/4 deletions are
  mechanical once Stage 2 exists.
