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

### 2. Synchronous read/dispatch path; writes go through the unified byte doorway

`NmpWasmRuntime::handle_json` is synchronous. It accepts a JSON-serialised
`WorkerRequest` and returns a JSON array of control `WorkerEvent`s. Read and
lifecycle operations (Start, Stop, Dispatch of kernel-namespaced actions,
CapabilityResult) complete on the calling frame.

Write operations (publish, react, follow, unfollow) cross the worker boundary
through the **single typed byte transport** defined in
[ADR-0064](0064-unified-write-command-boundary.md): one `DispatchBytes` doorway
carrying an open `DispatchEnvelope` (generated namespace + typed FlatBuffers
payload), reached only through generated typed builders — never a hand-written
wire tag, and never a wasm-specific action vocabulary. Signing is **not**
awaited inside the publish flow: it is the ADR-0050 signer-session capability
port (`sign` verb, mailbox-delivered completion), with NIP-07 as one fulfiller.
The reducer never blocks on a JS Promise; the async `window.nostr.signEvent(...)`
round-trip happens in the host capability bridge and re-enters Rust as an
explicit signed-or-rejected event (D7/D8). `correlation_id` identifies the write
from dispatch to terminal and never re-binds to the event id.

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
- **IndexedDB persistence is not yet wired.** The kernel still runs in memory
  and resets on page reload. This is not a design decision — it is follow-on
  work (see `docs/wasm-surface.md` §7).
- **The worker write/signing contract is owned by
  [ADR-0064](0064-unified-write-command-boundary.md).** Writes ride the one
  `DispatchBytes` byte doorway (open `DispatchEnvelope` + typed per-crate
  FlatBuffers payloads, generated typed builders), and signing rides the ADR-0050
  capability port. The hand-rolled `WorkerRequest::AppAction` / `"app_action"`
  wire tag and the `dispatch_app_action_async` / `start_publish_app_action` /
  `SetSigner`-as-install signing path are removed by that migration — they were a
  second, wasm-only write-and-sign vocabulary, which is exactly what ADR-0064
  collapses onto the shared native seam.

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
