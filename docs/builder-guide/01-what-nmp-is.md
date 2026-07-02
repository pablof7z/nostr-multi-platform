# 01 — What NMP is + why it exists

> Status: **SHIPS** · Audience: builders. Read `00-how-to-read.md` for the
> SHIPS/LANDED/PLANNED legend and reading paths.

## One paragraph

NMP (Nostr Multi-Platform) is a **Rust application kernel for building Nostr
apps**. One Rust core owns all protocol logic, all state, all caching, all
relay management, all signing orchestration, and all derived views; the
platform (SwiftUI / Compose / desktop / web) is a thin rendering shell that
calls `dispatch(action)` and re-renders a snapshot. The thesis is simple:
**make it nearly impossible to ship a broken Nostr app.** Whole bug classes —
stale replaceable events, leaked subscriptions, wrong relays for wrong
events, local/relay state races, multi-account desync — are ruled out by the
type system, single-actor ownership, and the FFI surface, not by
documentation or a linter. The doctrine D0–D10 is the contract that enforces
this; see `03-doctrine-d0-d8.md`.

## Why it exists

Every serious Nostr client re-derives the same hard parts: outbox routing,
replaceable-event supersession, subscription coalescing/lifecycle, dynamic
source tracking, multi-account sessions, negentropy backfill. NMP's bet is
that these are **framework-level correctness policy, not per-app work**. If
a developer does not go out of their way to defeat the framework, the app
is correct.

NMP is **not** a port of NDK or Applesauce, and **not** a reimplementation
of the Nostr protocol. It depends on the `rust-nostr` crate family for
`Event`, `Filter`, NIP types, LMDB, and NIP-46/07 signing primitives. NMP
is the missing multiplatform *application* layer above them.

## What you stop writing

The framework owns these. You don't write them, don't test them, don't debug
them.

- **Relay selection.** Not per-subscription. Not per-publish. Outbox routing
  is on; the planner decides which relay gets which REQ.
- **REQ/CLOSE lifecycle.** Open a view; subscriptions follow. Close the view;
  they close.
- **Duplicate filtering.** The store deduplicates on insert; the same event
  arriving on three relays stores once.
- **Replaceable-event supersession.** The store rejects older versions. You
  cannot hold a stale kind:0 or kind:3.
- **Dynamic-source auto-rewire.** When a follow/list/pointer source changes,
  dependent interests re-route. Zero app code.
- **DM decryption.** NIP-17 plaintext never reaches Swift or Kotlin.
- **Reconnect handling.** The relay manager retries; the view loop does not see
  disconnects.
- **Profile caching.** kind:0 is tracked and kept current; you render the
  freshest value or a deterministic placeholder (never a spinner).

If you find yourself writing any of these, stop. Either the framework already
handles it, or you've found a gap worth filing.

## Contrast — one paragraph each

**vs NDK (TypeScript).** NDK gets outbox-by-default ambition right and is
the reference for relay pools, sessions, sync, and WoT. But core NDK does
**not** auto-rewire open subscriptions when the follow list changes — that
"magic" comes from Svelte runes or explicit React deps, never core. Its
outbox tracker is in-memory and lost on cold start. NMP makes dynamic source
tracking framework-magic; apps dispatch zero rewire code.

**vs Applesauce (TypeScript).** Applesauce's lesson is *separation*: store,
models, relay selection, transport, and actions are distinct concerns. NMP
adopts that discipline. But Applesauce is RxJS streams in a browser and
exposes explicit relay routing to app code; NMP is actor-owned state
crossing FFI as bounded snapshots, with outbox routing invisible to app
code.

**vs raw `nostr-sdk` (Rust).** `nostr-sdk` gives you `Client`, a relay
pool, and async streaming — protocol mechanics. It does not give you
actor-owned app state, a subscription planner, automatic outbox routing,
derived reactive views, a kernel/extension boundary, or an FFI surface.
NMP composes `nostr-sdk` primitives into an opinionated app kernel; that
composition is the project.

## What ships today, what's coming

