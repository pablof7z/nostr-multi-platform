# 25 — Migration — NDK / Applesauce → NMP

> Status: **SHIPS** · Audience: builders coming from NDK or Applesauce.
> Prereqs: `01-what-nmp-is.md`, `02-mental-model.md`.

## The one shift to internalize first

NDK and Applesauce are **JS libraries you call**. NMP is a **Rust kernel that
owns your app's state**. In NDK/Applesauce your code holds the EventStore,
wires subscriptions, and decides relays. In NMP the actor owns all of that;
your platform code only `dispatch(action)`s intents and renders the snapshot
it is handed. You are not porting code — you are deleting the layer you used
to write. The synthesis behind this is in
`docs/design/ndk-applesauce-lessons.md:149-161`.

Critically, **Applesauce `model()` is not NMP `ViewModule`**. Applesauce is
RxJS observables in a browser; a model is an in-process stream you subscribe
to. An NMP `ViewModule` is an actor-owned projection that produces a bounded
snapshot crossing the FFI boundary at ≤60 Hz. They solve the same *problem*
(typed derived views) with incompatible *mechanics*. Treating them as the
same API is the central migration mistake.

## Deliverable 1 — translation table

| NDK term | Applesauce term | NMP term |
|---|---|---|
| `NDKRelaySet` / per-author relay calc | relay-map / `selectOptimalRelays` | `CompiledPlan` — the planner resolves relays from a `LogicalInterest`; you never assemble relay sets (`07-subscription-planner.md`; `docs/research/ndk/outbox.md`, `docs/research/applesauce/outbox.md`) |
| `ndk.subscribe(filters, opts)` | `eventStore.timeline(filters)` | `OpenView(spec)` dispatched as an action → kernel registers a `LogicalInterest`; you pass *intent*, not filters/relays (`docs/research/ndk/subscription-compilation.md`) |
| `NDKEvent` + manual derive | `eventStore.model(...)` (RxJS) | `ViewModule` — actor-owned projection emitted in the snapshot; **not** a stream you subscribe to (`docs/research/applesauce/event-store-query-builders.md`) |
| build event → `signer.sign` → `ndk.publish` | `ActionRunner` + `ctx.publish(event, relays?)` | `ActionModule` + the publish engine — one action signs, publishes (outbox-routed), and updates the store atomically (`docs/research/applesauce/outbox.md`) |
| `NDKPrivateKeySigner` / `NDKNip46Signer` / NIP-55 | `SimpleSigner` / `ExtensionSigner` / `AmberClipboardSigner` | `nmp-signers::Signer` (Local / NIP-46 / NIP-07) + Keyring capability; iOS Keychain SHIPS, iOS external-signer is a capability hook not turnkey (`docs/research/ndk/signers.md`, `docs/research/applesauce/signers.md`) |
| `@nostr-dev-kit/sessions` store + `activePubkey` | `AccountManager` + `IAccount` | kernel `AppState.session` + `nmp-signers::AccountManager`; account is identity-only, derived state lives in domain stores (`docs/research/sessions/synthesis.md:36-69`, `docs/research/ndk/wot-and-sessions.md`) |
| kind:3 watcher in sessions pkg + Svelte runes / React deps to rewire | consumer manually re-subscribes | **framework-magic** — kernel watches active account's kind:3, auto-recompiles every dependent interest on the wire; app dispatches **zero** code (`docs/research/ndk/kind3-auto-tracking.md:155-164`) |

## Deliverable 2 — things NMP does for you (stop writing this code)

Each item below is code you wrote in NDK/Applesauce that NMP **owns**:

- **Outbox routing.** No `calculateRelaySetFromEvent`, no passing `relays?`
  to publish. The planner resolves author write-relays + recipient
  inbox-relays automatically; manual relay selection is an audited opt-out
  (`docs/design/ndk-applesauce-lessons.md:107-117`).
- **kind:3 auto-rewire.** In NDK you needed Svelte runes or a React
  `[follows]` dep; in NMP the kernel restarts the wire subscription on
  kind:3 arrival with zero app code
  (`docs/research/ndk/kind3-auto-tracking.md:208-216`).
- **Subscription coalescing + lifecycle.** Applesauce dedupes by model hash
  but still sends one REQ per filter; NMP coalesces overlapping interests
  into minimal wire REQs and auto-closes on last consumer drop
  (`docs/research/applesauce/missing-features-for-nmp.md:38-43`).
- **Durable outbox + sync coverage.** NDK's tracker is in-memory and
  re-fetched every cold start; NMP persists relay metadata and NIP-77
  coverage as durable domain state
  (`docs/research/ndk/missing-features-for-nmp.md:33-41`).
- **Replaceable-event supersession + provenance merge.** No manual
  "is this newer by `created_at`?" guards — the EventStore enforces it on
  insert (`docs/research/applesauce/missing-features-for-nmp.md:13-15`).
- **Atomic write-then-store.** Actions publish *and* update local state as
  one operation; you cannot forget the local-state step
  (`docs/research/applesauce/outbox.md`).
- **Multi-account atomicity.** Switching account is one action; the kernel
  binds signer, rebuilds filters, and emits one snapshot — no
  logout/login/reload dance (`docs/research/sessions/synthesis.md:92`).

## Deliverable 3 — things you must not do

- **Do not 1:1 port NDK/Applesauce code.** The whole subscription/relay/store
  layer you wrote becomes `dispatch` calls. Porting it re-introduces the bug
  classes NMP exists to remove.
- **Do not treat Applesauce `model()` as `ViewModule`.** RxJS-in-browser vs
  actor-owned-snapshot-over-FFI. Don't expect JS event-stream ergonomics
  (`.pipe`, `.subscribe`, hot observables) across the FFI boundary; you get
  a snapshot with a monotonic `rev` guard
  (`docs/research/applesauce/event-store-query-builders.md`).
- **Do not import NDK relay-policy patterns.** No relay-set assembly, no
  per-call `relays` argument. Once the planner decides a relay is
  responsible for a subset of authors, that split is policy, not an
  app-overridable hint (`docs/design/ndk-applesauce-lessons.md:54-58`).
- **Do not reinvent Applesauce's `claimLatest` / refcount GC in app code.**
  The kernel's claim-based GC tracks which views reference which events and
  prunes automatically; an app-side parallel cache is a D4 violation
  (`docs/research/applesauce/missing-features-for-nmp.md:11-13`).
- **Do not hand-roll kind:3 watching in SwiftUI/Compose.** That is exactly
  the NDK trap (`docs/research/ndk/kind3-auto-tracking.md:155-162`); the
  kernel does it.
- **Do not expect NDK feature parity.** NDK ships DMs and a Wallet today;
  NMP defers both to post-v1 (`01-what-nmp-is.md`). Do not plan a migration
  that depends on them.

## See also

- [01 — What NMP is + why it exists](01-what-nmp-is.md)
- [02 — Mental model — kernel + 5 trait families](02-mental-model.md)
- [07 — Subscription planner — Interest → CompiledPlan → wire](07-subscription-planner.md)
- [10 — Outbox routing (NIP-65)](10-outbox-routing.md)
- [11 — Sessions + signers + identity scopes](11-sessions-signers.md)
