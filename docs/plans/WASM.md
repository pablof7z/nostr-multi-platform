# WASM Plan — Making nmp-wasm Real

**Status:** Planning  
**Last updated:** 2026-05-22  
**Related:** `docs/plan/m15-cross-platform.md`, ADR-0022, ADR-0024, ADR-0026

---

## 1. Current state (the honest baseline)

`crates/nmp-wasm` is a browser-local simulation. It has zero `nmp-core` dependency.
`WasmRuntime` stores notes in a `Vec<LocalNote>`, emits `CapabilityFailure` for every
action except `PublishNote`, and sets `author_pubkey: "browser-local"` on every event.
The multi-platform claim is structurally false until this is fixed.

What IS real today:
- `NmpWasmRuntime` wasm-bindgen struct with a working `handle_json` entry point
- Wire protocol (`WorkerRequest` / `WorkerEvent`) with proper JSON serde
- `web/chirp/src/nmp/` TypeScript layer: worker.ts, client.ts, wasmBridge.ts, snapshot.ts
- WebWorker isolation (the architecture is correct)
- Build config: `crate-type = ["cdylib", "rlib"]`

What is missing:
- Any `nmp-core` dep
- A real actor running in the WebWorker
- Relay connections (WebSocket)
- Persistent storage (IndexedDB / OPFS)
- Identity / signing (NIP-07 browser extension or nsec import)
- Snapshot output that matches the `featureSnapshotFromEnvelope` shape in snapshot.ts

---

## 2. Architecture decision: actor in a WebWorker

`nmp-core`'s actor runs on a dedicated `std::thread` using:
- `flume::Receiver::recv_timeout` (blocking read)
- `tungstenite` (raw TCP WebSocket)
- `lmdb` (memory-mapped files)
- `std::sync::Mutex` + `std::sync::Arc`

None of these compile to `wasm32-unknown-unknown`. The actor must be reimplemented
for wasm as a cooperative async task, keeping the **same actor ownership model**
(single logical thread of mutation) but using browser primitives.

**Decision: WasmActorDriver in nmp-wasm**

Rather than making nmp-core itself wasm-aware (which would add `#[cfg(target_arch = "wasm32")]`
noise throughout a 15k-LoC crate), `nmp-wasm` owns a wasm-specific actor driver that:
- Runs as a `wasm_bindgen_futures::spawn_local` task
- Replaces `flume` with `futures::channel::mpsc` (no threads, cooperative)
- Replaces `recv_timeout` with `futures::select!` over a command stream and a
  `gloo_timers::future::TimeoutFuture` for idle ticks
- Replaces `tungstenite` with `web_sys::WebSocket` callbacks wired into the mpsc

The existing `nmp-core` actor logic (relay management, subscription compilation,
event ingestion, snapshot emission) is **extracted into a pure `nmp-core-actor`
crate** (or an `nmp-core` feature) with no I/O primitives, then driven by either
the native thread driver or the wasm async driver.

**Alternative considered and rejected:** A single nmp-core that CFG-gates
everything. Rejected because: (a) the surface area of CFG gates would be enormous,
(b) it conflates the protocol logic (pure, portable) with the I/O driver (platform-
specific), and (c) it makes both targets worse. The right split is already implied
by the aim doc's "pure Rust, no FFI" description of `nmp-core`.

---

## 3. Implementation phases

### Phase 0 — Prerequisites (no new code, purely structural decisions)

**0a. ADR: wasm actor driver model**

Write `docs/decisions/0030-wasm-actor-driver.md` covering:
- Why `spawn_local` over `wasm-threads` (SharedArrayBuffer + COOP/COEP headers
  are not universally available in deployment; the cooperative model is safer default)
- `futures::channel::mpsc` as the command channel
- How idle ticks are emitted (gloo-timers 250ms timeout, same semantics as the
  native `recv_timeout`)
- The snapshot emission contract (same `KernelSnapshot` JSON shape — no new
  iOS-vs-Web divergence)

