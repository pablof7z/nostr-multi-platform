# ADR-0047 — NMP browser worker runtime contract

- **Status:** Accepted
- **Date:** 2026-06-12
- **Relates to:** ADR-0009 (app-extension kernel boundary), ADR-0024 (async capability protocol), ADR-0037 (typed FlatBuffers runtime projections), ADR-0040 (capability-worker seam)
- **Reference:** `docs/wasm-surface.md` (living contract — the single source of truth for the wire protocol)

## Context

NMP ships a browser-host delivery surface (`crates/nmp-wasm`). This ADR
supersedes the prior `docs/design/chirp-web-runtime.md` design doc, which was
named after one example app (Chirp) and carried stale plan content and
inaccurate JSON-transport framing. A framework contract must not live under an
example-app name: the WASM surface is now documented at `docs/wasm-surface.md`,
mirroring the `docs/ffi-surface.md` treatment of the FFI surface.

The native `NmpApp` actor (in `nmp-ffi`) runs on a dedicated OS thread, uses
`flume` channels for dispatch, and drives a `tokio`/`mio` relay transport.
None of that execution model is available in single-threaded WebAssembly.

## Decision

### 1. The Worker event loop IS the actor

NMP's browser runtime is a `KernelReducer` driven on a dedicated Worker's
event loop. There is no ported copy of the native thread + flume + tokio actor.
The Worker thread is the single writer of kernel state (D4), and `wasm-bindgen`
closures parked on `WebSocket::onmessage` deliver relay frames **synchronously**
on that same event loop — the `build_on_message` handler body calls the kernel
callbacks directly with no `spawn_local` indirection
(`nmp-network/src/browser_driver.rs:286-299`). (`spawn_local` is used only in
the NIP-07 sign bridge,
`nmp-signers/src/signers/nip07/wasm.rs`, where it parks the async
`window.nostr.signEvent` Promise on the event loop.) The actor pattern is
preserved; the execution substrate is not.

### 2. Synchronous read/dispatch path + Promise-based async write path

`NmpWasmRuntime::handle_json` is synchronous. It accepts a JSON-serialised
`WorkerRequest` and returns a JSON array of control `WorkerEvent`s. Read
operations (Start, Stop, Dispatch of kernel-namespaced actions, SetSigner,
CapabilityResult) complete on the calling frame.

Write operations that require a signer (PublishNote, React, Follow, Unfollow)
cannot block the wasm thread on a JS Promise (`window.nostr.signEvent(...)` is
async). These go through
`NmpWasmRuntime::dispatch_app_action_async(request_json)`, which returns a
`js_sys::Promise` resolving to the outcome event. The synchronous
`handle_json` shape is unchanged; only the one path that needs an `await` uses
a Promise.

### 3. JSON control envelopes; binary FlatBuffers snapshot frames

Control events (`HelloAccepted`, `RuntimeStatus`, `ActionAccepted`,
`CapabilityFailure`, `Error`) cross the boundary as JSON-serialised
`WorkerEvent` arrays. The host pattern-matches on the `"type"` discriminator
field and each event except `HelloAccepted` carries a `correlation_id` matching
the originating request (`RuntimeStatus` and `Error` carry it as an
`Option` — absent for spontaneous emissions not tied to a specific request).

Kernel state snapshots (`WorkerEvent::UpdateBytes`) are **not** included in
the JSON array. They are delivered as raw `Uint8Array` bytes through the
snapshot callback registered with `set_snapshot_callback`. Encoding FlatBuffers
bytes as a JSON number array produced ~3–4× payload overhead at 4 Hz; the
binary channel avoids this.

`WorkerEvent::UpdateBytes` remains a Serde-serialisable variant for native
tests and diagnostics, but is routed out of the JSON path on wasm32 at the
`handle_json` call site.

### 4. Browser hosts must report explicit honest degraded modes

Hosts must never synthesise product state to hide a missing or incomplete
runtime. When the runtime cannot complete an operation it returns a stable,
pattern-matchable reason string in a `CapabilityFailure` event (see
`docs/wasm-surface.md` §5). The `DegradedMode` enum in the protocol vocabulary
(`BrowserActorDriverMissing`, `CapabilityRejected`, `ProtocolMismatch`) is
available for `RuntimeStatus::Degraded` emissions when a host needs to surface
a lifecycle-level degradation; failure modes at the individual-action level
surface as `CapabilityFailure`.

### 5. Correlation IDs on all request/response pairs

Every `WorkerRequest` that expects a response carries a `correlation_id` string
supplied by the host, with the sole exception of `Hello` — `HelloAccepted`
carries no correlation, because `Hello` is a fire-and-observe handshake, not a
tracked async request (`protocol.rs:33-37`). Every other responsive
`WorkerEvent` echoes the id back. This is the single event channel the host
uses to match responses to pending requests; there is no separate
callback-per-dispatch.

## Consequences

- **`crates/nmp-wasm` is the framework-level WASM delivery surface.** Its
  protocol is documented at `docs/wasm-surface.md`. Example apps (including
  Chirp) consume this surface; they do not define it.
- **The publish path requires the async entrypoint.** Any host that routes
  app-level writes through `handle_json` will receive a `CapabilityFailure`
  with reason `publish_path_not_wired` even after a signer is installed. This
  is intentional: it points the integration at the correct entrypoint.
- **IndexedDB persistence is not yet wired.** The kernel still runs in memory
  and resets on page reload. This is not a design decision — it is follow-on
  work (see `docs/wasm-surface.md` §7).
- **`WorkerRequest::AppAction` uses the framework-neutral `"app_action"` wire
  tag, and the async publish entrypoint is `start_publish_app_action`.** No
  residual Chirp-ism remains: a generic delivery surface preserves no
  example-app wire name (`nmp-wasm/tests/protocol.rs` asserts the
  `"type": "app_action"` wire shape; `nmp-wasm/src/runtime.rs` exposes
  `start_publish_app_action`).

## Alternatives considered

**Port the native threaded `NmpApp` actor to WebAssembly.** Rejected. The
native actor depends on `feature = "native"` — it drives a `flume`-based
dispatch channel, a `mio`-backed relay transport, and blocks an OS thread.
None of these primitives exist on `wasm32-unknown-unknown`. Porting them would
require either a second async runtime (incompatible with the single-threaded
wasm execution model) or full rewrites of the I/O subsystem. The
`KernelReducer` path already exists and is the correct seam.

**JSON for all events (snapshot + control in one array).** Rejected. Encoding
a FlatBuffers binary snapshot as a JSON number array produced ~3–4× overhead
per emission at 4 Hz. The binary callback channel is the production path.

**Per-event JS callbacks instead of a correlation-id channel.** Rejected.
Multiple in-flight dispatches (e.g. SetSigner overlapping with a Start) require
a single channel that the host can demultiplex by id. Per-event callbacks push
demultiplexing state into the host without the framework's help.
