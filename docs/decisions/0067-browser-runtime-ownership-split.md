# ADR-0067 — Browser runtime ownership split (`nmp-browser-runtime` owns the Worker)

- **Status:** Accepted; `nmp-wasm` deleted in #2202 (residual cleanup tail);
  amended by ADR-0069 and ADR-0072
- **Date:** 2026-06-25
- **Supersedes-in-part:** ADR-0047 (browser worker runtime contract), ADR-0054 (web persistence OPFS-SQLite)
- **Relates to:** crate-boundaries.md §10 (binding crates), §9 (app composition), ADR-0053 (host-declared projections), ADR-0050 (signer-session capability)
- **Tracking epic:** #2045; crate deletion tracked in #2202

**Current disposition:** the browser runtime ownership split survives. ADR-0069
narrows composition: browser runtimes start explicit app composition, not hidden
production defaults. ADR-0072 adds the durable Worker/storage/capability rule:
silent in-memory or no-worker degradation cannot count as product runtime proof.

**Supersession note (2026-06-30):** `the deleted defaults bundle` is deleted. References below
to runtime adapters composing it are historical context only; current browser
composition uses `nmp-substrate` plus explicit protocol/app installers.

## Context

The prior `crates/nmp-wasm` shape made the wrong work look local: agents saw
missing signing/outbox/runtime behaviour in the WASM crate or the Chirp Web
shell and patched *there* instead of using NMP's existing signer, router,
defaults, and protocol crates. `nmp-wasm` read as the browser runtime owner; it
exposed a raw `WasmRuntime`/`KernelReducer` composition surface, pre-start hooks,
a hand-wired relay pool, and NIP-07-specific identity vocabulary. That violated
D0 (kernel/binding never grows app nouns), D4 (single writer per fact), and D7
(one composition door).

`nmp-wasm` has been deleted (#2202). Zero workspace crates depended on it.
Its `protocol.rs` serializable structs are now re-owned by `nmp-browser-runtime`
(which already re-exported them).

## Decision

NMP web apps use a **two-layer ownership split** (three-layer as originally
designed; the first layer — the `nmp-wasm` protocol-type holding crate — was a
transitional artifact and has been deleted):

1. **`crates/nmp-browser-runtime` is the browser platform adapter** (Layer 6
   runtime adapter, sibling to the native runtime adapter). It is also the sole
   wasm-bindgen ABI glue crate for the browser. It may depend on Layer 5
   composition crates because it is a composition root, exactly like
   `nmp-native-runtime` and leaf app runtimes. It owns: the wasm-bindgen Worker
   export (`NmpWasmRuntime`), the Worker event loop driver, the browser WebSocket
   transport adapter (transport-only), browser storage open, the
   capability/signer provider registry, browser timer/clock seams, JS callback
   wiring, capability-result delivery, and the serializable browser Worker
   protocol types (`WorkerRequest`, `WorkerEvent`, etc.). It exposes a **typed
   builder** (`BrowserAppBuilder`) that cannot `start()` until storage, relay
   policy, consumed projections, capability/signer providers, and NMP default
   composition are all explicitly decided — the browser twin of
   `nmp-native-runtime::NmpAppBuilder`.

2. **NMP crates and app Rust crates remain the behaviour owners.** Generic Nostr
   behaviour stays in NMP crates (outbox/routing, signers, relays, protocol
   modules, storage contracts, projections, diagnostics, replay/time); product
   behaviour stays in app Rust crates; web UI/TypeScript renders and executes raw
   capabilities only.

### ABI doctrine: `nmp-browser-runtime::wasm` is the sole browser ABI glue

The `nmp-browser-runtime::wasm` module is the wasm-bindgen ABI glue over
`NmpRuntimeCore`. It owns ABI shape, `wasm_bindgen` attribute placement, JS
callback registration, panic guards, and `Uint8Array` ↔ byte-slice conversion.
It must not own routing or outbox policy, signing policy or signer-provider
semantics, NIP modules, protocol defaults, app defaults, projection policy,
persistence policy, legacy `nmp-wasm` protocol compatibility concerns, or any
other business policy.

### Crate name and layer

The browser runtime crate is named **`nmp-browser-runtime`** and lives at
`crates/nmp-browser-runtime`. In `docs/architecture/crate-boundaries.md` §2 it
is listed in the Layer-6 row alongside `nmp-native-runtime`, `nmp-ffi`, and
`nmp-android-ffi`. Runtime adapters compose `explicit composition`; `nmp-browser-runtime`
is both the runtime adapter and the wasm-bindgen ABI shell for the browser target.

### Shared composition surface

Both native (`NmpAppBuilder` in `nmp-native-runtime`, per ADR-0068) and browser
(`BrowserAppBuilder` in `nmp-browser-runtime`) compose NMP through the **same**
`nmp_core::substrate::AppHost`-rooted registration surface. Runtime builders
must not hand-copy action-module registration, router/mailbox construction,
publish-resolver installation, or parser wiring; they use the same explicit
installers as native app roots.

## Consequences

- `crates/nmp-wasm` is **deleted** (#2202, the residual cleanup tail of this
  ADR). No live crate depended on it; its protocol types are owned by
  `nmp-browser-runtime`. The `wasm_abi_gates.rs` doctrine-lint gate now
  enforces that `nmp-wasm` cannot be reintroduced.
- The existing Chirp Web implementation (`web/chirp`, `apps/chirp/crates/nmp-app-chirp-web`, current `web/packages/runtime-web` preview pieces) is **not** a compatibility target and is deleted/quarantined (#2052 / #2077-#2080).
- ADR-0047 and ADR-0054 are amended in place so browser runtime ownership is
  assigned to `nmp-browser-runtime`, not `nmp-wasm`.
- OPFS-SQLite persistence (ADR-0054 / #1007) stays the intended direction: the
  store is injected through the browser builder's storage decision, owned by
  `nmp-browser-runtime`.
