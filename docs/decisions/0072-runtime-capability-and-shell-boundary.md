# ADR-0072: Runtime, capability, and shell boundary

## Status

Accepted for the architecture redesign direction.

## Context

NMP inherits RMP's core rule: Rust owns durable behavior and each platform
renders native UI. ADR-0067 and ADR-0068 split browser/native ABI glue from
runtime ownership. The redesign keeps that split, but tightens the product
boundary so typed sessions, actions, publish status, and capability results do
not leak into shell-owned policy or runtime-specific lifecycle recipes.

Downstream audits showed the same failure mode across web, iOS, Android, TUI,
widgets, AppIntents, CarPlay, Live Activities, workers, and server-ish helpers:
when the shell owns protocol parsing, relay policy, signer state, publish
completion, playback queue truth, or retry loops, the second platform has to
reimplement product correctness.

## Decision

Rust owns product state, protocol policy, relay routing, signing policy,
privacy, persistence, retry/recoverability, time decisions, durable navigation
meaning, and cross-platform invariants.

Native/web/desktop/TUI shells own exactly:

- rendering and platform UX affordances;
- execution of OS/browser capabilities;
- ephemeral presentation state that cannot become product truth.

For an app developer, this means Swift, Kotlin, TypeScript, or TUI code should
mostly do three things:

- start or attach to the Rust-owned app runtime;
- open typed read sessions and dispatch typed actions;
- render emitted state and answer capability requests.

Anything a second platform would need to reimplement to stay correct belongs in
Rust: read-source expansion, relay choice, signer choice, tag/envelope mutation,
publish retry, cache truth, admission policy, privacy checks, product queue
state, durable navigation meaning, and user-visible operation status.

Capability flow is:

```text
Rust requests capability
  -> shell executes raw OS/browser/API operation
  -> shell reports raw success/failure/data
  -> Rust decides state transition, retry, status, and user-visible meaning
```

Runtime crates own runtime lifecycle and platform constraints.
`nmp-native-runtime` owns native actor lifecycle and native builder state.
`nmp-uniffi` is the public native binding surface for iOS, Android, and desktop
hosts. Retained `nmp-ffi` C ABI symbols are transitional/internal compatibility
or test/app glue with owning issues, not a second public native API.
`nmp-browser-runtime` owns browser worker/runtime/wasm-bindgen ABI glue and
browser capability brokerage. `nmp-defaults` remains reusable composition, not a
runtime.

Browser durable storage must initialize before product start when durable mode
is required. Dedicated Worker/OPFS requirements are architectural constraints,
not test details. Missing wasm, missing Worker, OPFS failure, Web Locks
contention, or unsupported signer capability must produce typed degraded/failure
state or fail the proof. Silent in-memory fallback cannot count as product
runtime success.

Headless and OS-owned surfaces use typed actions, short-lived headless
invocation, capability results, or last Rust-emitted mirror frames first.
App-lifetime typed sessions are allowed only after a selected proof shows
resident state is required and uses the same lifecycle contract as visible
screens. Widgets, AppIntents, CarPlay, remote commands, Live Activities, share
extensions, and suspended-process resumes must not own parallel playback queues,
signer state, relay policy, deep-link admission, or publish result models.

## Consequences

Positive:

- Product correctness has one owner across Swift, Kotlin, TypeScript, TUI, and
  browser.
- Capability bridges stay small and replaceable.
- Browser/native runtime crates can enforce startup, storage, and capability
  ordering without owning app policy.
- Headless/OS flows cannot report fake success from dispatch acceptance.

Negative/tradeoffs:

- Some platform features require more typed action/result plumbing before they
  are correct.
- Browser proof requires real Worker/OPFS checks where durable storage is part of
  the claim.
- Native mirrors need explicit schema/cadence ownership instead of local caches.

## Alternatives considered

| Option | Why rejected |
|---|---|
| Let shells own small bits of protocol or publish policy for convenience | A second platform would have to reimplement them, violating the Rust-owned product boundary. |
| Treat browser degraded mode as success | It changes persistence/runtime semantics while hiding the failure. |
| Keep runtime ownership in ABI crates | It recreates the confusion ADR-0067 and ADR-0068 removed. |
| Use polling or refresh timers for correctness | NMP doctrine requires event-driven or blocking primitives; polling hides missing lifecycle ownership. |

## Fitness functions / enforcement

- Native/web shells do not choose Nostr relays, parse protocol meaning for
  product state, infer signer/publish success, or own retry/recoverability.
- Capability tests assert raw-result reporting and Rust-owned policy decisions.
- Browser runtime proof uses real Worker/OPFS storage where durable mode is
  claimed.
- OS/headless surfaces report typed Rust-owned pending/error/completion state,
  not just accepted dispatch.
- Polling/timer call sites are classified as presentation/capability sampling or
  rejected.

## Linked work

- ADR-0067: browser runtime ownership split.
- ADR-0068: native runtime ownership split.
- ADR-0064: typed action/write boundary.
- #2316: fragmented feature-state lifecycle.
- #2320: source-of-truth cleanup.
