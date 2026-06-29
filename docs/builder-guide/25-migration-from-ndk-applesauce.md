# 25 — Migration — NDK / Applesauce → NMP

> Status: **SHIPS** · Audience: builders coming from NDK or Applesauce.
> Prereqs: `01-what-nmp-is.md`, `02-mental-model.md`.

## The one shift to internalize first

NDK and Applesauce are **JS libraries you call**. NMP is a **Rust kernel that
owns your app's state**. In NDK/Applesauce your code holds the EventStore,
wires subscriptions, and decides relays. In NMP the actor owns all of that;
your platform code only `dispatch(action)`s intents and renders the snapshot
it is handed. You are not porting code — you are deleting the layer you used
to write.

Critically, **Applesauce `model()` is not NMP's snapshot projection**. Applesauce
is RxJS observables in a browser; a model is an in-process stream you subscribe
to. NMP's equivalent is a **typed read-session helper** that owns demand, replay,
and scoped event delivery, backed by a registered snapshot projection
(`register_snapshot_projection` or `register_typed_snapshot_projection`)
that pushes a bounded JSON or typed sidecar to the host at ≤60 Hz.
They solve the same *problem* (typed derived views) with incompatible
*mechanics*. Treating them as the same API is the central migration mistake.

## Concept translation

| NDK term | Applesauce term | NMP term |
|---|---|---|
| `NDKRelaySet` / per-author relay calc | relay-map / `selectOptimalRelays` | `CompiledPlan` — the planner resolves relays from a `LogicalInterest`; you never assemble relay sets (`07-subscription-planner.md`) |
| `ndk.subscribe(filters, opts)` | `eventStore.timeline(filters)` | `open_feed(FeedParams)` / action-dispatched claim → Rust registers `LogicalInterest`s; you pass *intent*, not filters/relays |
| `NDKEvent` + manual derive | `eventStore.model(...)` (RxJS) | **typed read-session helper** + `register_snapshot_projection` — helper owns demand/replay/delivery; snapshot pushed as a JSON slice on every emit tick; **not** a stream you subscribe to |
| build event → `signer.sign` → `ndk.publish` | `ActionRunner` + `ctx.publish(event, relays?)` | `ActionModule` + the publish engine — one action signs, publishes (outbox-routed), and updates the store atomically |
| `NDKPrivateKeySigner` / `NDKNip46Signer` / NIP-55 | `SimpleSigner` / `ExtensionSigner` / `AmberClipboardSigner` | `nmp-signers::Signer` (Local / NIP-46 / NIP-07) + Keyring capability; browser NIP-44 is capability-specific, not implied by every browser signer; iOS Keychain SHIPS, iOS external-signer is a capability hook not turnkey |
| `@nostr-dev-kit/sessions` store + `activePubkey` | `AccountManager` + `IAccount` | kernel `AppState.session` + `nmp-signers::AccountManager`; account is identity-only, derived state lives in app-owned stores |
| kind:3 watcher in sessions pkg + Svelte runes / React deps to rewire | consumer manually re-subscribes | **auto-rewire** — Rust owns source reduction and dependent-interest replacement; app dispatches **zero** rewire code. (`ReducedSource` is internal compiler vocabulary; not an app-facing noun.) |

## What NMP handles for you

Each item below is code you wrote in NDK/Applesauce that NMP **owns**:

- **Outbox routing.** No `calculateRelaySetFromEvent`, no passing `relays?`
  to publish. The planner resolves author write-relays + recipient
  inbox-relays automatically; manual relay selection is an audited opt-out.
- **Dynamic-source auto-rewire.** In NDK you needed Svelte runes or a React
  `[follows]` dep; in NMP, Rust source-reduction machinery replaces the
  materialized child interests when the source changes, with zero app code.
- **Subscription coalescing + lifecycle.** Applesauce dedupes by model hash
  but still sends one REQ per filter; NMP coalesces overlapping interests
  into minimal wire REQs and auto-closes on last consumer drop.
- **Durable outbox + sync coverage.** NDK's tracker is in-memory and
  re-fetched every cold start; NMP persists relay metadata and NIP-77
  coverage as durable domain state.
- **Replaceable-event supersession + provenance merge.** No manual
  "is this newer by `created_at`?" guards — the EventStore enforces it on
  insert.
- **Atomic write-then-store.** Actions publish *and* update local state as
  one operation; you cannot forget the local-state step.
- **Multi-account atomicity.** Switching account is one action; the kernel
  binds signer, rebuilds filters, and emits one snapshot — no
  logout/login/reload dance.

## What not to do

- **Do not 1:1 port NDK/Applesauce code.** The whole subscription/relay/store
  layer you wrote becomes `dispatch` calls. Porting it re-introduces the bug
  classes NMP exists to remove.
- **Do not treat Applesauce `model()` as an NMP snapshot projection.** RxJS-in-browser vs
  actor-owned-snapshot-over-FFI. Don't expect JS event-stream ergonomics
  (`.pipe`, `.subscribe`, hot observables) across the FFI boundary; you get
  a snapshot with a monotonic `rev` guard.
- **Do not import NDK relay-policy patterns.** No relay-set assembly, no
  per-call `relays` argument. Once the planner decides a relay is
  responsible for a subset of authors, that split is policy, not an
  app-overridable hint.
- **Do not reinvent Applesauce's `claimLatest` / refcount GC in app code.**
  The kernel's claim-based GC tracks which views reference which events and
  prunes automatically; an app-side parallel cache is a D4 violation.
- **Do not hand-roll dynamic source watching in SwiftUI/Compose.** Contact
  lists, mute lists, follow packs, and pointer cascades reduce in Rust and
  materialize normal interests through the registry/planner. Native only
  declares the feed/action/ref claim and renders snapshots.
- **Do not flatten signer capabilities.** NDK/Applesauce browser signers often
  expose optional encryption methods beside signing. In NMP, local-key browser
  sessions satisfy NIP-44 inline in Rust; browser NIP-46 sessions use pending
  provider routing; NIP-07 sessions can do NIP-44 only when the extension
  exposes both `window.nostr.nip44.encrypt` and `.decrypt`. A sign-only
  extension is not private-flow capable. See the shared matrix in
  [`docs/wasm-surface.md`](../wasm-surface.md#browser-signerprivate-flow-capability-model).
- **Do not expect full NDK feature parity.** NMP ships Rust-owned NIP-17 paths,
  but Wallet/NWC/Cashu/nutzap parity remains post-v1. Do not plan a migration
  that depends on wallet support.

## See also

- [01 — What NMP is + why it exists](01-what-nmp-is.md)
- [02 — Mental model — kernel + extension seams](02-mental-model.md)
- [07 — Subscription planner — Interest → CompiledPlan → wire](07-subscription-planner.md)
- [10 — Outbox routing (NIP-65)](10-outbox-routing.md)
- [11 — Sessions + signers + identity scopes](11-sessions-signers.md)
