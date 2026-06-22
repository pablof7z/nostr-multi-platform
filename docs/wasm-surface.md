# WASM Surface Reference

> **Reviewed:** 2026-06-22. Sourced directly from `crates/nmp-wasm/src/`.
> This document is the single source of truth for the `crates/nmp-wasm` worker
> protocol contract. See ADR-0047 (worker write/signing contract now defers to
> **ADR-0064** — the unified write/command boundary).

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

App-level **writes** ride the JSON control channel as a typed
`WorkerRequest::DispatchBytes` carrying a `DispatchEnvelope` (ADR-0064 §1; §3
below). There is no Promise write entrypoint and no wasm-only write enum: the
former `dispatch_app_action_async` / `AppAction` / `"app_action"` envelope were
deleted (#1743 Cut A). Signing is the ADR-0050 capability round-trip
(`begin_sign` → `sign_request` → `deliver_signer_response`), driven by pure
message re-entry — the reducer never awaits a persistent signer (D7/D8).

---

## 1. WorkerRequest — host → runtime

Serialised with `"type"` as the discriminator (`serde(tag = "type",
rename_all = "snake_case")`).

Source: `crates/nmp-wasm/src/protocol.rs` lines 6–30.

| `type` wire tag | Rust variant | Payload fields | Notes |
|---|---|---|---|
| `"hello"` | `Hello(ClientHello)` | `app_id: String`, `platform: String`, `protocol_version: u16` | **Host convention:** send before `Start`. The runtime enforces no ordering — `Start` without a prior `Hello` succeeds (`runtime.rs:185-223`). Protocol version must be `1`; mismatch returns `WorkerEvent::Error` with `code = "protocol_mismatch"`. |
| `"start"` | `Start(StartConfig)` | `app_id: String`, `relays: Vec<String>`, `relay_bootstrap: Vec<{url, role}>`, `database_name: String`, `correlation_id: String` | Starts the `KernelReducer`, spawns relay drivers (wasm32 only). Relay/bootstrap input is explicit host policy; the framework has no app-default fallback. |
| `"stop"` | `Stop` | `correlation_id: String` | Closes relay drivers, stops the kernel. |
| `"dispatch"` | `Dispatch(ActionDispatch)` | `action_type: String`, `payload: Value`, `correlation_id: String` | Generic kernel-namespaced JSON dispatch. Routes `nmp.kernel.*` actions through `KernelReducer::reduce`. App-namespaced writes use `dispatch_bytes` (see §3). |
| `"dispatch_bytes"` | `DispatchBytes(DispatchBytes)` | `bytes: Vec<u8>` | **ADR-0064 typed write doorway.** `bytes` are a finished `DispatchEnvelope` FlatBuffers root (file id `NMPD`) carrying `correlation_id` + generated `action_namespace` + opaque typed `payload`. Decoded through `nmp_core::dispatch_envelope::decode_dispatch_envelope` — the SAME path the native FFI `nmp_app_dispatch_action_bytes` uses. There is no wasm-only write vocabulary. |
| `"capability_result"` | `CapabilityResult(CapabilityResult)` | `capability: String`, `correlation_id: String`, `payload: Value` | Browser-side capability completion. Returns `CapabilityFailure` with reason `browser_actor_driver_missing` — the native actor capability handler is not available on wasm. |
| `"set_signer"` | `SetSigner(SetSigner)` | `kind: String`, `pubkey_hex: String`, `correlation_id: String` | Set the active identity. Only `kind = "nip07"` is wired. `pubkey_hex` is the result of `await window.nostr.getPublicKey()`. **No persistent signer is installed** (ADR-0064 §5): this only validates + canonicalizes the pubkey and seeds the kernel active account; signing is the `begin_sign` capability round-trip. |
| `"begin_sign"` | `BeginSign(BeginSign)` | `account_pubkey: String`, `unsigned_json: String` | ADR-0050 sign capability round-trip. Parks a sign op and emits `sign_request` for the main-thread broker to fulfil via `window.nostr.signEvent`. Pure message re-entry (D8). |
| `"deliver_signer_response"` | `DeliverSignerResponse(DeliverSignerResponse)` | `correlation_id: String`, `signed_json?: String`, `error?: String` | The broker delivers the signer response (success or rejection). Drives the parked op exactly once from this message handler — no polling (D8). Account-pinned. |

### `Dispatch` kernel-namespaced action types

Two routing paths serve these `action_type` values:

- **Kernel-namespaced actions** (`nmp.kernel.start` through `nmp.kernel.close_view`): `kernel_action_from_dispatch` maps them to `KernelAction` variants, then `KernelReducer::reduce` processes them. Source: `crates/nmp-wasm/src/dispatch_routing.rs` lines 148–176.
- **Claim/release actions** (`nmp.kernel.claim_*` / `nmp.kernel.release_*`): `claim_dispatch_from_action` parses the payload and routes to dedicated `KernelReducer` methods (`claim_profile`, `release_profile`, etc.) — not through `reduce`. Source: `crates/nmp-wasm/src/dispatch_routing.rs` lines 62–86.

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

## 3. The dispatch path (`handle_json`) and the typed write doorway

### `handle_json`

Source: `apps/chirp/nmp-app-chirp-web/src/wasm_binding.rs`.

```
handle_json(request: string): Result<JsValue, JsValue>
```

Accepts a JSON-serialised `WorkerRequest`. Returns a JSON string (array of
`WorkerEvent`s). `UpdateBytes` events are drained out of the array and pushed
through the snapshot callback before `handle_json` returns. The host sees a
single binary channel for snapshot frames regardless of whether they were
produced by a relay-inbound frame or by a `Start`/`Dispatch`/`DispatchBytes`
request.

**D6:** Returns `Err(JsValue)` for JSON deserialisation failure *and* for any
`WasmRuntimeError` from `WasmRuntime::handle` — concretely: `InvalidConfig`
(empty `app_id`, `database_name`, or `relays` on `Start`; relay-spawn failure
on wasm32) or `KernelContract` (unexpected `KernelUpdate` variant returned by
the pure reducer). All other runtime failures surface as `CapabilityFailure`
inside the `Ok` result — never a `JsValue` rejection on anything the user can
cause.

### The typed write doorway — `DispatchBytes`

App-level writes cross as `WorkerRequest::DispatchBytes { bytes }` where `bytes`
are a finished `DispatchEnvelope` (ADR-0064 §1). The host builds the envelope
through generated/typed builders (`web/packages/runtime-web/src/dispatchEnvelope.ts`
→ `encodeDispatchEnvelope`; the Chirp `action_namespace` lowering lives in
`web/chirp/src/nmp/actions.ts`). The `action_namespace` is a generated
discriminant — no human spells it at a call site — and is identical to the
native `ActionModule` registry key. The wasm runtime decodes the envelope
(`runtime/dispatch.rs::dispatch_bytes`) and routes by namespace; the opaque
payload is carried verbatim (the per-crate typed payload decode is the
`ActionModule`'s job).

A decode rejection (bad file identifier, schema_version tripwire mismatch,
oversize, missing routing fields) fails CLOSED with a data-shaped
`WorkerEvent::Error { code: "dispatch_envelope_rejected" }` — never a panic,
never a silent accept (D6).

**Web-preview disable.** Current web builds surface `CapabilityFailure` with the
`publish_not_supported_in_web_preview` prefix for every app-level write, because
the wasm composition root has no real `Nip65OutboxResolver`; accepting a write
would report success while publishing to zero relays. The write nonetheless
crosses through the typed envelope (not a wasm-only enum).

### Signing — the ADR-0050 capability round-trip

Signing is **not** an in-flow `Arc<dyn Signer>.await` (that path was deleted in
#1743 Cut A). A signed write is the message-driven capability round-trip:

1. The host sends `begin_sign { account_pubkey, unsigned_json }`.
2. The worker parks a sign op and emits `sign_request { correlation_id,
   account_pubkey, unsigned_json }` for the **main-thread broker** (Web Workers
   have no `window.nostr`).
3. The broker calls `window.nostr.signEvent(...)` and posts
   `deliver_signer_response { correlation_id, signed_json | error }` back.
4. The worker drives the parked op exactly once from that message handler — no
   polling, no tick-dependence (D8) — and emits `sign_completed` / `sign_failed`.

The signer backend (local key / NIP-07 / NIP-46 / NIP-55) is invisible to the
action vocabulary; the action payload carries no signer hint (V-78).

| Chirp `ChirpAction` | `action_namespace` | Notes |
|---|---|---|
| `publish_note` | `nmp.publish` | Lowers to `PublishRaw { kind: 1 }`. `reply_to_id` is host-resolved (NIP-10), not forwarded into the envelope. |
| `react` | `nmp.nip25.react` | `target_event_id` + `reaction` (default `"+"`). |
| `follow` | `nmp.follow` | `pubkey`. |
| `unfollow` | `nmp.unfollow` | `pubkey`. |

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

The callback fires on three triggers:
- **Relay-driven mutations:** an inbound `WebSocket::onmessage` fires a
  `BrowserRelayDriver` handler, which calls `KernelReducer::handle_relay_frame`,
  then pushes a snapshot via the registered callback. No timer is scheduled;
  the push fires only on relay activity.
- **Request-driven mutations:** `handle_json` drains `UpdateBytes` events from
  the handle result and routes them through the callback before returning.

In all cases the host receives snapshot frames on the same binary channel,
regardless of whether the mutation originated from the network or a host
dispatch.

---

## 5. Degraded-mode vocabulary

The runtime surfaces honest failure reasons as stable snake_case prefix strings
in `CapabilityFailure.reason`. Hosts must pattern-match on the prefix (split on
the first `: `).

Source: `crates/nmp-wasm/src/dispatch_routing.rs`;
`crates/nmp-wasm/src/signer_slot.rs`;
`crates/nmp-wasm/src/publish_path.rs`.

| Prefix | Source function | Condition |
|---|---|---|
| `signer_not_installed` | `write_path_unavailable_reason(false)` | App-level write dispatched before `SetSigner` seeded an active account. Host should prompt sign-in. |
| `publish_not_supported_in_web_preview` | `write_path_unavailable_reason(true)` → `publish_not_supported_in_web_preview_reason` | An active account is seeded but publishing is disabled in the web preview (no `OutboxResolver` wired, #1202/#1008). The single canonical "publishing disabled" prefix; hosts pattern-match it to surface an honest banner. |
| `dispatch_envelope_rejected` | `dispatch_bytes` decode | The `DispatchBytes` buffer is not a valid `DispatchEnvelope` (bad file identifier, schema_version mismatch, oversize, missing routing fields). Surfaced as `WorkerEvent::Error`, not `CapabilityFailure`. |
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

- **Web publish enablement.** The typed `DispatchBytes` write doorway currently
  returns `CapabilityFailure` with reason prefix
  `publish_not_supported_in_web_preview` for every app-level write because the
  wasm composition root lacks a real `Nip65OutboxResolver`. Enabling writes
  means installing the real composition root and wiring the per-crate typed
  payload decode + publish through the `ActionModule` registry (#1008).
- **IndexedDB store.** The kernel runs in memory; state resets on page reload.
  An IndexedDB replay-log adapter feeding explicit events into the kernel is
  unimplemented.
- **NIP-46 (bunker) / NIP-55 signer on wasm.** `SetSigner` only accepts
  `kind = "nip07"`. Other backends join as ADR-0050 sign capability fulfillers
  on the same `begin_sign` / `deliver_signer_response` round-trip.
