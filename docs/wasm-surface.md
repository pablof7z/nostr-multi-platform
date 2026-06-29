# WASM Surface Reference

> **Reviewed:** 2026-06-28. Sourced directly from
> `crates/nmp-browser-runtime/src/wasm/` and `web/packages/runtime-web/src/`.
> This document is the single source of truth for the browser worker protocol.
> `nmp-browser-runtime` owns the wasm-bindgen Worker export, worker composition,
> platform adaptation, signer registration, storage registration, the typed
> app builder (ADR-0067), and the serializable browser Worker protocol types.
> `crates/nmp-wasm` was deleted in #2202 — `nmp-browser-runtime::wasm` is the
> sole browser ABI glue.

`crates/nmp-browser-runtime` exports `NmpWasmRuntime` for the dedicated browser
Worker. That Worker event loop drives a `KernelReducer` (D4): it is the single
writer of kernel state, and composition, storage registration,
signer/capability provider registration, and app-builder policy all live in
`nmp-browser-runtime`.
TypeScript renders snapshots and executes browser capabilities; Rust owns
policy, routing, replay, Nostr protocol behaviour, and state transitions.

The `NmpWasmRuntime` class exposes three channels:

1. **JSON control channel** — `handle_json(request: string): string` (sync).
   Accepts JSON-serialised structured controls (`hello`, `start`,
   `resolve_ref`, `release_ref`, `set_identity`, signing replies, `stop`);
   returns a JSON array of control `WorkerEvent`s (all variants except
   `UpdateBytes`).
2. **Binary write channel** — `handle_dispatch_bytes(bytes: Uint8Array): string`
   (sync). Accepts a finished `DispatchEnvelope` FlatBuffers root for
   app-level writes. Browser hosts must use this method for `dispatch_bytes`
   so the byte buffer is not corrupted by JSON serialisation.
3. **Binary snapshot channel** — `set_snapshot_callback(fn: Function | null)`.
   Callback receives one `Uint8Array` argument (raw FlatBuffers `UpdateFrame`
   bytes) whenever a relay-driven kernel mutation produces a fresh snapshot.
   See §4.

App-level **writes** ride the binary write channel as a typed
`DispatchEnvelope` (ADR-0064 §1; §3 below). There is no Promise write
entrypoint and no wasm-only write enum. Signing is the ADR-0050 capability round-trip
(`begin_sign` → `sign_request` → `deliver_signer_response`), driven by pure
message re-entry; local-key sessions install a Rust signer provider and still
avoid host-side policy or polling (D7/D8).

---

## 1. WorkerRequest — host → runtime

Serialised with `"type"` as the discriminator (`serde(tag = "type",
rename_all = "snake_case")`).

Source: `crates/nmp-browser-runtime/src/wasm/protocol.rs`.

