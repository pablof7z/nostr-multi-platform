# 01 — What NMP is + why it exists

> Status: **SHIPS** · Audience: builders. Read `00-how-to-read.md` for the
> SHIPS/LANDED/PLANNED legend and reading paths.

## One paragraph

NMP (Nostr Multi-Platform) is a **Rust application kernel for building Nostr
apps**. One Rust core owns all protocol logic, all state, all caching, all
relay management, all signing orchestration, and all derived views; the
platform (SwiftUI / Compose / desktop / web) is a thin rendering shell that
calls `dispatch(action)` and re-renders a snapshot. The framing thesis, from
`docs/aim.md:9-15`: **make it nearly impossible to ship a broken Nostr app.**
Whole bug classes — stale replaceable events, leaked subscriptions, wrong
relays for wrong events, local/relay state races, multi-account desync — are
ruled out by the type system, single-actor ownership, and the FFI surface,
not by documentation or a linter. The doctrine D0–D8 (`docs/plan.md:9`) is the
contract that enforces this; see `03-doctrine-d0-d8.md`.

## Why it exists — the thesis

Every serious Nostr client re-derives the same hard parts: outbox routing,
replaceable-event supersession, subscription coalescing/lifecycle, kind:3
auto-tracking, multi-account sessions, negentropy backfill. NMP's bet is that
these are **framework-level correctness policy**, not per-app work. If a
developer does not go out of their way to defeat the framework, the app is
correct. The research that grounds this is durable: `docs/research/ndk/` and
`docs/research/applesauce/` reverse-engineer two mature TypeScript libraries,
and `docs/design/ndk-applesauce-lessons.md:149-161` distills the synthesis —
take NDK's "outbox is automatic by default" ambition and Applesauce's
compositional discipline, keep Rust as the sole owner of policy.

NMP is **not** a port of either library, and **not** a reimplementation of
the Nostr protocol — it depends on the `rust-nostr` crate family for `Event`,
`Filter`, NIP types, LMDB, NIP-46/07 signing primitives
(`docs/aim.md:74-89`). NMP is the missing multiplatform *application* layer
above them.

## Contrast — one paragraph each

**vs NDK (TypeScript).** NDK gets outbox-by-default ambition right and is the
reference for relay pools, sessions, sync, WoT. But core NDK does **not**
auto-rewire open subscriptions when the follow list changes — that "magic"
comes from Svelte runes or explicit React deps, never core
(`docs/research/ndk/kind3-auto-tracking.md:98-101`). Its outbox tracker is
in-memory and lost on cold start
(`docs/research/ndk/missing-features-for-nmp.md:33-41`). NMP makes kind:3
auto-tracking framework-magic — apps dispatch zero code.

**vs Applesauce (TypeScript).** Applesauce's lesson is *separation*: store,
models, relay selection, transport, actions are distinct concerns
(`docs/design/ndk-applesauce-lessons.md:33-37`). NMP adopts that discipline.
But Applesauce is RxJS streams in a browser and exposes explicit relay
routing to app code; NMP is actor-owned state crossing FFI as bounded
snapshots, with outbox routing invisible in app code
(`docs/research/applesauce/missing-features-for-nmp.md:20-27`).

**vs raw `nostr-sdk` (Rust).** `nostr-sdk` gives you `Client`, a relay pool,
and async streaming — protocol mechanics. It does not give you actor-owned
app state, a subscription planner, automatic outbox routing, derived
reactive views, a kernel/extension boundary, or an FFI surface. NMP composes
`nostr-sdk` primitives into an opinionated app kernel; that composition is
the project.

## The four arcs (status snapshot)

Per `docs/plan.md:5` and `docs/plan.md:30-52`, NMP ships along arcs:

| Arc | Milestones | Proves | Status (HB31, `docs/perf/orchestration-log.md:38-41`) |
|---|---|---|---|
| 1 — Social stack | M0–M8, M10, M10.5 | Kernel substrate + Nostr social slice + FFI hardening | M0–M8 + M10.5 **DONE**; M10 Blossom pending |
| 2 — Kernel boundary | M11, M11.5 | Non-social app (podcast) + Highlighter/NIP-29 prove D0 holds | M11 + M11.5 **step 0 DONE** |
| 3 — Release | M13–M17 | WoT, UniFFI, cross-platform, CLI, v1 | pending |

