# ADR-0067 — Browser runtime ownership split (nmp-wasm is ABI glue)

- **Status:** Accepted
- **Date:** 2026-06-25
- **Supersedes-in-part:** ADR-0047 (browser worker runtime contract), ADR-0054 (web persistence OPFS-SQLite)
- **Relates to:** crate-boundaries.md §10 (binding crates), §9 (app composition), ADR-0053 (host-declared projections), ADR-0050 (signer-session capability)
- **Tracking epic:** #2045

## Context

The current `crates/nmp-wasm` shape makes the wrong work look local: agents see missing signing/outbox/runtime behaviour in the WASM crate or the Chirp Web shell and patch *there* instead of using NMP's existing signer, router, defaults, and protocol crates. `nmp-wasm` reads as the browser runtime owner; it exposes a raw `WasmRuntime`/`KernelReducer` composition surface, pre-start hooks, a hand-wired relay pool, and NIP-07-specific identity vocabulary. That violates D0 (kernel/binding never grows app nouns), D4 (single writer per fact), and D7 (one composition door).

## Decision

NMP web apps use a **three-layer ownership split**:

1. **`crates/nmp-wasm` is the ABI/delivery shell only.** It owns wasm-bindgen exports, byte-oriented dispatch in/out, capability-result byte intake, JS callback registration, panic/error guards, and lifecycle handle mechanics. It owns **no** routing, signing policy, signer-provider choice, NIP modules, protocol defaults, app defaults, projection policy, persistence policy, retry policy, or account state. No public ABI method name may encode a Nostr/product feature (no `signer_kind`, `outbox`, `reply`, `profile`, `relay_policy`).

2. **`crates/nmp-browser-runtime` is the browser platform adapter** (Layer 6 runtime adapter, sibling to the native runtime adapter). It may depend on Layer 5 composition crates because it is a composition root, exactly like `nmp-native-runtime` and leaf app runtimes. It owns: the Worker event loop driver, the browser WebSocket transport adapter (transport-only), browser storage open, the capability/signer provider registry, browser timer/clock seams, JS callback wiring, and capability-result delivery. It exposes a **typed builder** (`BrowserAppBuilder`) that cannot `start()` until storage, relay policy, consumed projections, capability/signer providers, and NMP default composition are all explicitly decided — the browser twin of `nmp-native-runtime::NmpAppBuilder`.

3. **NMP crates and app Rust crates remain the behaviour owners.** Generic Nostr behaviour stays in NMP crates (outbox/routing, signers, relays, protocol modules, storage contracts, projections, diagnostics, replay/time); product behaviour stays in app Rust crates; web UI/TypeScript renders and executes raw capabilities only.

### Crate name and layer

The browser runtime crate is named **`nmp-browser-runtime`** and lives at `crates/nmp-browser-runtime`. In `docs/architecture/crate-boundaries.md` §2 it is added to the Layer-6 row alongside `nmp-native-runtime`, `nmp-ffi`, and `nmp-wasm` with the note that runtime adapters compose `nmp-defaults`, while ABI shells delegate to those runtimes. `nmp-wasm` stays in Layer 6 as ABI glue.

### Shared composition surface

Both native (`NmpAppBuilder` in `nmp-native-runtime`, per ADR-0068) and browser (`BrowserAppBuilder` in `nmp-browser-runtime`) register NMP defaults/protocol modules through the **same** `nmp_core::substrate::AppHost`-rooted registration surface. Runtime builders must not hand-copy action-module registration, router/mailbox construction, publish-resolver installation, or parser wiring; they call `nmp_defaults::register_defaults`.

## Consequences

- The existing Chirp Web implementation (`web/chirp`, `apps/chirp/crates/nmp-app-chirp-web`, current `web/packages/runtime-web` preview pieces) is **not** a compatibility target and is deleted/quarantined (#2052 / #2077-#2080).
- ADR-0047 and ADR-0054 are amended in place so browser runtime ownership is assigned to `nmp-browser-runtime`, not `nmp-wasm`.
- OPFS-SQLite persistence (ADR-0054 / #1007) stays the intended direction but is gated behind this split: the store is injected through the browser builder's storage decision, not constructed inside `nmp-wasm`.
- The legacy internal `RawWasmAbiAdapter` path in `crates/nmp-wasm` is retired
  once `nmp-browser-runtime::wasm::NmpWasmRuntime` covers the browser callers.
  `nmp-wasm` remains only as a serializable protocol-type crate for older Rust
  consumers; browser runtime behavior and wasm-bindgen exports live in
  `nmp-browser-runtime`.
