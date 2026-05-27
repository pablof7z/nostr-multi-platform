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
replaceable-event supersession, subscription coalescing/lifecycle, kind:3
auto-tracking, multi-account sessions, negentropy backfill. NMP's bet is
that these are **framework-level correctness policy, not per-app work**. If
a developer does not go out of their way to defeat the framework, the app
is correct.

NMP is **not** a port of NDK or Applesauce, and **not** a reimplementation
of the Nostr protocol. It depends on the `rust-nostr` crate family for
`Event`, `Filter`, NIP types, LMDB, and NIP-46/07 signing primitives. NMP
is the missing multiplatform *application* layer above them.

## Contrast — one paragraph each

**vs NDK (TypeScript).** NDK gets outbox-by-default ambition right and is
the reference for relay pools, sessions, sync, and WoT. But core NDK does
**not** auto-rewire open subscriptions when the follow list changes — that
"magic" comes from Svelte runes or explicit React deps, never core. Its
outbox tracker is in-memory and lost on cold start. NMP makes kind:3
auto-tracking framework-magic; apps dispatch zero code.

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

NMP ships an iOS social client (Chirp) on a working Rust kernel: outbox
routing, kind:3 auto-tracking, NIP-46 remote signing, NIP-77 negentropy
backfill, replaceable-event supersession, multi-account sessions, and the
reactive snapshot loop are all live. iOS Keychain is the production signer
path. The Highlighter and NIP-29 modules prove the kernel/extension
boundary holds for a second protocol surface.

Coming next: Blossom blob storage, an Android shell on the same kernel, a
WASM build for the browser, UniFFI to replace the raw C FFI, and a `nmp
init` CLI scaffold.

## Comparison — 6 axes

| Axis | NDK (TS) | Applesauce (TS) | raw `nostr-sdk` (Rust) | NMP |
|---|---|---|---|---|
| State ownership | app + zustand session store | app + RxJS subjects | app owns everything | single actor owns all state; app renders snapshots |
| Outbox routing | automatic *intent*; tracker in-memory, lost on restart | caller passes relays explicitly | none — caller picks relays | automatic & durable; manual relay is the audited opt-out |
| kind:3 auto-tracking | **not in core**; Svelte runes / React deps required | consumer subscribes manually | does not address | framework-magic; app dispatches zero code |
| Reactivity | RxJS / runes / hooks per framework | RxJS streams | does not address | actor reactive loop + bounded snapshots over FFI |
| Signers | NIP-07/46/55 (Android); no iOS external-signer | NIP-07/46; Android native; no iOS Secure Enclave | `nostr-connect` primitive only | `nmp-signers::Signer` + Keyring capability; iOS Keychain ships today |
| FFI / multiplatform | JS only | browser-first | Rust only | one Rust core, four delivery paths; raw C FFI today, UniFFI next |

`nostr-sdk`'s cells read "does not address" honestly: it is a protocol SDK,
not an application framework. NMP does not claim the app-layer axes as a
*replacement* — it is a different layer.

## What NMP is NOT

> - **NOT "Rust NDK."** It does not port NDK or Applesauce APIs. It is a
>   new actor-owned kernel synthesizing lessons from both.
> - **NOT a Nostr protocol library.** It depends on `rust-nostr`; it does
>   not reimplement events, filters, or NIPs.
> - **NOT at feature parity with NDK.** NDK ships DMs and Wallet today;
>   NMP defers both — see callout below. Do not read the comparison table
>   as "NMP > NDK."
> - **NOT a UI toolkit.** No SwiftUI/Compose components; the shell is the
>   builder's, the kernel is NMP's.

## Deferred to post-v1

> **NMP v1 does not ship DMs or Wallet.** The old NIP-17 DM stack (with
> the Notification Service Extension) and the NWC + zaps + Cashu + nutzaps
> stack are both deferred to post-v1. NDK has DMs and Wallet packages
> today; NMP does not. The eventual wallet and messaging designs are
> documented but aspirational. If you need DMs or a wallet now, NMP is not
> yet your framework.

## See also

- [02 — Mental model — kernel + 5 trait families](02-mental-model.md)
- [03 — Doctrine D0–D10 end-to-end](03-doctrine-d0-d8.md)
- [25 — Migration — NDK / Applesauce → NMP](25-migration-from-ndk-applesauce.md)
