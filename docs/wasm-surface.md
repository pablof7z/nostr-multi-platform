# WASM Surface Reference

> **Reviewed:** 2026-06-12. Sourced directly from `crates/nmp-wasm/src/`.
> This document is the single source of truth for the `crates/nmp-wasm` worker
> protocol contract. See ADR-0047 for rationale.

`crates/nmp-wasm` exposes the NMP browser runtime as a `wasm-bindgen` class
(`NmpWasmRuntime`) that a dedicated Worker instantiates. The Worker event loop
is the actor (D4): it is the single writer of kernel state in the browser host.
TypeScript renders snapshots and executes browser capabilities; Rust owns
policy, routing, replay, Nostr protocol behaviour, and state transitions.

All production functions are on the `NmpWasmRuntime` class. The wire protocol
uses two channels:

1. **JSON control channel** — `handle_json(request: string): string` (sync).
   Accepts a JSON-serialised `WorkerRequest`; returns a JSON array of control
   `WorkerEvent`s (all variants except `UpdateBytes`).
2. **Binary snapshot channel** — `set_snapshot_callback(fn: Function | null)`.
   Callback receives one `Uint8Array` argument (raw FlatBuffers `UpdateFrame`
   bytes) whenever a relay-driven kernel mutation produces a fresh snapshot.
   See §4.

There is a third entrypoint for app-level writes that need a JS Promise:
`dispatch_app_action_async(request_json: string): Promise<string>` (§3).

---

## 1. WorkerRequest — host → runtime

Serialised with `"type"` as the discriminator (`serde(tag = "type",
rename_all = "snake_case")`).

Source: `crates/nmp-wasm/src/protocol.rs` lines 6–30.

| `type` wire tag | Rust variant | Payload fields | Notes |
|---|---|---|---|
| `"hello"` | `Hello(ClientHello)` | `app_id: String`, `platform: String`, `protocol_version: u16` | Must be the first message. Protocol version must be `1`; mismatch returns `WorkerEvent::Error` with `code = "protocol_mismatch"`. |
| `"start"` | `Start(StartConfig)` | `app_id: String`, `relays: Vec<String>` (default: Chirp defaults), `relay_bootstrap: Vec<{url, role}>` (default: Chirp defaults), `database_name: String`, `correlation_id: String` | Starts the `KernelReducer`, spawns relay drivers (wasm32 only). |
| `"stop"` | `Stop` | `correlation_id: String` | Closes relay drivers, stops the kernel. |
| `"dispatch"` | `Dispatch(ActionDispatch)` | `action_type: String`, `payload: Value`, `correlation_id: String` | Generic kernel-namespaced dispatch. Routes `nmp.kernel.*` actions through `KernelReducer::reduce`. App-namespaced writes not routed through the sync path (see §3). |
| `"chirp_action"` | `AppAction(AppActionDispatch)` | `action: AppAction`, `correlation_id: String` | **Note:** wire tag is `"chirp_action"` — a residual Chirp-ism flagged for future rename (ADR-0047). On the sync path this always returns `CapabilityFailure`; use `dispatch_app_action_async` instead (§3). |
| `"capability_result"` | `CapabilityResult(CapabilityResult)` | `capability: String`, `correlation_id: String`, `payload: Value` | Browser-side capability completion. Returns `CapabilityFailure` with reason `browser_actor_driver_missing` — the native actor capability handler is not available on wasm. |
| `"set_signer"` | `SetSigner(SetSigner)` | `kind: String`, `pubkey_hex: String`, `correlation_id: String` | Install a signer. Only `kind = "nip07"` is wired. `pubkey_hex` is the result of `await window.nostr.getPublicKey()` — the host completes the async handshake before sending this request. |

### `Dispatch` kernel-namespaced action types

These `action_type` values route through `KernelReducer::reduce` on the sync path.
Source: `crates/nmp-wasm/src/dispatch_routing.rs` lines 148–176.

| `action_type` | `KernelAction` | Notes |
|---|---|---|
| `"nmp.kernel.start"` | `KernelAction::Start` | Redundant with `WorkerRequest::Start` in most host flows. |
| `"nmp.kernel.stop"` | `KernelAction::Stop` | Redundant with `WorkerRequest::Stop`. |
| `"nmp.kernel.diagnostics"` | `KernelAction::RunDiagnostics` | |
| `"nmp.kernel.open_uri"` | `KernelAction::OpenUri { uri }` | `payload.uri: String` |
| `"nmp.kernel.open_view"` | `KernelAction::OpenView { namespace, key }` | `payload.namespace: String`, `payload.key: String` |
| `"nmp.kernel.close_view"` | `KernelAction::CloseView { namespace, key }` | `payload.namespace: String`, `payload.key: String` |
| `"nmp.kernel.claim_profile"` | (claim registry, not KernelAction) | `payload.pubkey: String`, `payload.consumer_id: String` |
| `"nmp.kernel.release_profile"` | (claim registry) | `payload.pubkey: String`, `payload.consumer_id: String` |
| `"nmp.kernel.claim_event"` | (claim registry) | `payload.uri: String`, `payload.consumer_id: String` |
| `"nmp.kernel.release_event"` | (claim registry) | `payload.uri: String`, `payload.consumer_id: String` |