**0b. ADR: wasm storage tier**

Write `docs/decisions/0031-wasm-storage.md` covering:
- **Tier 1 (MVP):** in-memory only. `nostr_database::MemoryDatabase`. Zero storage
  deps, all events lost on tab close. Acceptable for proof-of-concept.
- **Tier 2 (v1):** OPFS-backed SQLite via `nostr-sqlite` + `rusqlite`'s
  `wasm32-unknown-unknown` target (requires `bundler` target for wasm-pack to get
  synchronous OPFS access). Events survive tab refreshes.
- **Tier 3 (post-v1):** IndexedDB via a `nostr-database` IndexedDB adapter
  (either upstream or contributed). Better browser compat than OPFS.
- Decision: ship Tier 1 in the first PR, Tier 2 before v1 cut.

**0c. CI: add `wasm32-unknown-unknown` check target**

Add `cargo check --target wasm32-unknown-unknown -p nmp-wasm` to CI.
Currently nmp-wasm builds for the native target only and the wasm32 path is
never verified. This will fail immediately because `nmp-core` will be added as
a dep and most of nmp-core doesn't compile to wasm32 yet. The CI gate exists
to track progress, not to gate merges (mark it advisory until Phase 3 lands).

---

### Phase 1 — Extract the portable kernel logic

**Goal:** Pull the pure, I/O-free kernel logic out of nmp-core into something
nmp-wasm can depend on without pulling in tungstenite / lmdb / std::thread.

**1a. Identify the pure core**

Audit `crates/nmp-core/src/kernel/` for everything that compiles to wasm32:
- `kernel/types.rs` — pure data, safe
- `kernel/ingest.rs` — event parsing, pure
- `kernel/update.rs` — diff/emit logic, pure
- `kernel/action_registry.rs` — action dispatch logic, pure
- `kernel/snapshot.rs` — snapshot assembly, pure (except projection callbacks)
- `relay_worker/` — NOT pure (tungstenite, TCP)
- `actor/` — NOT pure (std::thread, flume blocking)
- `store/` — depends on lmdb backend (NOT pure, but the `nostr-database` trait IS)

Mark each file with a `// wasm32-ok` or `// wasm32-blocked` annotation for
the initial audit PR. This audit drives the Phase 1 extraction scope.

**1b. Feature-gate the I/O layer**

Add a `native` Cargo feature to `nmp-core` (enabled by default) that gates:
- `relay_worker/` (tungstenite, native WebSocket)
- `actor/` (std::thread, flume blocking recv)
- `ffi/` (C ABI, not needed in wasm context)
- `nmp-nostr-lmdb` dep

When `native` is off, `nmp-core` compiles to wasm32 with only the kernel logic,
the substrate traits, and the nostr-database trait. This is the minimal surface
nmp-wasm needs.

**1c. nmp-wasm Cargo.toml gains nmp-core**

```toml
[dependencies]
nmp-core = { path = "../nmp-core", default-features = false }
nmp-chirp-config = { path = "../nmp-chirp-config" }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
futures = { version = "0.3", default-features = false, features = ["alloc"] }
gloo-timers = { version = "0.3", features = ["futures"] }
nostr = { workspace = true }

[target.'cfg(target_arch = "wasm32")'.dependencies]
js-sys = "0.3"
web-sys = { version = "0.3", features = ["WebSocket", "MessageEvent", ...] }
```

---

### Phase 2 — Wasm WebSocket relay transport

The native relay_worker uses raw tungstenite over TCP. The wasm relay needs
`web_sys::WebSocket`.

**2a. Define `RelaySocket` trait**

In `nmp-core` (or a new `nmp-relay-transport` crate), define a trait:

```rust
pub trait RelaySocket: Send + 'static {
    fn send_text(&self, msg: &str) -> Result<(), RelaySocketError>;
    fn close(&self);
}
```

This is the only interface the relay worker needs from the platform. The native
impl wraps tungstenite; the wasm impl wraps `web_sys::WebSocket`.

