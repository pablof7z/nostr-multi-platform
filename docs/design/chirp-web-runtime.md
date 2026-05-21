# Chirp Web Runtime

Chirp Web is a browser host for the same Rust-owned application model as iOS,
Android, desktop, and TUI. TypeScript renders state and executes browser
capabilities. Rust owns policy, routing, replay, Nostr protocol behavior, and
state transitions.

## Landing Contract

- `crates/nmp-wasm` defines the worker protocol in Rust and keeps it
  host-testable without linking native-only core dependencies into the browser
  facade.
- `web/chirp` consumes the same protocol shape in TypeScript, runs it through a
  dedicated worker, and treats runtime status as first-class UI state.
- Browser hosts must never synthesize product state to hide missing runtime
  support. Missing pieces report explicit degraded modes.
- Worker messages carry correlation ids for actions, capability completions,
  lifecycle transitions, and diagnostics. Core update envelopes cross this
  boundary as JSON so the browser worker API is stable while the native Rust
  envelope type can continue to evolve internally.

## Worker Shape

The browser runs NMP in a dedicated worker:

1. UI sends `hello` with protocol version and platform metadata.
2. UI sends `start` with app id, relay URLs, database name, and correlation id.
3. Rust returns `runtime_status`, `update`, `capability_failure`, or `error`
   events.
4. UI sends user intent as `dispatch` messages.
5. Browser APIs report raw capability results through `capability_result`.

The current slice intentionally returns `browser_actor_driver_missing` after
`start`. That event now travels through a dedicated browser worker, which means
the UI is already using the boundary where the generated wasm runtime will sit.
It is still a real degraded state, not a mock success path. The next slice links
a single-threaded browser actor driver behind the same protocol.

## Follow-On Work

- Add a browser relay transport using WebSocket callbacks instead of polling.
- Add an IndexedDB replay log adapter that feeds Rust explicit events.
- Link generated Chirp app dispatch into `nmp-wasm`.
- Move the web app from degraded diagnostics to live snapshot rendering.
- Add parity fixtures that compare web snapshots with iOS, Android, desktop,
  and TUI projections for the same action history.