Any other `action_type` returns `CapabilityFailure` with `write_path_unavailable_reason`
(§3 degraded vocabulary).

---

## 2. WorkerEvent — runtime → host

Serialised with `"type"` as the discriminator (`serde(tag = "type",
rename_all = "snake_case")`). Returned as a JSON array from `handle_json`,
except `UpdateBytes` which is routed through the binary snapshot callback (§4).

Source: `crates/nmp-wasm/src/protocol.rs` lines 222–246.

| `type` wire tag | Rust variant | Payload fields | Notes |
|---|---|---|---|
| `"hello_accepted"` | `HelloAccepted` | `protocol_version: u16`, `status: RuntimeStatus` | Response to `Hello`. `status` is `"ready"`. |
| `"runtime_status"` | `RuntimeStatus` | `status: RuntimeStatus`, `correlation_id: Option<String>` | Emitted on `Start` (`"running"`) and `Stop` (`"stopped"`). `correlation_id` echoes the request. |
| `"action_accepted"` | `ActionAccepted` | `action_type: String`, `correlation_id: String` | Successful dispatch (including `SetSigner` → `action_type = "nmp.set_signer"`). |
| `"update_bytes"` | `UpdateBytes` | `bytes: Vec<u8>` | Binary FlatBuffers `UpdateFrame`. **Never appears in the JSON array returned by `handle_json`.** Routed through the snapshot callback (§4). Remains `Serialize` for native tests. |
| `"capability_failure"` | `CapabilityFailure(CapabilityFailure)` | `capability: String`, `correlation_id: String`, `reason: String` | Honest failure; host surfaces this, never hides it. `reason` starts with a stable snake_case prefix (§3). |
| `"error"` | `Error` | `code: String`, `message: String`, `correlation_id: Option<String>` | Protocol-level errors (e.g. `protocol_mismatch`, deserialisation failure). |

### RuntimeStatus vocabulary

Source: `crates/nmp-wasm/src/protocol.rs` lines 206–220.

| Serialised value | Meaning |
|---|---|
| `"ready"` | Kernel allocated, not yet started. Returned in `HelloAccepted`. |
| `"running"` | Kernel started, relay drivers active (wasm32) or in-process (native CI). Returned after `Start`. |
| `"stopped"` | Kernel stopped via `Stop`. |
| `{"degraded": "browser_actor_driver_missing"}` | Available in the protocol vocabulary for host-level degradation reporting; not currently emitted by the runtime itself (see §3 for action-level `CapabilityFailure` reason strings). |
| `{"degraded": "capability_rejected"}` | Protocol vocabulary; not currently emitted by the runtime. |
| `{"degraded": "protocol_mismatch"}` | Protocol vocabulary; not currently emitted by the runtime (the mismatch surface is `WorkerEvent::Error` with `code = "protocol_mismatch"`). |

---

## 3. Sync vs async dispatch paths

### Sync path — `handle_json`

Source: `crates/nmp-wasm/src/lib.rs` lines 71–99.

```
handle_json(request: string): Result<JsValue, JsValue>
```

Accepts a JSON-serialised `WorkerRequest`. Returns a JSON string (array of
`WorkerEvent`s). `UpdateBytes` events are drained out of the array and pushed
through the snapshot callback before `handle_json` returns. The host sees a
single binary channel for snapshot frames regardless of whether they were
produced by a relay-inbound frame or by a `Start`/`Dispatch` request.

**D6:** Returns `Err(JsValue)` only for JSON deserialisation failure (programmer
error). All runtime failures surface as `CapabilityFailure` inside the `Ok`
result — never a `JsValue` rejection on anything the user can cause.

### Async path — `dispatch_app_action_async`

Source: `crates/nmp-wasm/src/lib.rs` lines 188–215.

```
dispatch_app_action_async(request_json: string): Promise<string>
```

Accepts a JSON-serialised `AppActionDispatch` (the inner struct from
`WorkerRequest::AppAction`, without the outer `"type":"chirp_action"` wrapper).
Returns a `Promise` resolving to a JSON-serialised `WorkerEvent` —
`ActionAccepted` on success or `CapabilityFailure` for every honest failure mode.
Promise rejects only on invalid `request_json` (programmer error).

**Why a separate entrypoint:** `handle_json` is synchronous; the NIP-07 sign
step (`window.nostr.signEvent(...)`) is an async JS Promise the wasm thread
cannot block on. The async entrypoint lets the host `await` the Promise without
changing the synchronous contract of `handle_json`.