**2b. `WasmRelaySocket` in nmp-wasm**

In `crates/nmp-wasm/src/relay.rs`:

```rust
pub struct WasmRelaySocket {
    ws: web_sys::WebSocket,
    // Outbound message queue flushed on `onopen`
}
```

Inbound messages arrive via `ws.set_onmessage(...)` JS callback. These are
forwarded into the `futures::channel::mpsc` relay-event channel feeding the
wasm actor driver. The callback pattern avoids polling (D8 is not violated —
the wasm actor still advances by receiving from the channel, not by `.await`-ing
a WebSocket directly).

**2c. Relay lifecycle in the wasm actor**

The wasm actor driver mirrors the native actor's relay management loop:
- On `Start`: open WebSocket connections to each configured relay
- On relay `onopen`: send `REQ` subscriptions
- On relay `onmessage`: deserialize `["EVENT",...]` / `["EOSE",...]` / `["NOTICE",...]`
- On relay `onclose/onerror`: schedule reconnect with exponential backoff using
  `gloo_timers::future::TimeoutFuture`

The relay reconnect backoff is the same policy as native (ADR-0022).

---

### Phase 3 — Wasm actor driver

**3a. `WasmKernelDriver` in `crates/nmp-wasm/src/driver.rs`**

```rust
pub struct WasmKernelDriver {
    // The kernel logic reused from nmp-core
    kernel: KernelState,
    // Command channel (replaces flume)
    command_rx: futures::channel::mpsc::Receiver<WasmCommand>,
    // Relay event channel
    relay_rx: futures::channel::mpsc::Receiver<RelayFrame>,
    // Snapshot output callback
    on_update: js_sys::Function,
}
```

`KernelState` is the pure kernel struct from Phase 1 (event store, subscription
planner, action registry, projection registry). It contains no I/O handles.

**3b. Cooperative event loop**

```rust
pub async fn run(mut self) {
    let idle_tick = gloo_timers::future::TimeoutFuture::new(250);
    loop {
        futures::select! {
            cmd = self.command_rx.next() => self.handle_command(cmd),
            frame = self.relay_rx.next() => self.handle_relay_frame(frame),
            _ = idle_tick => self.emit_snapshot_if_dirty(),
        }
    }
}
```

Idle ticks emit snapshots at ≤60Hz when the kernel state is dirty. This matches
the native actor's emit model — no change, no snapshot.

**3c. Wire into `WasmRuntime`**

`WasmRuntime` (currently a toy) is replaced:

```rust
pub struct WasmRuntime {
    command_tx: futures::channel::mpsc::Sender<WasmCommand>,
    // snapshot state for synchronous handle() calls
    latest_snapshot: Rc<RefCell<Option<serde_json::Value>>>,
}
```

`WasmRuntime::new()` spawns the driver via `wasm_bindgen_futures::spawn_local`.
`WasmRuntime::handle()` sends a command and returns immediately with buffered events
(same fire-and-forget model the iOS/Android C ABI uses). Snapshot updates arrive
out-of-band via the `on_update` callback.

**Consequence for the TypeScript layer:** The current `WasmBridge.handle()` returns
`WorkerEvent[]` synchronously. With the real driver, most commands are fire-and-
forget and the response events arrive asynchronously. `handle_json` should be
changed to accept a response callback or return a Promise. `worker.ts` already
handles async via its `onmessage` loop, so the TypeScript side changes are minimal.

---

### Phase 4 — Identity and signing (NIP-07)

**4a. NIP-07 capability module**

The browser exposes `window.nostr` (NIP-07):
- `getPublicKey() → Promise<string>` — returns hex pubkey
- `signEvent(event) → Promise<SignedEvent>` — returns signed event

This is an async JS capability, matching the ADR-0024 async capability protocol
exactly. The wasm actor drives it like any other async capability:

1. Action executor mints a `correlation_id` and emits `WorkerEvent::CapabilityRequest`:
   ```json
   { "type": "capability_request",
     "capability": "nip07.sign",
     "correlation_id": "...",
     "payload": { "event": { ... unsigned event ... } } }
   ```
2. `worker.ts` receives the event, calls `await self.nostr.signEvent(event)`,
   posts `WorkerRequest::CapabilityResult` with the signed event back into the worker.
3. The wasm actor receives the result, routes to the pending executor.

**4b. nsec import (browser-local key)**

For users without a NIP-07 extension:
- `WorkerRequest::Start` gains an optional `nsec: Option<String>` field
- The wasm actor holds the nsec in-memory (NOT in any storage — ephemeral only)
- Signs events in-wasm using the `nostr` crate's `Keys::from_sk_str`
- Warns the user via a `WorkerEvent::Warning` that the key is ephemeral

This matches the existing `ChirpAction` pattern from the iOS side.

**4c. NIP-46 bunker (post-v1)**

NIP-46 bunker signing requires persistent relay connections. The infrastructure
(WebSocket relay transport from Phase 2) will support it, but the handshake
and session-persistence logic is deferred to a follow-on PR.

---

### Phase 5 — Snapshot output alignment

**5a. Match `featureSnapshotFromEnvelope` expectations**

`web/chirp/src/nmp/snapshot.ts` decodes the snapshot envelope into a `FeatureSnapshot`.
The wasm actor must emit the **same `KernelSnapshot` JSON shape** as the native iOS
kernel. Key projections expected:

- `projections.accounts` — `AccountLine[]`
- `projections.active_account` — string
- `projections.relay_diagnostics` — `RelayDiagnosticLine[]`
- `projections.publish_outbox` — `OutboxLine[]`
- `projections.nmp.nip17.dm_inbox` — `DmInboxProjection`
- `projections.chirp.follow_list` — follow list
- `projections.nmp.nip29.group_chat` — group chat messages
- `chirpTimeline.cards` — `ChirpEventCard[]`

The wasm actor registers these as snapshot projections (mirroring the Swift-side
`register_snapshot_projection` calls) so the output JSON shape is identical.

**5b. WireDelta for the web layer**