M0–M8 + M10.5 + M11 step 0 + M11.5 are DONE on master per HB31; M10
(Blossom) + M13–M17 remain on the v1 ladder.

## Deliverable 1 — comparison table (6 axes)

| Axis | NDK (TS) | Applesauce (TS) | raw `nostr-sdk` (Rust) | NMP |
|---|---|---|---|---|
| State ownership | app + zustand session store | app + RxJS subjects | app owns everything | single actor owns all state; app renders snapshots (`docs/aim.md:25-31`) |
| Outbox routing | automatic *intent*; tracker in-memory, lost on restart | caller passes relays explicitly | none — caller picks relays | automatic & durable; manual relay is the audited opt-out (`docs/plan.md:9` D3) |
| kind:3 auto-tracking | **not in core**; Svelte runes / React deps required | consumer subscribes manually | does not address | framework-magic; app dispatches zero code (`docs/design/ndk-applesauce-lessons.md` synthesis) |
| Reactivity | RxJS / runes / hooks per framework | RxJS streams | does not address | actor reactive loop + bounded snapshots over FFI (`docs/plan.md:9` D8) |
| Signers | NIP-07/46/55(Android); no iOS external-signer | NIP-07/46; Android native; no iOS Secure Enclave | `nostr-connect` primitive only | `nmp-signers::Signer` + Keyring capability; iOS Keychain SHIPS (M6) |
| FFI / multiplatform | JS only | browser-first | Rust only | one Rust core, four delivery paths; raw C FFI today, UniFFI at M14 (PLANNED) |

`nostr-sdk`'s cells read "does not address" honestly: it is a protocol SDK,
not an application framework. NMP does not claim the app-layer axes as a
*replacement* — it is a different layer.

## Deliverable 2 — what NMP is NOT (box)

> - **NOT "Rust NDK."** It does not port NDK or Applesauce APIs. It is a new
>   actor-owned kernel synthesizing lessons from both
>   (`docs/aim.md:93-99`).
> - **NOT a Nostr protocol library.** It depends on `rust-nostr`; it does
>   not reimplement events, filters, or NIPs (`docs/aim.md:74-89`).
> - **NOT at feature parity with NDK.** NDK ships DMs and Wallet today; NMP
>   defers both — see callout below. Do not read the comparison table as
>   "NMP > NDK."
> - **NOT a UI toolkit.** No SwiftUI/Compose components; the shell is the
>   builder's, the kernel is NMP's (`02-mental-model.md`).
> - **NOT everything in `docs/aim.md` at v1.** `aim.md` is the long-term
>   north star; v1 ships the kernel subset per `docs/plan.md`, with explicit
>   deferrals (`docs/aim.md:175-177`).

## Deliverable 3 — deferred to post-v1 (callout)

> **NMP v1 does not ship DMs or Wallet.** Per
> `docs/plan/scope-adjustments-2026-05-18.md` (and `docs/plan.md:42,47`),
> the old M9 (NIP-17 DMs + NSE) and M12 (NWC + zaps + Cashu + nutzaps) are
> **deferred to post-v1** (`docs/plan/post-v1.md`); M11.5 Highlighter +
> `nmp-nip29` took that slot. NDK has DMs and Wallet packages today; NMP
> does not. `aim.md` §4.9–§4.10 describe the eventual wallet/messaging
> design, but those are aspirational, not v1. If you need DMs or a wallet
> now, NMP is not yet your framework. Track the discrepancy register in
> `27-discrepancies.md`.

## See also

- [02 — Mental model — kernel + 5 trait families](02-mental-model.md)
- [03 — Doctrine D0–D8 end-to-end](03-doctrine-d0-d8.md)
- [25 — Migration — NDK / Applesauce → NMP](25-migration-from-ndk-applesauce.md)
