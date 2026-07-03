# 29 — Building a NIP-60/61 wallet app on NMP

> **Status: LANDED (spine only) · Audience: builders.** Doctrine: **D0**
> (kernel stays product-ignorant), **D4** (single writer), **D5** (bounded
> state), **D6** (no error types across FFI), **D7** (capabilities report,
> native never decides policy).

This section teaches the shape of a NIP-60 (Cashu ecash wallet) / NIP-61
(nutzap) app on NMP: which actions you dispatch, what the bounded `"wallet"`
projection looks like, how capability-gated UI works, and why relay selection
is never your app's job. The design of record is
[`docs/architecture/nip60-nip61-wallet-design.md`](../architecture/nip60-nip61-wallet-design.md)
(cited below as "the design doc"); this section restates it in builder-guide
form and does not relitigate it. Track activation status in GitHub epic
[#2864](https://github.com/pablof7z/nostr-multi-platform/issues/2864) and
milestone issue [#2872](https://github.com/pablof7z/nostr-multi-platform/issues/2872).

**Read this before trusting a code block below.** `nmp-wallet` exists in the
workspace today (`crates/nmp-wallet`) and `nmp-nip60` is an active
workspace/CI member again (epic Phase 0, [#2866](https://github.com/pablof7z/nostr-multi-platform/pull/2866)),
but `nmp-wallet` is a **private, unwired spine**: canonical action-name
constants, capability flags, the `WalletProjection` type, and the durable
operation journal all exist as real Rust code, but no `WalletBackend` is
registered, no `ActionModule` dispatches a Cashu/nutzap action yet, and no app
or runtime composes the crate. The support matrix in
[`../nips.md`](../nips.md) records this precisely: no product/UI surface
exists yet. See "What's validated today vs. pending" below for the exact line
between running code and design intent — and re-check `../nips.md` before you
build against any row here, since this is an actively-landing epic.

This section's first reader is
[nutsack](https://github.com/pablof7z/nutsack), an external thin-shell TUI
proof-of-concept that dispatches exactly the actions and renders exactly the
projection described here, gated behind a cargo feature until the backend
lands. If you are building a wallet app, mirror nutsack's crate split
(`<app>-core` owns action builders + a projection mirror + composition; the
shell renders and dispatches only) rather than inventing your own.

## The product surface: one namespace, pluggable backend

A wallet app dispatches `nmp.wallet.*` actions and renders one `"wallet"`
projection. It never learns whether the active backend is NIP-47 (Lightning
via NWC) or NIP-60 (Cashu ecash) — that is a `WalletBackend` selection Rust
owns, not a fact your UI branches on. This is the same "backend is invisible
to the caller" pattern the publish engine uses for signer choice (see
[12 — Publishing + the publish engine](12-publish-and-ledger.md#choosing-the-signer--typed-provenance)):
the app dispatches a typed intent; Rust resolves who actually executes it.

Concretely, `nmp-wallet` (`crates/nmp-wallet/src/lib.rs`) owns:

- the app-facing `nmp.wallet.*` action namespace (name constants only today —
  see below);
- the bounded `"wallet"` projection type under the `"wallet"` key
  (`crates/nmp-wallet/src/projection.rs`);
- backend selection policy and the `WalletBackend` seam
  (`crates/nmp-wallet/src/backend.rs`);
- the durable wallet operation journal (`crates/nmp-wallet/src/journal/`);
- which backend's `PaymentPort` NIP-57 pays through — it does not own or
  reassign `PaymentPort` itself (see below).

`nmp-nip60` owns pure NIP-60/NIP-61 mechanics only — event codecs, Cashu
proof/DLEQ/P2PK math, token rollover — with zero relay I/O and zero app
policy. `nmp-nip47` owns NWC mechanics, the NWC actor runtime, and the
`WalletPaymentPort` `PaymentPort` implementation. Today `nmp-nip47` also owns
the *only* real `ActionModule`s under the `nmp.wallet.*` namespace
(`connect`/`disconnect`/`pay_invoice`); `nmp-wallet` declares those names as
its own via `nmp-ownership` claims but does not yet implement any dispatch
path for them. Neither NIP crate — nor `nmp-wallet` itself, yet — is
composed into a running app.

## Dispatch: `nmp.wallet.*` actions

The canonical action namespaces (`crates/nmp-wallet/src/lib.rs`
`ACTION_*` constants, matching design doc §"Product Surface"). There is
**exactly one name per action** — no canonical/legacy pairs:

| Action | Backend | Status |
|---|---|---|
| `nmp.wallet.select_backend` | either | name constant only; no dispatcher |
| `nmp.wallet.connect` | NIP-47 | **SHIPS today**, dispatched by `nmp-nip47` (`crates/nmp-nip47/src/action/connect.rs`) |
| `nmp.wallet.disconnect` | NIP-47 | **SHIPS today**, dispatched by `nmp-nip47` |
| `nmp.wallet.pay_invoice` | NIP-47 | **SHIPS today**, dispatched by `nmp-nip47` (`crates/nmp-nip47/src/action/mod.rs`) |
| `nmp.wallet.cashu.create` | NIP-60 | name constant only; no dispatcher |
| `nmp.wallet.cashu.recover` | NIP-60 | name constant only; no dispatcher |
| `nmp.wallet.cashu.deposit_quote` | NIP-60 | name constant only; no dispatcher |
| `nmp.wallet.cashu.complete_deposit` | NIP-60 | name constant only; no dispatcher |
| `nmp.wallet.nutzap.publish_info` | NIP-60/61 | name constant only; no dispatcher |
| `nmp.wallet.nutzap.send` | NIP-60/61 | name constant only; no dispatcher |
| `nmp.wallet.nutzap.redeem` | NIP-60/61 | name constant only; no dispatcher |

`nmp.wallet.connect`/`disconnect`/`pay_invoice` are the permanent canonical
names for NWC, not a placeholder pending a `nmp.wallet.nwc.*` rename. A
backend-qualified rename is scoped as epic #2864 Phase 2 (NWC consolidation)
and, if it happens, ships as a **single-PR hard-break** — move `nmp-nip47`'s
`ActionModule` and wire schema, update every caller in the same change. There
is never an alias period; do not invent an `nmp.wallet.nwc.*` name yourself in
the meantime.

Dispatch, once a Cashu/nutzap `ActionModule` exists, follows the same pattern
every other builder-guide section uses — an unsigned, typed, correlation-id'd
intent, never a direct call into mint HTTP or a relay socket:

```rust
// Illustrative — no ActionModule dispatches this action yet. Name/shape per
// the design doc; no proof, key, or quote id ever appears in a dispatch
// payload.
app.dispatch_action(
    nmp_wallet::ACTION_CASHU_DEPOSIT_QUOTE,
    CashuDepositQuote { mint_url, amount_sats, correlation_id },
);
```

Every mint round-trip and Nostr publish this triggers is meant to be
reconciled through the durable operation journal
(`crates/nmp-wallet/src/journal/saga.rs`) — `Draft → Prepared → MintPending →
MintSettled → PublishPending → Settled`, with `Unknown`/`Failed` terminal
states for interrupted operations (design doc §"State Machine"). Your app
never awaits a mint call directly; it dispatches, then reads the projection's
pending-operation row keyed by `correlation_id`. Agents extending `nmp-wallet`
should also read the design doc's "Three Wallet-State Concerns" section: the
pre-effect saga, the fold-derived balance/proof-set state, and the
post-observation causal trail (`crates/nmp-wallet/src/journal/{saga,ledger,trail,fact}.rs`)
are three schemas that must never merge — that split is why a crash mid-mint
can never double-spend or silently lose an operation.

## Render: the bounded `"wallet"` projection

The `"wallet"` key is screen-shaped and bounded (D5), never an unbounded
ledger. `WalletProjection` (`crates/nmp-wallet/src/projection.rs`) is real
code today:

- `active_backend_id: Option<WalletBackendId>`, `readiness: WalletReadiness`
  (`NotConfigured`/`Activating`/`Ready`/`Degraded`);
- `capabilities: WalletCapabilities` (see below);
- `balances: Vec<WalletBalanceRow>` — mint + unit + amount, no proofs;
- `cashu_p2pk_pubkey: Option<String>` — the NIP-61 receive identity, distinct
  from the Nostr pubkey;
- `accepted_mint_count` / `accepted_relay_count`;
- `pending_operations: Vec<WalletOperation>`, keyed by correlation id;
- `recent_history: Vec<WalletHistoryRow>` and `receive_rows: Vec<WalletReceiveRow>`,
  both capped to `MAX_WALLET_PROJECTION_ROWS = 100` by `with_recent_history`/
  `with_receive_rows` (oldest rows drop first).

It **never** carries wallet private keys, Cashu proofs, proof secrets, quote
ids, NWC secrets, bearer tokens, plaintext NIP-44 payloads, raw mint HTTP
responses, or unbounded event history — `projection.rs`'s own test asserts the
serialized JSON never contains a `"proof"`/`"secret"`/`"quote_id"`/`"nsec"`/
`"plaintext"` marker. Treat that list as load-bearing, not aspirational; it is
the same secret-material boundary [16 — Capabilities](16-capabilities.md)
enforces for the request/result types that cross the capability bridge.

`WalletProjection` is not registered as a snapshot projection anywhere yet —
no `register_projection("wallet", ...)` call composes it into a running app.
What actually ships under the `"wallet"` key today is still `nmp-nip47`'s
older, narrower `WalletStatus` (`crates/nmp-nip47/src/status.rs`): NWC
connection `status`, `relay_url`, `wallet_pubkey_hex`,
`balance_msats`/`balance_sats`, and a heartbeat-derived `connection_state`.
It follows the same raw-data doctrine `WalletProjection` does (no
pre-rendered label, tone, or bech32 encoding — the shell derives those), but
it has no backend id, no capability flags, no pending-operation rows, and no
Cashu fields. When `nmp-wallet` is composed, `nmp-nip47`'s projection becomes
backend-internal state and `WalletProjection` takes over the `"wallet"` key;
until then, do not assume `WalletStatus` is the final shape.

## Capability-gated UI (absent capability ≠ failing button)

This is the load-bearing UX rule from the design doc and nutsack's spec alike:
**a control the active backend cannot do is greyed out from an absent
capability flag, never a button that dispatches and then fails at runtime.**
This is [16 — Capabilities](16-capabilities.md)'s "decides vs. reports"
principle applied to the wallet: the projection reports which operations the
active backend supports as data; your UI reads that data to decide what to
render enabled, never to decide policy itself. `nmp-wallet` enforces the
mapping in code, not just prose: `WalletCapabilities::action_namespaces()`
(`crates/nmp-wallet/src/capability.rs`) returns only the action names a set of
capability flags unlocks, and its own test
(`absent_capability_means_absent_user_action`) asserts an NWC-only capability
set never yields `ACTION_NUTZAP_SEND`.

Capability flags (`WalletCapabilities`, `crates/nmp-wallet/src/capability.rs`):

| Flag | NIP-47 (NWC) | NIP-60 (Cashu) |
|---|---|---|
| `pay_bolt11` | yes | not yet (future: Cashu melt via NUT-05) |
| `create_cashu_wallet` | no | yes |
| `publish_nutzap_info` | no | yes |
| `send_nutzap` | no | yes |
| `redeem_nutzap` | no | yes |
| `deposit_cashu` | no | yes |
| `melt_cashu` | no | not yet |
| `observe_nutzap_receipts` | no | yes |

(`WalletCapabilities::nwc_payments()` and `::cashu_nutzaps()` are the two
constructors matching these columns today — no backend actually returns
either of them from a live `WalletBackend::capabilities()` yet, since no
backend is registered.)

A wallet home screen reads `capabilities` off the projection and renders each
action's control enabled/disabled from the matching flag — it does not probe
by calling the action and catching a failure, and it does not hardcode "NWC
backends can't send nutzaps" as app logic. The flag is the only fact the app
needs; which backend produces which flag value is Rust's to decide, and it can
change (Cashu melt filling in `pay_bolt11` later) without your UI code
changing.

## Relay acquisition is NMP-owned, not app-provided

Wallet/nutzap relay selection is Rust-owned and route-provenanced, same as
outbox routing for ordinary events (D3,
[10 — Outbox routing](10-outbox-routing.md)). Concretely (design doc
§"Relay Acquisition"):

- **Your own wallet's relays** come from your own `kind:10019` `relay` tags,
  fetched through the normal self-event startup path; if none exists yet, NMP
  falls back to your NIP-65 relay list. An app-provided manual relay list is
  never the default wallet path.
- **Sending a nutzap** resolves the *recipient's* `kind:10019`: only their
  listed mints, the exact mint URL from their `mint` tag, and their listed
  relays. There is no "pick a relay" step in your app.
- **Receiving nutzaps** subscribes `kinds:[9321]`, `#p:[active_pubkey]`,
  `#u` limited to your wallet's accepted mints; the local operation journal
  and redeemed event ids — not wall-clock native state — are the retry-safety
  source of truth.

A bootstrap relay list (to publish your very first `kind:10019` before you
have one) is the one legitimate app-level relay concern, and even then it is
config, not policy: it is never substituted for the `kind:10019`/NIP-65
resolution above once a receive policy exists.

`WalletConfig::legacy_relay_hint` (`crates/nmp-nip60/src/wallet_event.rs`) is
a read-only, non-authoritative field decoded from a legacy `kind:17375`
`relay` tag — it exists for backward compatibility only. `kind:17375` never
emits `relay` tags on write, and no code path feeds `legacy_relay_hint` back
into a real relay decision; `kind:10019` (with NIP-65 fallback) remains the
only authoritative wallet relay source. If you find yourself reading
`legacy_relay_hint` to pick a relay, stop — that is the exact bug class this
field's name exists to flag.

## Fail-closed rules

Every one of these is represented as **absent-capability or rejected-action
state**, never a crash, a silent no-op, or a false success:

- unsupported mint (not listed in the recipient's `kind:10019`, or the local
  wallet's accepted-mint set);
- missing/invalid P2PK lock or missing DLEQ proof on a received nutzap —
  shown as a rejected candidate, not counted as balance;
- no NIP-44 signer capability — Cashu wallet activation fails as state
  (`kind:17375` requires NIP-44 encryption; NIP-46/NIP-07 signers without
  NIP-44 support cannot activate a Cashu backend, and browser hosts without a
  validated NIP-44 capability do not get Cashu enabled at all);
- recipient has no trusted mint, no P2PK pubkey, or no reachable nutzap relay
  set — the send action fails closed rather than guessing a fallback;
- an interrupted mint/publish sequence reconciles to `Unknown` and is resolved
  by checking proof state at the mint before any retry, never re-spent blind.

## Worked example: nutsack's flow

nutsack (external, thin-shell, zero wallet logic) drives exactly this action
sequence for its MVP loop once the backend lands:

1. **Login** — an nsec goes to NMP's signer composition; the app never holds
   wallet-policy key material.
2. **Create wallet** — `select_backend(Cashu)` → `cashu.create([mint_url])` →
   `nutzap.publish_info`. NMP generates the Cashu P2PK key (distinct from the
   Nostr key), NIP-44-encrypts `kind:17375`, and publishes `kind:10019`.
3. **Deposit** — `cashu.deposit_quote(mint_url, amount_sats)` →
   `cashu.complete_deposit(correlation_id)`.
4. **Send a nutzap** — `nutzap.send(recipient_pubkey, amount_sats)`. NMP
   resolves the recipient's `kind:10019` and fails closed per the rules above.
5. **Receive/redeem** — the projection's `receive_rows` show
   verified/rejected candidates; `nutzap.redeem(event_id)` swaps into
   wallet-owned proofs and publishes `kind:7376`.
6. **View balance/history** — read `balances` and the bounded `recent_history`
   rows off the projection; nothing here is a separate query API.

Every step is dispatch-and-render. There is no step where the app constructs a
mint request, validates a DLEQ proof, or picks a relay — that would be exactly
the wallet logic the thin-shell rule forbids.

## What's validated today vs. pending

| Piece | Status | Where |
|---|---|---|
| `nmp.wallet.{connect,disconnect,pay_invoice}` (NWC), dispatchable today | **SHIPS** | `crates/nmp-nip47/src/action/` |
| `"wallet"` projection actually rendered today (NWC-only `WalletStatus`) | **SHIPS** | `crates/nmp-nip47/src/status.rs` |
| `nmp-wallet` crate: action-name constants, `WalletCapabilities`, `WalletProjection` type, operation journal (saga/ledger/trail) | **LANDED**, unwired | `crates/nmp-wallet/src` |
| NIP-60/61 event codecs, Cashu proof/DLEQ/P2PK math, token rollover, `legacy_relay_hint` compatibility field | **LANDED**, active workspace member | `crates/nmp-nip60` |
| `nmp.wallet.{cashu,nutzap}.*` actually dispatchable via a registered `ActionModule` | **PLANNED** | no crate yet |
| A `WalletBackend` impl (Cashu or NWC) actually registered/selectable | **PLANNED** | no crate yet — only a test-only `EmptyBackend` exists (`crates/nmp-wallet/src/backend.rs` tests) |
| `WalletProjection` registered as the live `"wallet"` snapshot | **PLANNED** | no composition exists yet |
| Any app/runtime composing `nmp-wallet` | **PLANNED** | no consumer in-tree |

Activation requires a `WalletBackend` implementation (Cashu, then NWC) wired
through `nmp-wallet`'s composition and a live snapshot registration — see the
design doc's "Tests And Gates" section for the full activation test list.
Until then, treat the action-name/projection-shape/capability-flag surface as
a stable contract you can build against (as nutsack does, behind a disabled
cargo feature), but treat end-to-end dispatch as **not yet real**.

## Anti-patterns

1. **Building the mint HTTP call, DLEQ check, or relay pick in app code.**
   That is exactly the wallet logic this design keeps in Rust. If nutsack's
   MVP needs something NMP doesn't expose, that is an NMP gap — file or
   comment on the epic; do not implement it in the app.
2. **Rendering a button that dispatches and fails instead of reading the
   capability flag.** The projection reports capability as data; the UI
   decides layout from that data, never from a caught error.
3. **Treating an app-provided relay list, or `legacy_relay_hint`, as the
   wallet default.** The bootstrap-relay exception is for publishing your
   very first `kind:10019` only; every wallet/nutzap read after that resolves
   from `kind:10019`/NIP-65.
4. **Putting a proof, secret, quote id, or private key in an action payload,
   log line, or example.** None of those cross the action/projection
   boundary, ever — not even in a doc code sample.
5. **Inventing an `nmp.wallet.nwc.*` name for NWC actions.** The canonical
   names are `nmp.wallet.connect`/`disconnect`/`pay_invoice`, permanently,
   until (and unless) epic #2864 Phase 2 lands a single-PR hard-break. There
   is no alias period to build against early.
6. **Assuming `nmp-wallet`'s existence means a backend is live.** The crate
   compiling and its tests passing means the name/shape contract is stable
   enough to code a client against; it does not mean any action dispatches or
   any projection renders in a running app yet.

See also: [architecture: NIP-60/NIP-61 wallet design](../architecture/nip60-nip61-wallet-design.md) ·
[16 — Capabilities (D7)](16-capabilities.md) ·
[12 — Publishing + the publish engine](12-publish-and-ledger.md) ·
[11 — Sessions + signers + identity scopes](11-sessions-signers.md) ·
[10 — Outbox routing (NIP-65)](10-outbox-routing.md) ·
[03 — Doctrine D0–D10 end-to-end](03-doctrine-d0-d8.md) ·
[NIP support matrix](../nips.md).