| `type` wire tag | Rust variant | Payload fields | Notes |
|---|---|---|---|
| `"hello"` | `Hello(ClientHello)` | `app_id: String`, `platform: String`, `protocol_version: u16` | **Host convention:** send before `Start`. The runtime enforces no ordering — `Start` without a prior `Hello` succeeds (`runtime.rs:185-223`). Protocol version must be `1`; mismatch returns `WorkerEvent::Error` with `code = "protocol_mismatch"`. |
| `"start"` | `Start(StartConfig)` | `app_id: String`, `relays: Vec<String>`, `relay_bootstrap: Vec<{url, role}>`, `database_name: String`, `correlation_id: String` | Starts the `KernelReducer`, spawns relay drivers (wasm32 only). Relay/bootstrap input is explicit host policy; the framework has no app-default fallback. |
| `"stop"` | `Stop` | `correlation_id: String` | Closes relay drivers, stops the kernel. |
| `"resolve_ref"` | `ResolveRef(ResolveRef)` | `namespace: u32`, `key: String`, `consumer_id: String`, `shape: u32`, `liveness: u32`, optional `hints: String[]`, optional `event_author: String`, `correlation_id: String` | ADR-0063 structured reference-resolution control. This is not an app-write doorway and cannot carry arbitrary action namespaces. Event refs may carry relay hints and nevent author metadata decoded by the app from NIP-19/NIP-21 TLVs. |
| `"release_ref"` | `ReleaseRef(ReleaseRef)` | `namespace: u32`, `key: String`, `consumer_id: String`, `correlation_id: String` | ADR-0063 structured reference release. |
| `"dispatch_bytes"` | `DispatchBytes(DispatchBytes)` | `bytes: Vec<u8>` | **ADR-0064 typed write doorway.** `bytes` are a finished `DispatchEnvelope` FlatBuffers root (file id `NMPD`) carrying `correlation_id` + generated `action_namespace` + opaque typed `payload`. Production browser hosts call `handle_dispatch_bytes(bytes)` for this request instead of JSON-stringifying the `bytes` field. Decoded through `nmp_core::dispatch_envelope::decode_dispatch_envelope` — the same dispatch envelope path native hosts use. There is no wasm-only write vocabulary. |
| `"capability_result"` | `CapabilityResult(CapabilityResult)` | `capability: String`, `correlation_id: String`, `payload: Value` | Browser-side capability completion. Returns `CapabilityFailure` with reason `browser_actor_driver_missing` — the native actor capability handler is not available on wasm. |
| `"set_identity"` | `SetIdentity(SetIdentity)` | `kind: String`, `correlation_id: String`, optional `pubkey_hex: String`, optional `secret_key_bech32: String`, optional `bunker_uri: String`, optional `identity_relays: Vec<{url, read, write}>` | Set the active identity. `kind = "nip07"` uses `pubkey_hex` from `await window.nostr.getPublicKey()` and signs through `begin_sign`/`deliver_signer_response`. `kind = "local_key"` requires `secret_key_bech32`; Rust decodes the `nsec`, derives the pubkey, installs a `LocalKeySigner`, and redacts request debug. `kind = "nip46"` requires `bunker_uri`; Rust owns the NIP-46 bunker handshake, installs the signer after `get_public_key`, and redacts the URI. `identity_relays` forwards raw identity relay permissions; Rust canonicalizes and merges them before active-account bootstrap. |
| `"begin_sign"` | `BeginSign(BeginSign)` | `account_pubkey: String`, `unsigned_json: String` | ADR-0050 sign capability round-trip. Parks a sign op and emits `sign_request` for the main-thread broker to fulfil via `window.nostr.signEvent`. Pure message re-entry (D8). |
| `"deliver_signer_response"` | `DeliverSignerResponse(DeliverSignerResponse)` | `correlation_id: String`, `signed_json?: String`, `error?: String` | The broker delivers the signer response (success or rejection). Drives the parked op exactly once from this message handler — no polling (D8). Account-pinned. |

### Structured reference controls

The worker accepts `dispatch_bytes` for app writes. A host that sends an
unknown JSON control type fails serde deserialisation and receives a protocol
error from `handle_json`.

`resolve_ref` / `release_ref` are the only JSON control messages that mutate
component reference bookkeeping:

| Request | Namespace codes | Shape/liveness |
|---|---|---|
| `resolve_ref` | `0 = profile`, `1 = event` | profile shapes: `0 = ref`, `1 = card`; event shapes: `0 = embed`, `1 = raw`; liveness: `0 = CacheOk`, `1 = Live`; optional `hints` seeds event/profile relay hints without reopening URI dispatch |
| `release_ref` | `0 = profile`, `1 = event` | no shape/liveness fields |

Unknown discriminants return `CapabilityFailure` with reason prefix
`invalid_ref_request`.

---

## 2. WorkerEvent — runtime → host

Serialised with `"type"` as the discriminator (`serde(tag = "type",
rename_all = "snake_case")`). Returned as a JSON array from `handle_json`,
except `UpdateBytes` which is routed through the binary snapshot callback (§4).

Source: `crates/nmp-browser-runtime/src/wasm/protocol.rs`.