The native iOS layer already receives `WireDelta` updates but discards them (review #67).
For the web layer, `WireDelta` is more important — sending 50KB full snapshots to the
TypeScript layer on every keystroke is expensive. After the basic snapshot path works,
add `WorkerEvent::Delta` carrying a JSON Patch (RFC 6902) diff from the previous snapshot.
`worker.ts` applies the patch to the last full snapshot. This is a post-v1 optimization.

---

### Phase 6 — Storage persistence

**6a. In-memory baseline (ships in Phase 3 PR)**

`nostr_database::MemoryDatabase` — zero extra deps, events lost on tab close.
Acceptable for initial testing.

**6b. OPFS SQLite (v1)**

`nostr-sqlite` + `rusqlite` with `bundler` wasm target:
- Requires `wasm-pack --target bundler` (synchronous OPFS uses `Atomics.wait`,
  only available in bundler/worker context — already satisfied since nmp-wasm
  runs inside a WebWorker)
- Event store survives tab refresh
- `database_name` from `StartConfig` maps to an OPFS filename

**6c. Migration across storage tiers**

No migration needed between Tier 1 and Tier 2 — events in memory are lost on tab
close anyway. Tier 2→3 migration is a future concern.

---

### Phase 7 — Build pipeline and artifact delivery

**7a. wasm-pack build**

Add `just build-wasm` to the justfile:

```makefile
build-wasm:
    wasm-pack build crates/nmp-wasm --target web --out-dir ../../web/public/nmp-wasm
```

Output: `web/public/nmp-wasm/nmp_wasm.js` + `nmp_wasm_bg.wasm`.
The `defaultModulePath = "/nmp-wasm/nmp_wasm.js"` in `wasmBridge.ts` already
points to this location.

**7b. Vite integration**

`web/chirp/vite.config.ts` needs:
- `@rollup/plugin-wasm` or Vite's built-in `?url` import for the `.wasm` binary
- The wasm file copied to `public/nmp-wasm/` as a static asset (not bundled inline —
  the WebWorker fetches it by URL)

**7c. CI**

Add a CI job:
1. `just build-wasm` — checks the wasm build compiles
2. `npx playwright test` against `just dev` — runs the existing web tests with
   the real wasm runtime (not DegradedRuntime)

---

### Phase 8 — Protocol evolution

**8a. `WorkerRequest::CapabilityResult` path**

Currently `WasmRuntime::handle(CapabilityResult)` returns a `CapabilityFailure`
with "capability completions require a running actor". Once the real actor exists,
this routes the result to the pending executor via the command channel.

**8b. `WorkerEvent::CapabilityRequest` (new event type)**

Add to the `WorkerEvent` enum:

```rust
CapabilityRequest {
    capability: String,
    correlation_id: String,
    payload: Value,
}
```

This is the wasm-side equivalent of the native `dispatch_capability` C call.
`worker.ts` pattern-matches on this event type and dispatches to the appropriate
browser API (NIP-07, keychain placeholder, etc.).

**8c. `WorkerEvent::ActionOutcome` (new event type)**

Mirror `last_action_outcomes` from the native kernel:

```rust
ActionOutcome {
    action_type: String,
    correlation_id: String,
    success: bool,
    message: Option<String>,
}
```

Used by the TypeScript layer to drive toast notifications and spinner state —
the same gap identified in review #57 for native.

---

## 4. Full dependency graph (phases must land in order)

```
Phase 0  ──────────────────────────────────────────────────────────────────────
  0a (ADR-0030: wasm actor driver)
  0b (ADR-0031: wasm storage)
  0c (CI: wasm32 check target, advisory)

Phase 1  (requires 0a, 0b)
  1a (audit kernel for wasm32 compat)
  1b (feature-gate native I/O in nmp-core)
  1c (nmp-wasm gains nmp-core dep, cargo check passes)

Phase 2  (requires 1b)
  2a (RelaySocket trait)
  2b (WasmRelaySocket using web-sys)
  2c (relay lifecycle in wasm actor)

Phase 3  (requires 1c, 2c)
  3a (WasmKernelDriver struct)
  3b (cooperative event loop)
  3c (wire into WasmRuntime, replace toy impl)
  → First end-to-end: relay connects, timeline events appear in browser

Phase 4  (requires 3c)
  4a (NIP-07 capability: WorkerEvent::CapabilityRequest, worker.ts handler)
  4b (nsec import)

Phase 5  (requires 3c)
  5a (snapshot projection alignment with featureSnapshotFromEnvelope)

Phase 6  (requires 3c)
  6a (in-memory baseline — ships with Phase 3)
  6b (OPFS SQLite persistence)

Phase 7  (requires 3c)
  7a (just build-wasm)
  7b (Vite integration)
  7c (CI: wasm build + Playwright tests)

Phase 8  (requires 4a, 5a)
  8a (CapabilityResult routing)
  8b (WorkerEvent::CapabilityRequest)
  8c (WorkerEvent::ActionOutcome)
```

---

## 5. What does NOT need to change

- **`WasmRuntime::handle` signature** — stays `fn handle(&mut self, request: WorkerRequest) -> Result<Vec<WorkerEvent>, WasmRuntimeError>`. The sync contract is preserved; async results arrive via callback.
- **`NmpWasmRuntime` wasm-bindgen struct** — `handle_json` interface is correct. The inner `WasmRuntime` is replaced; the outer binding is untouched.
- **`wasmBridge.ts` and `worker.ts`** — the TypeScript protocol layer is well-designed. `worker.ts` needs one addition: a handler for `WorkerEvent::CapabilityRequest`. Everything else works as-is.
- **`protocol.rs`** — wire types are correct and test-covered. `WorkerEvent` gains two new variants (CapabilityRequest, ActionOutcome) but existing variants are unchanged.
- **`WorkerRequest::Start` config** — already accepts relays and database_name. Gains optional `nsec`.
- **The WebWorker isolation model** — running nmp-wasm in a dedicated WebWorker is architecturally correct and must be preserved.

---

## 6. Risks and mitigations

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| OPFS + Atomics.wait not available in target browser (Safari < 16.4) | Medium | Tier 1 in-memory fallback already planned; detect at runtime |
| `nmp-core` has hidden `std::thread` or `std::sync` deps that block wasm32 compile | High | Phase 1 audit + CI check gate before any runtime work |
| wasm binary size exceeds 5MB, breaking load time | Medium | `wasm-opt -Oz`, split nostr crypto into a separate wasm module if needed |
| NIP-07 extensions not present (most mobile browsers) | High | nsec import fallback (Phase 4b) is the unblocked path |
| WebSocket relay reconnect in wasm differs from native semantics | Low | The gloo-timers backoff mirrors native; behavior is covered by integration tests |
| `nostr` crate (secp256k1) compiles to wasm32 but is slow | Low | secp256k1 has a wasm target; performance acceptable for client signing |

---

## 7. Out of scope (explicitly deferred)

- **NIP-46 bunker signing in wasm** — infrastructure exists after Phase 2, implementation post-v1
- **WireDelta / JSON Patch** — post-v1 optimization (Phase 5b)
- **Android wasm** — separate milestone (M15)
- **Desktop wasm (Tauri)** — the Tauri renderer runs in a browser context; this plan applies unchanged
- **Service Worker for offline** — post-v1
- **Background notification decryption** — iOS/Android only; browser has no equivalent
- **wasm threads (`wasm-threads` feature, SharedArrayBuffer)** — requires COOP/COEP headers; deferred in favor of cooperative single-thread model
- **`nmp-codegen` Swift→TS code generation** — separate effort, not a prerequisite

---

## 8. Success criteria

The plan is complete when:

1. `cargo check --target wasm32-unknown-unknown -p nmp-wasm` passes in CI
2. `just build-wasm` produces a valid wasm binary
3. Opening chirp-web in a browser:
   - Shows a relay connection status (not `DegradedMode`)
   - Displays a real Nostr timeline fetched from configured relays
   - Allows publishing a note via NIP-07 (or nsec import) that appears on relay
4. `snapshot.ts`'s `featureSnapshotFromEnvelope` decodes a wasm-emitted snapshot
   into a populated `FeatureSnapshot` (not all-empty strings)
5. The `DegradedRuntime` fallback path is never hit in a successful load
6. A Playwright test exercises the golden path (start → timeline load → publish note)
   against the real wasm build (not DegradedRuntime)

---

## 9. Suggested PR sequence

| PR | Description | Phases |
|----|-------------|--------|
| #A | ADRs 0030 and 0031, CI wasm32 check (advisory) | 0a, 0b, 0c |
| #B | Audit + `native` feature-gate in nmp-core; nmp-wasm gains nmp-core dep | 1a, 1b, 1c |
| #C | `RelaySocket` trait + `WasmRelaySocket` (web-sys WebSocket) | 2a, 2b |
| #D | `WasmKernelDriver` async event loop + relay lifecycle | 2c, 3a, 3b |
| #E | Wire into `WasmRuntime`, replace toy impl; in-memory storage | 3c, 6a |
| #F | Snapshot projection alignment (FeatureSnapshot shape) | 5a |
| #G | NIP-07 capability + nsec import | 4a, 4b |
| #H | `CapabilityResult` routing, `ActionOutcome` event | 8a, 8c |
| #I | `just build-wasm`, Vite integration, CI wasm job | 7a, 7b, 7c |
| #J | OPFS SQLite persistence | 6b |

PRs A–C can land in parallel. PRs D–F must be sequential. PRs G–H can land
in parallel after E. PRs I and J can land after G.

Total estimate: 6–8 weeks of focused effort, assuming one developer.
PRs A–E (relay + actor) are the critical path.
