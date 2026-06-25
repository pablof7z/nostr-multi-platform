# runtime-web

ABI/worker client for nmp-wasm byte transport. **NOT the NMP browser runtime owner** — that is the `nmp-browser-runtime` Rust crate.

## Purpose

This package provides generic TypeScript glue for the worker-based ABI surface:

- **DispatchEnvelope** — byte-frame builders for ADR-0064 typed write transport.
- **WasmBridge** — FlatBuffers decoder + worker message router.
- **DegradedRuntime** — fallback message handler when the wasm bridge is unavailable.
- **Worker shim** — postMessage client that routes requests/responses between main thread and worker.

No Nostr protocol, signing, or routing policy is implemented here. All such logic lives in Rust (`nmp-wasm` and `nmp-browser-runtime` crates).

## Usage

The web worker loads this package and instantiates a `WasmBridge` or `DegradedRuntime` to handle incoming `WorkerRequest` messages and emit `WorkerEvent` responses.

Main thread code should not import from this package directly; instead, use the browser runtime's public host API.

## Product Rule

**Web UI must not expose a control unless it is backed by a real Rust/NMP action and projection.**

Example violations:
- Replies wait for Rust-owned NIP-10 action support, never TypeScript tag construction.
- Reactions must route through the Rust publish path, not raw event signing at the UI boundary.
- Follow buttons must call the Rust follow action, not construct kind:3 manually.

This rule ensures that every UI control has a corresponding Rust/NMP action owner and cannot drift from the kernel's actual capabilities or event-format evolution. UI-only controls are forbidden.

See #2038 (nmp-browser-runtime Rust crate rebuild) for the path to restore web product enforcement of this rule.