| `type` wire tag | Rust variant | Payload fields | Notes |
|---|---|---|---|
| `"hello_accepted"` | `HelloAccepted` | `protocol_version: u16`, `status: RuntimeStatus` | Response to `Hello`. `status` is `"ready"`. |
| `"runtime_status"` | `RuntimeStatus` | `status: RuntimeStatus`, `correlation_id: Option<String>` | Emitted on `Start` (`"running"`) and `Stop` (`"stopped"`). `correlation_id` echoes the request. |
| `"action_accepted"` | `ActionAccepted` | `action_type: String`, `correlation_id: String` | Successful dispatch or control request. `SetIdentity` returns `action_type = "nmp.set_identity"`; reference controls return `nmp.kernel.resolve_ref` / `nmp.kernel.release_ref`. |
| `"update_bytes"` | `UpdateBytes` | `bytes: Vec<u8>` | Binary FlatBuffers `UpdateFrame`. **Never appears in the JSON array returned by `handle_json`.** Routed through the snapshot callback (§4). Remains `Serialize` for native tests. |
| `"capability_failure"` | `CapabilityFailure(CapabilityFailure)` | `capability: String`, `correlation_id: String`, `reason: String` | Honest failure; host surfaces this, never hides it. `reason` starts with a stable snake_case prefix (§3). |
| `"error"` | `Error` | `code: String`, `message: String`, `correlation_id: Option<String>` | Protocol-level errors (e.g. `protocol_mismatch`, deserialisation failure). |

### RuntimeStatus vocabulary

Source: `crates/nmp-browser-runtime/src/wasm/protocol.rs`.

| Serialised value | Meaning |
|---|---|
| `"ready"` | Kernel allocated, not yet started. Returned in `HelloAccepted`. |
| `"running"` | Kernel started, relay drivers active (wasm32) or in-process (native CI). Returned after `Start`. |
| `"stopped"` | Kernel stopped via `Stop`. |
| `{"degraded": "browser_actor_driver_missing"}` | Available in the protocol vocabulary for host-level degradation reporting; not currently emitted by the runtime itself (see §3 for action-level `CapabilityFailure` reason strings). |
| `{"degraded": "capability_rejected"}` | Protocol vocabulary; not currently emitted by the runtime. |
| `{"degraded": "protocol_mismatch"}` | Protocol vocabulary; not currently emitted by the runtime (the mismatch surface is `WorkerEvent::Error` with `code = "protocol_mismatch"`). |

---

## 3. Control path (`handle_json`) and the typed write doorway

### `handle_json`

Source: `crates/nmp-browser-runtime/src/wasm/mod.rs` and
`crates/nmp-browser-runtime/src/wasm/core.rs`.

```
handle_json(request: string): JsValue
```

Accepts a JSON-serialised structured-control `WorkerRequest`. Returns a JSON
string (array of `WorkerEvent`s). `UpdateBytes` events are drained out of the
array and pushed through the snapshot callback before `handle_json` returns.
The host sees a single binary channel for snapshot frames regardless of
whether they were produced by a relay-inbound frame or by a
`Start`/`ResolveRef`/`ReleaseRef` request.

**D6:** User-caused failures return data-shaped `WorkerEvent`s. JSON
deserialisation returns `Error { code: "parse_error" }`; invalid controls and
capability failures return `Error` or `CapabilityFailure` records. The JS bridge
does not need an exception path for normal user or relay behavior.

### `handle_dispatch_bytes`

Source: `crates/nmp-browser-runtime/src/wasm/mod.rs` and
`crates/nmp-browser-runtime/src/wasm/core.rs`.

```
handle_dispatch_bytes(bytes: Uint8Array): JsValue
```

Accepts the raw bytes of a finished `DispatchEnvelope` and returns the same
JSON-serialised `WorkerEvent[]` shape as `handle_json`. `UpdateBytes` events are
drained out of the array and routed through the snapshot callback before the
method returns. `web/packages/runtime-web/src/wasmBridge.ts` routes
`WorkerRequest::DispatchBytes` through this method and refuses to fall back to
`handle_json`, because `JSON.stringify(Uint8Array)` corrupts the payload into an
object shape.