NMP ships a working Rust kernel consumed by native and browser shells: outbox
routing, active-user follow source tracking, NIP-46 remote signing, NIP-77
negentropy backfill, replaceable-event supersession, multi-account sessions,
and the reactive snapshot loop are all live. iOS Keychain is the production
signer path. The Highlighter and NIP-29 modules prove the kernel/extension
boundary holds for additional protocol surfaces. `nmp init` scaffolds a
ready-to-build Rust workspace with a thin `<name>-core` composition crate, a
`<name>-app` app-owned UniFFI facade, app-local generated action builders, and a
starter action-to-reactive-view path plus headless `examples/shell.rs`. Native
shells consume the app facade through UniFFI bindings; browser shells consume
the wasm-bindgen runtime and generated action bytes. Composition is app-owned
Rust code using reusable installers, and the scaffold includes checks for both
action-builder drift and Swift/Kotlin UniFFI binding generation.
Chirp is now an external consumer rather than an in-repo app.

Coming next: Blossom blob storage, continued browser-runtime hardening, and
the full multi-platform starter around the same explicit composition model.

## Comparison — 6 axes

| Axis | NDK (TS) | Applesauce (TS) | raw `nostr-sdk` (Rust) | NMP |
|---|---|---|---|---|
| State ownership | app + zustand session store | app + RxJS subjects | app owns everything | single actor owns all state; app renders snapshots |
| Outbox routing | automatic *intent*; tracker in-memory, lost on restart | caller passes relays explicitly | none — caller picks relays | automatic & durable; manual relay is the audited opt-out |
| dynamic source tracking | kind:3 follow tracking lives in sessions/framework glue; Svelte runes / React deps still wire app filters | consumer subscribes manually | does not address | typed session helpers own dynamic sources; app dispatches zero rewire code |
| Reactivity | RxJS / runes / hooks per framework | RxJS streams | does not address | actor reactive loop + bounded snapshots over FFI |
| Signers | NIP-07/46/55 (Android); no iOS external-signer | NIP-07/46; Android native; no iOS Secure Enclave | `nostr-connect` primitive only | `nmp-signers::Signer` + Keyring capability; iOS Keychain ships today |
| FFI / multiplatform | JS only | browser-first | Rust only | one Rust core; UniFFI for native, wasm-bindgen for browser |

`nostr-sdk`'s cells read "does not address" honestly: it is a protocol SDK,
not an application framework. NMP does not claim the app-layer axes as a
*replacement* — it is a different layer.

## What NMP is NOT

> - **NOT "Rust NDK."** It does not port NDK or Applesauce APIs. It is a
>   new actor-owned kernel synthesizing lessons from both.
> - **NOT a Nostr protocol library.** It depends on `rust-nostr`; it does
>   not reimplement events, filters, or NIPs.
> - **NOT at feature parity with NDK.** NMP ships NIP-17 through Rust-owned
>   modules and reference-app surfaces, but each signer/backend has explicit
>   capability limits; Wallet remains deferred. Do not read the comparison
>   table as "NMP > NDK."
> - **NOT a UI toolkit.** No SwiftUI/Compose components; the shell is the
>   builder's, the kernel is NMP's.

## Capability caveats

> **NMP does not promise every signer can do every private flow.** Local-key
> signers can satisfy NIP-44 directly; NIP-46 signers depend on the remote
> provider approving `nip44_encrypt` / `nip44_decrypt`; NIP-07 browser
> extensions are private-flow capable only when they expose both optional
> `window.nostr.nip44` verbs. See the browser capability matrix in
> [`docs/wasm-surface.md`](../wasm-surface.md#browser-signerprivate-flow-capability-model).
> Wallet/NWC/Cashu/nutzap parity remains post-v1.

## See also

- [02 — Mental model — kernel + extension seams](02-mental-model.md)
- [03 — Doctrine D0–D10 end-to-end](03-doctrine-d0-d8.md)
- [NIP support matrix](../nips.md)
- [25 — Migration — NDK / Applesauce → NMP](25-migration-from-ndk-applesauce.md)
