# runtime-web

Worker client for the `nmp-browser-runtime` byte transport. **NOT the NMP
browser runtime owner** — that is the `nmp-browser-runtime` Rust crate.

## Purpose

This package provides generic TypeScript glue for the worker-based ABI surface:

- **DispatchEnvelope** — byte-frame builders for ADR-0071 typed write transport.
- **WasmBridge** — FlatBuffers decoder + worker message router.
- **DegradedRuntime** — fallback message handler when the wasm bridge is unavailable.
- **Worker shim** — postMessage client that routes requests/responses between main thread and worker.
- **Packaged wasm asset** — the `nmp-browser-runtime` wasm-bindgen output staged
  into `dist/wasm` during package build.
- **NIP-07 sign broker** (`fulfilSignRequestViaExtension` / `installNip07SignBroker`)
  — the main-thread fulfiller for the `begin_sign` / `sign_request` round-trip
  (Web Workers have no `window.nostr`).
- **`decodeUpdateFrame`** — read-side decode of the `NMPU` `update_bytes` wire
  into the declared projection-key set plus a typed-sidecar lookup, verified
  against the GENERATED `PROJECTION_CONTRACT`.
- **`KeyedRefCache`** — the keyed per-row reference cache (`refs.profile` /
  `refs.event`), GENERATED from the same registry as the Swift/Kotlin
  `KeyedRefCache` twins (see `crates/nmp-codegen/src/ts_keyed_cache.rs`), with
  a typed `profile(pubkey)` / `profiles()` accessor.

The keyed-cache and projection-contract modules are GENERATED — regenerate via
`cargo run -p nmp-codegen -- gen keyed-ref-cache --platform ts --out
web/packages/runtime-web/src/keyedRefCache.generated.ts` and `cargo run -p
nmp-codegen -- gen projection-contract --platform ts --out
web/packages/runtime-web/src/projectionContract.generated.ts`. Never edit
`*.generated.ts` by hand.

No Nostr protocol, signing, or routing policy is implemented here. All such
logic lives in Rust (`nmp-browser-runtime` and lower NMP crates).

## Usage

The web worker loads this package and instantiates a `WasmBridge` or `DegradedRuntime` to handle incoming `WorkerRequest` messages and emit `WorkerEvent` responses.

Split web apps import the worker as a package subpath:

```ts
const worker = new Worker(new URL("@nmp/runtime-web/worker", import.meta.url), {
  type: "module",
});
```

They should not copy `web/packages/runtime-web` or manually stage
`/public/nmp-browser-runtime`; the package build owns that artifact.

## Product Rule

**Web UI must not expose a control unless it is backed by a real Rust/NMP action and projection.**

Example violations:
- Replies wait for Rust-owned NIP-10 action support, never TypeScript tag construction.
- Reactions must route through the Rust publish path, not raw event signing at the UI boundary.
- Follow buttons must call the Rust follow action, not construct kind:3 manually.

This rule ensures that every UI control has a corresponding Rust/NMP action owner and cannot drift from the kernel's actual capabilities or event-format evolution. UI-only controls are forbidden.

See `docs/wasm-surface.md` for the browser runtime ABI contract.