### AppAction variants

Source: `crates/nmp-wasm/src/protocol.rs` lines 97–116.
Serialised with `"action"` tag (`serde(tag = "action", rename_all = "snake_case")`).

| `action` | Payload fields | Notes |
|---|---|---|
| `"publish_note"` | `content: String`, `reply_to_id?: String` | Maps to `nmp.publish / PublishRaw { kind: 1 }`. `reply_to_id` is accepted but currently ignored (host builds NIP-10 tags). |
| `"react"` | `target_event_id: String`, `reaction?: String` (default `"+"`) | Maps to `nmp.nip25.react`. |
| `"follow"` | `pubkey: String` | Maps to `nmp.follow`. |
| `"unfollow"` | `pubkey: String` | Maps to `nmp.unfollow`. |

---

## 4. Binary snapshot callback

Source: `crates/nmp-wasm/src/lib.rs` lines 123–126;
`crates/nmp-wasm/src/runtime.rs` lines 157–159.

```
set_snapshot_callback(callback: Function | null): void
```

Install a JS callback the runtime invokes with a `Uint8Array` argument
(raw FlatBuffers `UpdateFrame` bytes) whenever kernel state changes. Install
this once at Worker boot. Passing `null` clears the slot; subsequent snapshot
frames are dropped on the synchronous path.

The callback fires on two triggers:
- **Relay-driven mutations:** an inbound `WebSocket::onmessage` fires a
  `BrowserRelayDriver` handler, which calls `KernelReducer::handle_relay_frame`,
  then pushes a snapshot via the registered callback. No timer is scheduled;
  the push fires only on relay activity.
- **Request-driven mutations:** `handle_json` drains `UpdateBytes` events from
  the handle result and routes them through the callback before returning.

In both cases the host receives snapshot frames on the same binary channel,
regardless of whether the mutation originated from the network or from a host
dispatch.

---

## 5. Degraded-mode vocabulary

The runtime surfaces honest failure reasons as stable snake_case prefix strings
in `CapabilityFailure.reason`. Hosts must pattern-match on the prefix (split on
the first `: `).

Source: `crates/nmp-wasm/src/dispatch_routing.rs` lines 112–138;
`crates/nmp-wasm/src/signer_slot.rs` lines 47–67.

| Prefix | Source function | Condition |
|---|---|---|
| `signer_not_installed` | `write_path_unavailable_reason(None)` | App-level write dispatched before `SetSigner`. Host should prompt sign-in. |
| `publish_path_not_wired` | `write_path_unavailable_reason(Some(&signer))` | Signer installed but write routed through sync `handle_json` instead of `dispatch_app_action_async`. Host integration error. |
| `browser_actor_driver_missing` | `browser_driver_missing_reason()` | `CapabilityResult` received; no native actor to route it. The wasm runtime drains the JS pending state and returns this reason. |
| `unsupported_signer_kind` | `SignerInstallError::UnsupportedKind` | `SetSigner.kind` is not `"nip07"`. Only NIP-07 is wired. |
| `invalid_signer_pubkey` | `SignerInstallError::InvalidPubkey` | `SetSigner.pubkey_hex` failed secp256k1 x-only pubkey parse. |

---

## 6. Routing decisions (diagnostic pull)

Source: `crates/nmp-wasm/src/lib.rs` lines 141–144;
`crates/nmp-wasm/src/runtime.rs` lines 481–483.

```
recent_routing_decisions(): string
```

Returns a JSON string (`schema_version: 1`) of the kernel's recent routing
decisions ring buffer. Pull-only — call on demand (e.g. debug inspector);
not pushed on every snapshot tick. Always returns a well-formed document;
empty rings render as `{"schema_version":1,"capacity":0,"publishes":[],
"subscriptions":[]}` (D6). Mirrors the iOS FFI symbol
`nmp_app_recent_routing_decisions` so a single routing-inspector renderer can
work across both surfaces.

---

## 7. Follow-on work (not yet shipped)

- **IndexedDB store.** The kernel runs in memory; state resets on page reload.
  An IndexedDB replay-log adapter feeding explicit events into the kernel is
  unimplemented.
- **NIP-46 (bunker) signer on wasm.** `SetSigner` only accepts `kind = "nip07"`.
  A wasm NIP-46 transport is a future Stage 3c follow-up.
- **`"chirp_action"` wire tag rename.** The `WorkerRequest::AppAction` variant
  serialises as `"chirp_action"` — a residual Chirp-ism. Rename to
  `"app_action"` (or a framework-namespaced equivalent) in a future
  breaking-wire-version bump.
- **Parity fixtures.** Cross-platform snapshot comparison between web, iOS,
  Android, desktop, and TUI for the same action history is unimplemented.