### The typed write doorway — `DispatchBytes`

App-level writes cross as raw `DispatchEnvelope` bytes (ADR-0064 §1). The host
builds the envelope through generated/typed builders
(`web/packages/runtime-web/src/dispatchEnvelope.ts` → `encodeDispatchEnvelope`;
see the rebuilt web app's `action_namespace` lowering). The `action_namespace`
is a generated discriminant
— no human spells it at a call site — and is identical to the native
`ActionModule` registry key. The wasm runtime decodes the envelope
(`runtime/dispatch.rs::dispatch_bytes`) and routes by namespace; the opaque
payload is carried verbatim (the per-crate typed payload decode is the
`ActionModule`'s job).

A decode rejection (bad file identifier, schema_version tripwire mismatch,
oversize, missing routing fields) fails CLOSED with a data-shaped
`WorkerEvent::Error { code: "dispatch_envelope_rejected" }` — never a panic,
never a silent accept (D6).

With no active account, typed writes fail with `signer_not_installed`; after
`set_identity`, typed writes reach the `ActionModule` registry and the
`WasmOutboxResolver`. Malformed typed payloads fail as data-shaped
`CapabilityFailure`s from the registry/decode path.

Chirp Web's generated builder set includes `nmp.nip17.send` (`N17S`),
`nmp.nip17.hydrate_peer_relay_list` (`N17H`), and
`nmp.nip17.publish_relay_list` (`N17R`). The browser shell may pass the
selected peer pubkey, recipient pubkey, optional reply id, and draft content
into those builders, but the runtime and `nmp-nip17` own NIP-17 relay-list
lookup and interest shape, NIP-44 encryption/decryption, gift-wrap
construction, signing, explicit relay routing, and fail-closed errors. Browser
local-key sessions satisfy the required NIP-44 capability synchronously. Browser
NIP-46 sessions route `nip44_encrypt` / `nip44_decrypt` through the registered
`Nip46Signer`: the runtime parks the `SignerOp<String>` continuation, relay
re-entry delivers the NIP-46 RPC response, and the next pump resumes the
continuation. The NIP-07 signer bridge is separate: `nmp-signers` probes
`window.nostr.nip44` and exposes NIP-44 only when the extension provides both
`encrypt(pubkey, plaintext)` and `decrypt(pubkey, ciphertext)`. TypeScript and
UI code must not call `window.nostr.nip44` directly.

### Browser signer/private-flow capability model

Browser private flows are capability-shaped, not a blanket "browser signer"
promise:

| Browser signer path | NIP-44 support | NIP-17 send/private-flow result |
|---|---|---|
| `kind = "local_key"` | Supported inline in Rust through `LocalKeySigner`. | Supported when normal NIP-17 routing inputs are available. The host may collect draft text and recipient pubkey, but Rust owns NIP-44, gift-wrap construction, signing, and relay targeting. |
| `kind = "nip46"` | Supported through the browser runtime provider registry. `nip44_encrypt` / `nip44_decrypt` return pending operations, relay re-entry delivers the NIP-46 RPC response, and the next runtime pump resumes the parked continuation. Implemented by #2195 / PR #2248. | Supported when the remote signer approves the verb and normal NIP-17 routing inputs are available. Provider rejection or unsupported remote capability surfaces as runtime failure. |
| `kind = "nip07"` with `window.nostr.nip44.encrypt` and `.decrypt` | Supported by the `nmp-signers` wasm NIP-07 bridge. The signer implementation, not TypeScript UI, calls the optional extension methods. Implemented by #2247 / PR #2249. | Supported only for extensions that expose both NIP-44 verbs and return valid string results. Extension rejection, thrown JS errors, or malformed return values surface as signer/runtime failure. |
| `kind = "nip07"` without both NIP-44 verbs | Not supported. Sign-event support alone is not enough for NIP-17 private flows. | Private-message send/decrypt must fail visibly through Rust action/runtime state. The host must not add TypeScript crypto fallback or call `window.nostr.nip44` directly. |
| No active signer or unsupported signer kind | Not supported. | Typed writes fail as normal runtime capability errors such as `signer_not_installed` or `unsupported_signer_kind`. |

This same matrix is the NIP support model referenced by the Chirp Web product
spec and builder-guide signer docs.

### Signing - the ADR-0050 capability round-trip

Signing is a message-driven capability round-trip:

1. The host sends `begin_sign { account_pubkey, unsigned_json }`.
2. The worker parks a sign op and emits `sign_request { correlation_id,
   account_pubkey, unsigned_json }` for the **main-thread broker** (Web Workers
   have no `window.nostr`).
3. The broker calls `window.nostr.signEvent(...)` and posts
   `deliver_signer_response { correlation_id, signed_json | error }` back.
4. The worker drives the parked op exactly once from that message handler — no
   polling, no tick-dependence (D8) — and emits `sign_completed` / `sign_failed`.

The signer backend (local key / NIP-07 / NIP-46 / NIP-55) is invisible to the
action vocabulary; the action payload carries no signer hint (V-78). In the
browser runtime, `local_key` signers satisfy publish signing inside Rust
through the registered `LocalKeySigner`; NIP-46 bunker signers satisfy publish
signing and NIP-44 cipher operations through the Rust-owned browser NIP-46
runtime; for event signing, NIP-07 remains the main-thread `sign_request`
capability round-trip.

NIP-44 follows the same signer-owned boundary. Local-key accounts encrypt and
decrypt inline in Rust. NIP-07 accounts call the optional
`window.nostr.nip44.encrypt/decrypt` methods from the signer implementation only
when both verbs are present; missing namespace, missing verb, rejected Promise,
thrown JS error, or non-string result is surfaced as a normal signer/runtime
failure. NIP-46 accounts use signer-provider RPC. Parking and resuming pending
NIP-07/NIP-46 NIP-44 operations is owned by the browser runtime.

### Browser local-key storage policy

The web host may read a pasted `nsec` only long enough to send
`set_identity kind=local_key`. Rust must parse, zeroize the transient request
string, and hold only the in-memory signer provider for the current session.
The secret must never appear in browser storage, URL state, snapshots, action
history, debug output, or diagnostics. Reloading Chirp Web requires a new paste
until a secure-storage ADR changes this contract.

| Chirp `ChirpAction` | `action_namespace` | Notes |
|---|---|---|
| `publish_note` | `nmp.publish` | Lowers to an unsigned kind:1 publish intent. `reply_to_id` is host-resolved (NIP-10), not forwarded into the envelope. |
| `react` | `nmp.nip25.react` | `target_event_id` + `reaction` (default `"+"`). |
| `follow` | `nmp.follow` | `pubkey`. |
| `unfollow` | `nmp.unfollow` | `pubkey`. |

---

## 4. Binary snapshot callback

Source: `crates/nmp-browser-runtime/src/wasm/mod.rs` and
`crates/nmp-browser-runtime/src/wasm/core.rs`.

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
  then pushes a snapshot via the registered callback. Relay activity may arm a
  one-shot maintenance deadline, but follow-up wakes continue only when the
  reducer reports an explicit runtime deadline.
- **Request-driven mutations:** `handle_json` and `handle_dispatch_bytes` drain
  `UpdateBytes` events from the handle result and route them through the
  callback before returning.

In all cases the host receives snapshot frames on the same binary channel,
regardless of whether the mutation originated from the network or a host
dispatch.

---

## 5. Degraded-mode vocabulary

The runtime surfaces honest failure reasons as stable snake_case prefix strings
in `CapabilityFailure.reason`. Hosts must pattern-match on the prefix (split on
the first `: `).

Source: `crates/nmp-browser-runtime/src/wasm/dispatch.rs`,
`crates/nmp-browser-runtime/src/wasm/identity.rs`, and
`crates/nmp-browser-runtime/src/wasm/ref_routing.rs`.

| Prefix | Source function | Condition |
|---|---|---|
| `signer_not_installed` | `signer_not_installed_reason()` | App-level write dispatched before `SetIdentity` seeded an active account. Host should prompt sign-in. |
| `invalid_ref_request` | `invalid_ref_request_reason()` | A structured `resolve_ref` / `release_ref` control request carried an unknown namespace, shape, or liveness discriminant. |
| `dispatch_envelope_rejected` | `dispatch_bytes` decode | The `DispatchBytes` buffer is not a valid `DispatchEnvelope` (bad file identifier, schema_version mismatch, oversize, missing routing fields). Surfaced as `WorkerEvent::Error`, not `CapabilityFailure`. |
| `browser_actor_driver_missing` | `browser_driver_missing_reason()` | `CapabilityResult` received; no native actor to route it. The wasm runtime drains the JS pending state and returns this reason. |
| `unsupported_signer_kind` | `install_identity` | `SetIdentity.kind` is not `"nip07"`, `"local_key"`, or `"nip46"`. |
| invalid signer public key | `SignerInstallError::InvalidPubkey` | `SetIdentity.pubkey_hex` failed secp256k1 x-only pubkey parse. |
| `missing_local_key` | `install_identity` | `SetIdentity.kind = "local_key"` omitted `secret_key_bech32`. |
| `invalid_local_key` | `install_identity` | The supplied `secret_key_bech32` did not decode as a valid nsec. |
| `missing_nip46_bunker_uri` | `install_identity` | `SetIdentity.kind = "nip46"` omitted `bunker_uri`. |
| `invalid_nip46_bunker_uri` | `install_identity` | The supplied NIP-46 `bunker_uri` did not parse or did not contain a relay. |

---

## 6. Routing decisions (diagnostic pull)

Source: `crates/nmp-browser-runtime/src/wasm/mod.rs` and
`crates/nmp-browser-runtime/src/wasm/core.rs`.

```
recent_routing_decisions(): string
```

Returns a JSON string (`schema_version: 1`) of the kernel's recent routing
decisions ring buffer. Pull-only — call on demand (e.g. debug inspector);
not pushed on every snapshot tick. Always returns a well-formed document;
empty rings render as `{"schema_version":1,"capacity":0,"publishes":[],
"subscriptions":[]}` (D6). The equivalent iOS FFI surface is
the native debug-info helper for the routing-decisions domain, so a single
routing-inspector renderer can work across both surfaces.

---

## 7. Follow-on work (not yet shipped)

- **Web end-to-end publish completion.** The typed `DispatchBytes` doorway and
  `WasmOutboxResolver` are live. Unsigned writes still depend on the browser
  host's ADR-0050 sign round-trip and a follow-up publish of the signed event,
  plus user-visible per-relay verdicts.
- **OPFS-SQLite browser store (shipping under #1007).** The durable backend and
  the async-open-before-`Start` injection seam are live: the worker `await`s
  `prepare_store(app_id, database_name)` before dispatching `Start` so the kernel
  injects the per-app OPFS store instead of running in memory. Before opening
  OPFS, `prepare_store` attempts a Web Locks `ifAvailable` exclusive lock keyed
  by the same database name. The lock-holder is the only durable tab; non-holders
  fall back to in-memory and report `opfs_store_open_failure:
  second_tab_pool_lock` on the Tier-3 snapshot. Other open failures report the
  stable taxonomy in `crates/nmp-browser-runtime/src/wasm/store_failure.rs`
  (Safari < 17.4 / OPFS-SAH unavailable, private browsing, quota denied, handle
  loss, unknown). IndexedDB is not the chosen backend: it is async-only and
  cannot satisfy the synchronous `EventStore` contract.
- **NIP-55 signer on wasm.** Android external-signer intents are not a browser
  runtime capability. NIP-46 bunker signing and NIP-44 encrypt/decrypt routing
  are wired through `kind = "nip46"` and the browser provider registry. The
  NIP-07 `window.nostr.nip44` bridge lives in `nmp-signers` and advertises
  capability only for extensions that expose both verbs.
