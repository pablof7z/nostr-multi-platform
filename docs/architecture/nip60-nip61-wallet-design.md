# NIP-60/NIP-61 Wallet Architecture

> Status: draft design for [#1001](https://github.com/pablof7z/nostr-multi-platform/issues/1001).
> Date: 2026-07-03.

This document records the post-v1 architecture for NIP-60 Cashu wallets and
NIP-61 nutzaps in NMP. It is a design, not an implementation claim. The support
matrix remains authoritative for release status.

Protocol baseline:

- [NIP-60 Cashu Wallets](https://github.com/nostr-protocol/nips/blob/master/60.md)
  defines wallet configuration `kind:17375`, token events `kind:7375`,
  optional history `kind:7376`, optional quote tracking `kind:7374`, NIP-44
  encrypted wallet state, and token rollover by NIP-09 deletion.
- [NIP-61 Nutzaps](https://github.com/nostr-protocol/nips/blob/master/61.md)
  defines nutzap info `kind:10019`, nutzap events `kind:9321`, P2PK Cashu
  proofs, accepted mint and relay advertisement, redemption history, and
  observer validation.
- [NUT-11 P2PK](https://github.com/cashubtc/nuts/blob/main/11.md) and
  [NUT-12 DLEQ](https://github.com/cashubtc/nuts/blob/main/12.md) are required
  for safe nutzap send/receive defaults. Unsupported mints fail closed for
  NIP-61.

## Decision Summary

The first wallet milestone is a real kernel-integrated Cashu/nutzap product
surface, not a resurrected proof of concept. It ships the smallest complete
loop: create or recover a NIP-60 wallet, publish `kind:10019`, show bounded
wallet state, receive/redeem nutzaps, and send nutzaps to a profile or event.

NWC consolidation is a prerequisite architecture change, not the first product
surface. NIP-47 remains the BOLT-11 payment backend that can satisfy NIP-57
zaps. NIP-60 becomes a second backend that can hold ecash, send nutzaps, redeem
nutzaps, and later melt to BOLT-11.

`crates/nmp-nip60` returns to ordinary workspace and CI coverage at the start
of activation, before any public product claim. That reactivation must remove
or gate false surfaces such as unsupported `pay_invoice` stubs. An operation
the backend cannot do is represented as an absent capability in Rust-owned
state, not as a user-discoverable action that fails at runtime. (Phase 0 of
this reactivation has landed: #2866, closing #2865; post-merge follow-ups are
tracked in #2870 and referenced where relevant below.)

The closed #1508 demo is folded into the real wallet milestone. If a gallery or
developer demo is needed, it must exercise the same Rust actions, projections,
mint-port fixtures, and publish path as production. `apps/nmp-wallet-poc` stays
deleted.

## Ownership Model

### New Wallet Composition Crate

Add a reusable Layer-4 composition crate, `nmp-wallet`. Per
[`crate-boundaries.md`](crate-boundaries.md) §9/§10a, "composition root" names
an app/runtime installer that wires the substrate and protocol crates a
running app needs (`nmp-substrate`, app/runtime builders, and the
`nmp-browser-runtime` platform-adapter exception) — not a Layer-4
protocol/product crate. `nmp-wallet` is a Layer-4 composition crate that
assembles lower-level protocol crates, the same way `nmp-note-feed` composes
`nmp-nip01`, `nmp-nip18`, `nmp-content`, and `nmp-feed` into one reusable feed
surface (crate-boundaries §8). Its scope is bounded to wallet backend
selection and journaling, not an open-ended "everything wallet" bucket — the
same distinction crate-boundaries §8 draws between a legitimate composition
crate and the forbidden central-relations "reusable framework bucket".

`nmp-wallet` owns:

- the app-facing wallet action namespaces;
- the wallet typed projection under the `"wallet"` key;
- backend selection policy: which configured `WalletBackend` handles an
  intent, including which backend the substrate `PaymentPort` routes to for
  NIP-57 zaps (`nmp-wallet` does not own or reassign `PaymentPort` itself —
  see Existing NIP Crates below);
- the durable wallet operation journal;
- the Rust-owned `WalletBackend` seam;
- registration of wallet read interests and relay-text/event observers.

`nmp-wallet` may depend on `nmp-nip47`, `nmp-nip60`, `nmp-nip57` only through
the explicit composition surfaces it needs. The `nmp-nip57` dependency is
concrete, not speculative: at composition time `nmp-wallet` calls
`nmp_nip57::Config::with_payment_port` with an `Arc<dyn PaymentPort>` for the
selected backend (`nmp-nip47`'s `WalletPaymentPort` today; a future Cashu-melt
implementation once `pay_bolt11` is proven) to wire NIP-57 zaps to the active
wallet. `nmp-core` must not learn Cashu, nutzap, NWC, NIP-60, NIP-61, or mint
nouns.

### Existing NIP Crates

`nmp-nip60` owns reusable NIP-60/NIP-61 mechanics:

- event codecs for `kind:17375`, `kind:7375`, `kind:7376`, `kind:7374`,
  `kind:10019`, and `kind:9321`;
- Cashu proof, DLEQ, P2PK, token rollover, and mint request/response types;
- pure validation of NIP-60/NIP-61 event shape;
- a Cashu backend adapter for the `nmp-wallet::WalletBackend` seam.

It does not own relay sockets, app product policy, UI projection keys, backend
selection, or a private operation queue.

`nmp-nwc` owns the pure NWC protocol codec: connection-URI parsing, NIP-04/
NIP-44 request/response encryption, and the `kind:23194`/`kind:23195` event
shapes. `nmp-nip47` owns the NWC actor-side runtime, its `nmp.wallet.*` action
surface, and — per crate-boundaries §8 — the `PaymentPort` implementation
(`WalletPaymentPort`) injected into the zap chain at composition time. This
design does not reassign `PaymentPort` ownership away from `nmp-nip47`;
`nmp-wallet` only selects which backend that port routes to when NWC is one of
several configured backends. When composed through `nmp-wallet`, `nmp-nip47`'s
current `"wallet"` projection becomes backend-internal state. Standalone
NWC-only composition may keep the existing projection until the migration
removes that compatibility path.

`nmp-nip57` remains wallet-agnostic. It emits `PaymentIntent` through the
substrate `PaymentPort`; it does not depend on `nmp-nip47` or `nmp-nip60`.
`nmp-wallet` supplies the selected backend's `Arc<dyn PaymentPort>`
implementation to `nmp-nip57`'s composition entry point.

### Runtime And Shell

Native and browser shells render `nmp-wallet` projections and dispatch typed
actions only. They do not choose mints, relays, backend fallback, redemption
policy, proof validation, retry policy, or token rollover behavior.

Mint HTTP is a Rust-owned capability lane. Native runtimes can execute it in a
Rust worker. Browser runtimes can execute fetch as a raw capability result, but
Rust still owns request construction, response validation, retry, proof state,
and terminal status.

## WalletBackend Seam

`WalletBackend` lives in `nmp-wallet`, not `nmp-core` and not `nmp-nip60`.
It is actor-owned and command-shaped. It should not expose blocking methods to
UI callers or return success across FFI.

Conceptual shape:

```rust
pub trait WalletBackend: Send + Sync {
    fn id(&self) -> WalletBackendId;
    fn capabilities(&self) -> WalletCapabilities;
    fn snapshot(&self, scope: WalletProjectionScope) -> WalletBackendSnapshot;

    fn start_intent(
        &self,
        ctx: WalletBackendContext<'_>,
        intent: WalletIntent,
        correlation_id: Option<String>,
    ) -> Vec<ActorCommand>;

    fn on_wallet_event(
        &self,
        ctx: WalletBackendContext<'_>,
        event: &KernelEvent,
    ) -> Vec<ActorCommand>;

    fn on_mint_result(
        &self,
        ctx: WalletBackendContext<'_>,
        result: MintResult,
    ) -> Vec<ActorCommand>;
}
```

The actual trait can be split by file or sub-trait for size, but the ownership
is one seam: `nmp-wallet` decides which backend receives an intent and how
terminal status is represented.

Capability flags include:

- `pay_bolt11`;
- `create_cashu_wallet`;
- `publish_nutzap_info`;
- `send_nutzap`;
- `redeem_nutzap`;
- `deposit_cashu`;
- `melt_cashu`;
- `observe_nutzap_receipts`.

NIP-47 initially implements `pay_bolt11`. NIP-60 initially implements Cashu
wallet, nutzap send, and nutzap redeem. Cashu melt can later implement
`pay_bolt11` once NUT-05 support and double-payment reconciliation are proven.

## Product Surface

The app-facing action surface is owned by `nmp-wallet`:

- `nmp.wallet.select_backend`;
- `nmp.wallet.nwc.connect`;
- `nmp.wallet.nwc.disconnect`;
- `nmp.wallet.pay_invoice`;
- `nmp.wallet.cashu.create`;
- `nmp.wallet.cashu.recover`;
- `nmp.wallet.cashu.deposit_quote`;
- `nmp.wallet.cashu.complete_deposit`;
- `nmp.wallet.nutzap.publish_info`;
- `nmp.wallet.nutzap.send`;
- `nmp.wallet.nutzap.redeem`.

Existing `nmp.wallet.connect`, `nmp.wallet.disconnect`, and
`nmp.wallet.pay_invoice` from `nmp-nip47` can remain as compatibility aliases
only while migration is in progress. The canonical namespaces should make the
backend explicit where an action is backend-specific.

The `"wallet"` typed projection is bounded and screen-shaped:

- active backend id and connection/readiness state;
- backend capability flags;
- balances by unit and mint, aggregated without proofs;
- public Cashu P2PK pubkey for NIP-61;
- accepted nutzap mints and relay count;
- pending operation summaries keyed by correlation id;
- recent history rows for the open wallet view only;
- received nutzap candidates for the open receive view only.

The projection never contains wallet private keys, Cashu proofs, proof secrets,
quote ids, NWC secrets, bearer tokens, plaintext NIP-44 payloads, raw mint
responses, or unbounded event history.

## Relay Acquisition

Relay selection remains Rust-owned and route-provenanced.

For the active user's own wallet:

1. Fetch the active user's `kind:10019` from the normal self-event startup path.
2. Use its `relay` tags as the wallet/nutzap relay set for `kind:17375`,
   `kind:7375`, `kind:7376`, and received `kind:9321`.
3. If no usable `kind:10019` exists, fall back to the active user's NIP-65
   relay list, matching NIP-60 guidance.
4. Never use a native-provided or app-provided manual relay list as the default
   wallet path.

For sending a nutzap:

1. Resolve the recipient's `kind:10019` through kernel reads.
2. Use only mints listed by that event and the exact mint URL from its `mint`
   tag in the outgoing `u` tag.
3. Publish `kind:9321` to the relays listed by the recipient's `kind:10019`.
4. If the recipient has no trusted mint, no P2PK pubkey, or no reachable
   nutzap relay set, fail closed in action state.

For receiving nutzaps:

1. Subscribe with `kinds:[9321]`, `#p:[active_pubkey]`, and `#u` limited to the
   wallet's accepted mint URLs.
2. Use the latest local `kind:7376` redeemed marker as a lower bound when
   possible, but do not make it the only correctness marker. The local operation
   journal and redeemed event ids remain the source of retry safety.
3. Redeem only events whose mint and P2PK lock match the active `kind:10019`.

`nmp-nip60` is an active workspace member (Phase 0, #2866, closing #2865).
`relay` tags on `kind:17375` are decoded into `legacy_relay_hint`, a field
named and documented as a non-authoritative compatibility hint — it must never
become the relay-selection source of truth. `kind:10019` `relay` tags, with
NIP-65 fallback, remain the only authoritative relay set, and that resolution
policy belongs to `nmp-wallet` (Phase 1), not `nmp-nip60`. This boundary is not
yet fully closed: post-merge review (#2870) found that `nmp-nip60`'s
`publish_nutzap_info` helper currently seeds a `kind:10019`'s `relay` tags
directly from `legacy_relay_hint` — dormant today (zero callers) but the one
path where the hint would become truth if left unfixed. That fix must land
before `nmp-wallet` gives `publish_nutzap_info` its first caller.

## State Machine

`nmp-wallet` owns a durable operation journal. It is required because mint
round-trips and relay publication can succeed independently, and process death
must not create double-spend or double-pay behavior.

Operation states:

- `Draft`: input accepted, no external request issued.
- `Prepared`: proofs or quotes selected and locked locally.
- `MintPending`: an HTTP request to a mint is in flight.
- `MintSettled`: mint response succeeded and Nostr events must be published.
- `PublishPending`: signed Nostr events are queued through the publish engine.
- `Settled`: token state and history are reflected locally.
- `Unknown`: process or transport interrupted after an external side effect;
  reconcile before retry.
- `Failed`: no external value moved, or reconciliation proved no value moved.

Before any mint request that can spend or create value, the operation journal
records the inputs being consumed. On restart, the wallet reconciles pending
operations by checking proof state at the relevant mint, then either publishes
the missing NIP-60 events or unlocks the local selection.

Reducers do not await mint HTTP. They emit commands, and mint workers return
raw results. Rust maps those results into wallet state, action stages, and
publish commands.

## Three Wallet-State Concerns

Wallet state is chaotic to reason about: NIP-09 deletions arrive, mint
reconciliation probes report that a proof was already spent, new `kind:7375`
token events land, nutzaps are redeemed — all interleaved and out of order. To
keep this tractable, `nmp-wallet` separates three concerns that must never be
conflated. They are distinguished by their **write moment**, not their lifetime.

| Concern | Write moment | Question it answers | Storage |
|---|---|---|---|
| Money-safety saga | pre-effect | "did my mint spend happen before I crashed?" | durable, persisted, at-most-once |
| Derived state | fold result | "what is my proof set / balance right now?" | in-memory, rebuildable |
| Causal trail | post-observation | "why is the state the shape it is right now?" | in-memory ring, over a durable `kind:7376` tier |

The saga (the state machine above) writes **pre-effect** — "about to consume
proofs A,B for a mint spend." That pre-record *is* the at-most-once mechanism.
The causal trail writes **post-observation** — facts about what arrived,
settled, or reconciled. Derived state is the fold over post-observation facts.

The trail cannot be read out of the saga: the saga only knows *locally
initiated* operations, while most of what makes wallet state chaotic — inbound
token arrivals, NIP-09 deletions from the user's *other* devices, incoming
nutzaps — never touches the saga. The relationship is **producer/consumer**: the
saga emits facts into the trail (`MintSettled`, `Unknown → reconciled`,
`Failed`) so the trail can also explain locally caused shape changes. Their
schemas never merge — money-critical pre-effect records must not live in a
diagnostic log with bounded eviction.

## Event-Sourced Reducer And Causal Trail

`nmp-wallet` computes wallet state as a fold over an ordered stream of typed
`WalletFact`s, each carrying its cause and provenance:

```rust
enum WalletFact {
    TokenAdded     { token_event: EventId, amount: Msat, mint: MintUrl, via: Provenance },
    TokenDeleted   { token_event: EventId, cause: DeleteCause },   // NIP-09 in, or local rollover
    MintProbed     { proof: ProofRef, verdict: ProofVerdict },     // Spent / Unspent / Unknown
    NutzapRedeemed { nutzap: EventId, amount: Msat, sender: PubkeyRef },
    SagaTransition { op: CorrelationId, from: OpState, to: OpState }, // producer wiring from the saga
    StateRebuilt   { from: Vec<EventId> },                          // genesis fact after restart
}

enum Provenance  { Relay(RelayRef), Saga(CorrelationId), MintRollover }
enum DeleteCause { Nip09Delete { by: EventId }, LocalRollover { op: CorrelationId } }
```

Two views are maintained over the same fact stream:

- a **time-ordered bounded delta ring** — answers "what sequence of events
  produced this shape?";
- a **per-atom last-cause index** (`token_event_id → last WalletFact that
  touched it`) — answers "why is *this specific* proof/token here?". This index
  is `O(current state)`, not `O(traffic)`, so a nutzap flood cannot evict the
  cause of a token the user still holds.

Four invariants keep the reducer honest:

1. **Confluence.** The reducer's terminal state must be order-insensitive even
   though the trail is order-sensitive. A NIP-09 delete arriving before the
   `kind:7375` it deletes must tombstone, not no-op; otherwise two devices show
   different balances and the trail "explains" a state that should not exist.
2. **The ring is never a rebuild authority.** Restart rebuilds state from Nostr
   events plus saga reconciliation, entering the trail as a `StateRebuilt`
   genesis fact — named explicitly so restart is never later "optimized" into
   ring replay.
3. **Derived state rides Trellis only at the acquisition layer** (which
   relays/filters/interests, per the `kind:10019` relay rules). Proof-set
   derivation is Nostr/product meaning and stays actor-owned in `nmp-wallet`,
   never in Trellis core (ADR-0075 Ownership). A causal timeline is a different
   primitive from Trellis's dependency graph, and ADR-0075 confines Trellis
   trace to dev-only tooling — so the trail is `nmp-wallet`'s own construct.
4. **Privacy at the type level.** `WalletFact` payloads carry only event ids, op
   ids, amounts-by-unit, and canonical mint URLs (and the latter only when the
   URL is already public via `kind:10019`) — never proofs, proof secrets, or
   keys. This is a property of the types, not a display-edge filter.

### Durable Tier: `kind:7376`, Not A Local Trail

The causal trail does not get its own durable local store. The durable causal
record is the protocol's own: NIP-60 `kind:7376` history events (already
required by this design for every balance-changing operation; codec in
`crates/nmp-nip60/src/history_event.rs`) plus the `del` field on token events.
On restart, the wallet folds `kind:7376` into coarse pre-session facts, and the
in-memory ring then accumulates fine-grained facts from there — two resolutions
in one timeline. The in-memory ring is only the session-local high-resolution
overlay (mint-probe verdicts, arrival provenance, saga correlation ids) too
transient or too fine for `kind:7376`.

## NIP-60 Event Rules

Wallet configuration:

- Build `kind:17375` as NIP-44 encrypted content containing the Cashu wallet
  private key and one or more accepted mints.
- The Cashu private key is not the user's Nostr key and never leaves Rust-owned
  wallet state.
- Signing/decryption must be signer-transparent. Local keys can run NIP-44 in
  Rust. NIP-46/NIP-07 paths require signer NIP-44 capability; otherwise Cashu
  wallet activation fails as state.

Token events:

- `kind:7375` stores unspent proofs grouped by mint and unit.
- Spending any proof from a token event creates replacement token events for
  unspent proofs and change, then NIP-09 deletes the consumed token event.
- The delete event must include `["k","7375"]`.
- The `del` field records destroyed token event ids for wallet transition
  audit, but the local journal remains the retry authority.

History:

- `kind:7376` is optional in the NIP but required by NMP wallet actions for any
  balance-changing operation.
- Created and destroyed token ids live in encrypted content.
- Redeemed nutzap `e` tags stay public as NIP-61 requires.
- No history projection is unbounded across FFI.

Quotes:

- `kind:7374` quote tracking is optional and should be used only when local
  state is insufficient for cross-device continuity.
- Quote events use NIP-40 expiration. Kernel time decides expiration, not native
  wall-clock reads.

## NIP-61 Event Rules

`kind:10019` is the public receive policy:

- `relay` tags are where senders publish nutzaps.
- `mint` tags are the only mints senders may use, including supported units.
- `pubkey` is the Cashu P2PK public key and must not be the user's main Nostr
  public key.

`kind:9321` send:

- proof tags contain P2PK-locked proofs with DLEQ data;
- `u` uses the exact recipient-listed mint URL;
- `p` tags the recipient's Nostr pubkey;
- `e` and `k` identify the zapped target when present;
- content is an optional public comment and must not carry secrets.

Receiving:

- verify that the event is p-tagged to the active user;
- verify that the `u` mint is accepted by the active `kind:10019`;
- verify that each P2PK secret locks to the active Cashu P2PK pubkey;
- verify DLEQ before counting or presenting the nutzap as valid;
- swap proofs into fresh wallet-owned proofs before marking redeemed;
- publish `kind:7376` with redeemed `e` tags and sender `p` tags.

Observer counting:

- counts for another user's event must be derived from public data only:
  receiver `kind:10019`, mint keysets, P2PK lock, `u` tag, and DLEQ proof.
- unverifiable nutzaps may be shown as rejected/ignored state, not counted as
  value.

## Privacy And Security

The wallet runtime treats Cashu proofs and private keys as secret material.

- No secret material enters action namespaces, action tags, logs, diagnostics,
  snapshots, or PR/debug fixture prose.
- Log surfaces use operation ids, event ids, mint host hashes or canonicalized
  URLs only when the URL is already public via `kind:10019`.
- Mint HTTP responses are raw worker results until Rust validates them.
- Unknown recipient inbox, unknown accepted mint, unsupported NUT-11, missing
  DLEQ, or mismatched P2PK lock fails closed.
- Received nutzap proofs are never republished to relays. Only redemption
  history and token rollover events leave the wallet.

## Tests And Gates

Activation requires:

- `cargo test -p nmp-nip60` clean for any `nmp-nip60` change (the crate is an
  active workspace/CI member as of Phase 0, #2866);
- `cargo test -p nmp-wallet` for backend selection, journal transitions,
  projection bounds, and compatibility aliases;
- NIP-60 event tests for wallet, token, history, quote, NIP-09 delete tags, and
  signer-transparent NIP-44 paths;
- NIP-61 event tests for `kind:10019`, `kind:9321`, accepted mint filtering,
  P2PK lock validation, DLEQ validation, and redeemed history tags;
- integration tests with a mock mint and mock relay transport proving no
  private socket path exists;
- restart/reconcile tests for `MintPending`, `MintSettled`, and
  `PublishPending`;
- NIP-57 integration tests proving zaps pay through the selected backend via
  `PaymentPort`, with no `nmp-nip57 -> nmp-nip47` or
  `nmp-nip57 -> nmp-nip60` dependency;
- browser signer capability tests proving NIP-60 is disabled when NIP-44
  encrypt/decrypt is unavailable;
- `cargo test -p nmp-testing --test doctrine_lint_smoke`;
- `cargo build --workspace` in the PR that re-adds wallet crates or changes
  workspace dependencies.

## Non-Goals

This design does not claim:

- a custodial wallet;
- mint recommendation or scoring policy;
- a complete Lightning wallet;
- full NIP-57 product support;
- native-owned wallet state;
- a standalone wallet proof-of-concept app;
- browser Cashu support without a validated mint HTTP capability and NIP-44
  signer capability.
