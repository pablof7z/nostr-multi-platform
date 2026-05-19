# FFI Surface Reference

> **Reviewed:** 2026-05-18 (T143). Pinned to `origin/master` at the commit
> when T143 landed. Previous revision (91d76b9) was stale: `capability.rs`
> and `lifecycle.rs` symbols were listed as "in-flight / pending"; `nmp-signer-broker`
> was absent; caller column still named Pulse (deleted HB50).

The kernel ships a flat `extern "C"` raw C ABI regardless of Rust module layout.
All production functions accept a `*mut NmpApp` opaque handle and return void
(or `*mut c_char` for `dispatch_capability`). **D6 invariant holds universally:**
null or invalid arguments are silent no-ops; no `Result` or exception type ever
crosses the boundary. The callers are **Chirp** (iOS, via `NmpCore.h`) and
**Android** (via `nmp-android-ffi` JNI shim which calls through Rust paths, not
direct C ABI). Pulse was deleted in HB50.

Total: **39 production** `#[no_mangle]` symbols + **2 test-support-only** symbols
(cfg-gated, never in shipping builds).

---

## 1. Lifecycle init (`ffi/mod.rs`)

| Symbol | Signature | Behavior | Callers | Threading | D6 | D7 |
|---|---|---|---|---|---|---|
| `nmp_app_new` | `() -> *mut NmpApp` | Allocate the kernel handle, spawn actor thread + update-listener thread. | Chirp, Android JNI (`nativeNew`) | Called on caller thread; returns non-null or crashes (OOM). Actor/listener run on own OS threads. | n/a — returns pointer, cannot error across FFI | n/a |
| `nmp_app_free` | `(app: *mut NmpApp)` | Reclaim handle via `Box::from_raw`; `Drop` sends `Shutdown` and joins both threads (synchronous). | Chirp deinit, Android JNI (`nativeFree`) | Synchronous on calling thread. NOT idempotent on double-free (UB). | null is no-op | n/a |
| `nmp_app_set_update_callback` | `(app, context: *mut c_void, callback: Option<fn(*mut c_void, *const c_char)>)` | Register push callback for JSON snapshot updates. `None` unregisters. | Chirp, Android JNI | Callback fires on update-listener thread. Payload `*const c_char` is valid only for the call duration — callee must copy before returning. | null app / poisoned lock → early return | D7-clean: transport only |
| `nmp_app_start` | `(app, _events_per_second: c_uint, visible_limit: c_uint, emit_hz: c_uint)` | Send `ActorCommand::Start`; clamps `visible_limit` to 1–500 (0 → default), `emit_hz` to 1–12 (0 → default). `_events_per_second` is a legacy unused ABI slot. | Chirp, Android JNI (`nativeStart`) | Fire-and-forget | null → early return | n/a |
| `nmp_app_configure` | `(app, _events_per_second: c_uint, visible_limit: c_uint, emit_hz: c_uint)` | Same as `start` but sends `ActorCommand::Configure` (hot-reconfigure without re-init). | Chirp | Fire-and-forget | null → early return | n/a |
| `nmp_app_stop` | `(app)` | Send `ActorCommand::Stop`. | Chirp, Android JNI (`nativeStop`) | Fire-and-forget | null → early return | n/a |
| `nmp_app_reset` | `(app)` | Send `ActorCommand::Reset`; clears kernel state. | Chirp | Fire-and-forget | null → early return | n/a |

---

## 2. Signer broker init (`nmp-signer-broker/src/ffi.rs`)

Separate static library (`libnmp_signer_broker.a`). D0: the broker crate
depends on both `nmp-core` and `nmp-signers`; to preserve the D0 boundary
(`nmp-core` must not depend on `nmp-signers`) the broker lives in its own
archive.

| Symbol | Signature | Behavior | Callers | Threading | D6 | D7 |
|---|---|---|---|---|---|---|
| `nmp_signer_broker_init` | `(app: *mut NmpApp)` | Construct a process-global `BunkerBroker`, register the `bunker://` hook. Idempotent — repeated calls are no-ops (the `OnceLock` is already set). Must be called once after `nmp_app_new`, before any `nmp_app_signin_bunker`. | Chirp boot | Called on caller thread; broker runs a worker thread internally. | null → early return | D7-clean: hooks a URI handler; decides no policy |
| `nmp_app_cancel_bunker_handshake` | `(app: *mut NmpApp)` | Cancel any in-flight NIP-46 handshake. Idempotent/safe when nothing is in flight. `app` arg is currently unused (kept for future per-app brokers). | Chirp | Synchronous | null → no-op (OnceLock not set) | n/a |

---

## 3. App-lifecycle callbacks (`ffi/lifecycle.rs`)

scenePhase → kernel bridge. Swift observes `@Environment(\.scenePhase)` and
calls `foreground`/`background`; the kernel decides what each phase means (D7).
`.inactive` has NO symbol — the shell silently drops it.

| Symbol | Signature | Behavior | Callers | Threading | D6 | D7 |
|---|---|---|---|---|---|---|
| `nmp_app_lifecycle_foreground` | `(app: *mut NmpApp)` | Report `scenePhase == .active`. Actor folds into `LifecyclePhase::Foreground` and fires the registered observer on a Background→Foreground (or first-after-boot) transition. Repeated calls debounce to no-op. | Chirp (`ChirpApp.onChange(scenePhase)`) | Fire-and-forget; observer fires on actor thread | null → early return | D7-clean: shell reports fact; kernel decides meaning |
| `nmp_app_lifecycle_background` | `(app: *mut NmpApp)` | Report `scenePhase == .background`. Sends `LifecyclePhase::Background`. No built-in consumer reacts today but hook is present for future policy. | Chirp | Fire-and-forget | null → early return | D7-clean |
| `nmp_app_set_lifecycle_callback` | `(app: *mut NmpApp, context: *mut c_void, callback: Option<fn(*mut c_void, u32)>)` | Register observer for meaningful phase transitions. Phase codes: `0`=Foreground, `1`=Background. `None` unregisters. Callback executes on actor thread; re-registering inside the callback is legal (mutex released before invoke). Chirp does not currently register — exposed for test harnesses and future shell consumers. | none today (declared in NmpCore.h) | Callback fires on actor thread | null app / poisoned lock → early return | D7-clean: transport only |

---

## 4. Capability socket (`ffi/capability.rs`)

Routes kernel `CapabilityRequest` JSON to a registered native handler (e.g.
Swift `KeychainCapability.handleJSON(_:)`) and returns a `CapabilityEnvelope`
JSON. This is the seam for PD-019 / T96 keychain capability.

> **STOP — not declared in `NmpCore.h`:** all three symbols exist in the Rust
> ABI but are absent from `ios/Chirp/Chirp/Bridge/NmpCore.h`. Chirp cannot
> currently call them. Header update is needed before KeychainCapability wiring
> can go live.

| Symbol | Signature | Behavior | Callers | Threading | D6 | D7 |
|---|---|---|---|---|---|---|
| `nmp_app_set_capability_callback` | `(app: *mut NmpApp, context: *mut c_void, callback: Option<fn(*mut c_void, *const c_char) -> *mut c_char>)` | Register the native capability handler. `None` unregisters. A request received while unregistered yields an error envelope, never a crash. | none today (not in header) | Synchronous registration; callback invoked on the thread that calls `dispatch_capability` | null app / poisoned lock → early return | D7-clean: socket transports envelopes, decides no policy |
| `nmp_app_dispatch_capability` | `(app: *mut NmpApp, request_json: *const c_char) -> *mut c_char` | Route a `CapabilityRequest` JSON to the registered handler, return a heap-allocated `CapabilityEnvelope` JSON string. MUST be released via `nmp_app_free_string`. Returns a populated error envelope on missing handler, malformed request, or NULL handler return — never NULL for valid app+request. | none today (not in header) | Synchronous on calling thread | Never returns NULL for non-null app+request; error is data | D7-clean: pure transport |
| `nmp_app_free_string` | `(ptr: *mut c_char)` | Release a Rust-allocated `*mut c_char` returned by `dispatch_capability`. null is a no-op. This is the ONLY symbol where Rust allocates memory the caller must free. | Callers of `dispatch_capability` | Synchronous | null → no-op | n/a |

---

## 5. Action dispatch — identity / publish / account / relay (`ffi/identity.rs`)

All fire-and-forget. Outcomes surface via snapshot fields (`last_error_toast`,
`accounts`, `publish_queue`). There is no single `nmp_dispatch` symbol — the
surface is per-verb.

| Symbol | Signature | Behavior | Callers | D6 | D7 |
|---|---|---|---|---|---|
| `nmp_app_signin_nsec` | `(app, secret: *const c_char)` | Sign in with a raw nsec key string. | Chirp | invalid → early return | n/a |
| `nmp_app_signin_bunker` | `(app, uri: *const c_char)` | Initiate NIP-46 bunker connect via `uri`. Routed through signer-broker if `nmp_signer_broker_init` was called. | Chirp | invalid → early return | n/a |
| `nmp_app_create_new_account` | `(app)` | Generate a fresh keypair and sign in. | Chirp | null → early return | n/a |
| `nmp_app_switch_active` | `(app, identity_id: *const c_char)` | Switch the active signing identity. | Chirp | invalid → early return | n/a |
| `nmp_app_remove_account` | `(app, identity_id: *const c_char)` | Remove account from the identity store. | Chirp | invalid → early return | n/a |
| `nmp_app_publish_note` | `(app, content: *const c_char, reply_to_id_or_null: *const c_char)` | Sign + publish a kind:1 note. `reply_to_id_or_null` NULL means top-level (valid, not a drop). | Chirp | null/empty content → early return; null reply is valid | n/a |
| `nmp_app_publish_unsigned_event` | `(app, unsigned_json: *const c_char)` | Sign + publish a pre-constructed `UnsignedEvent` JSON (`pubkey`, `kind`, `tags`, `content`, `created_at`). Caller's `pubkey` field is ignored — kernel derives from active identity's signing key (D7: caller cannot pick which identity signs). Malformed JSON → silent drop. | none today — exported but absent from `NmpCore.h` (STOP) | invalid/malformed → silent drop | D7-clean: signing identity is kernel's decision |
| `nmp_app_react` | `(app, target_event_id: *const c_char, reaction: *const c_char)` | Publish a kind:7 reaction. `reaction` NULL defaults to `"+"`. | Chirp | non-hex target → early return | n/a |
| `nmp_app_follow` | `(app, pubkey: *const c_char)` | Add pubkey to kind:3 follow list. Validates hex pubkey. | Chirp | non-hex → early return | n/a |
| `nmp_app_unfollow` | `(app, pubkey: *const c_char)` | Remove pubkey from kind:3 follow list. | Chirp | non-hex → early return | n/a |
| `nmp_app_add_relay` | `(app, url: *const c_char, role: *const c_char)` | Add a relay. `role` NULL defaults to `"both"`. | Chirp | null/empty url → early return | n/a |
| `nmp_app_remove_relay` | `(app, url: *const c_char)` | Remove a relay by URL. | Chirp | invalid → early return | n/a |
| `nmp_app_open_timeline` | `(app)` | Open the main timeline subscription. | Chirp, Android (via `nmp-android-ffi` Rust paths) | null → early return | n/a |

All 13 symbols: threading is fire-and-forget on calling thread; actor processes asynchronously.

---

## 6. Snapshot pull — timeline / profile interest (`ffi/timeline.rs`)

There is **no `nmp_drain_updates` pull symbol**. Snapshot delivery is push-only
via the `nmp_app_set_update_callback` registration. All timeline commands below
are fire-and-forget dispatches that cause subsequent snapshot emissions.

| Symbol | Signature | Behavior | Callers | D6 | D7 |
|---|---|---|---|---|---|
| `nmp_app_open_author` | `(app, pubkey: *const c_char)` | Register interest in an author's notes feed. Validates hex pubkey. | Chirp | non-hex → early return | n/a |
| `nmp_app_open_thread` | `(app, event_id: *const c_char)` | Register interest in an event thread. Validates hex event ID. | Chirp | non-hex → early return | n/a |
| `nmp_app_open_firehose_tag` | `(app, tag: *const c_char)` | Register interest in a hashtag firehose. Free-form string (no hex check). | Chirp | null/empty → early return | n/a |
| `nmp_app_open_uri` | `(app, uri: *const c_char)` | Route a `nostr:` URI or bare NIP-19 entity. Kernel resolves the entity and pushes `ViewOpened` or `UriRejected` via snapshot. T80/T95. | none today — exported but absent from `NmpCore.h` (STOP) | null/invalid → silent no-op | D7-clean: kernel decides routing |
| `nmp_app_claim_profile` | `(app, pubkey: *const c_char, consumer_id: *const c_char)` | Increment refcount for a profile interest. Kernel fetches and emits profile metadata while any consumer holds a claim. Validates hex pubkey. | Chirp | any invalid arg → early return | n/a |
| `nmp_app_release_profile` | `(app, pubkey: *const c_char, consumer_id: *const c_char)` | Decrement refcount. When refcount reaches zero, kernel stops fetching. Validates hex pubkey. | Chirp | any invalid arg → early return | n/a |
| `nmp_app_close_author` | `(app, pubkey: *const c_char)` | Deregister author interest. Declared in `NmpCore.h` but not called from `KernelBridge.swift` — header-declared, unwired in Swift bridge layer. | declared, not wired in Chirp | non-hex → early return | n/a |
| `nmp_app_close_thread` | `(app, event_id: *const c_char)` | Deregister thread interest. Same status as `close_author`. | declared, not wired in Chirp | non-hex → early return | n/a |

---

## 7. NIP-47 Wallet Connect (`ffi/wallet.rs`)

All fire-and-forget. Outcomes surface via snapshot `wallet_status` and
`last_error_toast` fields.

| Symbol | Signature | Behavior | Callers | D6 | D7 |
|---|---|---|---|---|---|
| `nmp_app_wallet_connect` | `(app, uri: *const c_char)` | Parse a `nostr+walletconnect://` URI, subscribe for kind:23195 responses, send initial `get_info` + `get_balance`. Replaces any existing connection. | Chirp | invalid → early return | D7-clean: caller passes URI, kernel decides protocol |
| `nmp_app_wallet_disconnect` | `(app)` | Send CLOSE to NWC relay, clear wallet state. | Chirp | null → early return | n/a |
| `nmp_app_wallet_pay_invoice` | `(app, bolt11: *const c_char, amount_msats_json: *const c_char)` | Pay a BOLT-11 invoice. `amount_msats_json` NULL uses the invoice's embedded amount. | Chirp | null/invalid bolt11 → early return | D7-clean: payment amount policy stays with caller's intent |

---

## 8. Cancellation (`nmp-signer-broker/src/ffi.rs`)

`nmp_app_cancel_bunker_handshake` — documented in section 2 (Signer broker).
No `_drop` or `_cancel` symbols exist outside the broker crate.

---

## 9. Diagnostics

No dedicated diagnostic FFI symbols exist. Telemetry for the diagnostics screen
rides on the standard update-callback JSON: the snapshot includes relay
connection state, NIP-77 reconciler counters, publish queue, and (via timeline)
profile interest refcounts. No separate diag entry point.

---

## 10. Test-support-only (`ffi/testing.rs`)

Both gated on `#[cfg(any(test, feature = "test-support"))]`. Never part of the
production ABI — shipping Swift/C never sees them. D0 gate: production code
constructs a `VerifiedEvent` only via `try_from_raw` (full Schnorr + id-hash);
`from_raw_unchecked` is accessible only through these symbols.

| Symbol | Args | VerifiedEvent path | Notes |
|---|---|---|---|
| `nmp_app_inject_pre_verified_events` | `(app, base_id_prefix: *const c_char, base_created_at: u64, count: u32)` | `from_raw_unchecked` — bypasses Schnorr (placeholder 128-zero sig). | Legacy perf-harness only. Prefer `inject_signed_events` for new harnesses. null prefix → `"stress"`. |
| `nmp_app_inject_signed_events` | `(app, base_created_at: u64, count: u32)` | `try_from_raw` — full Schnorr via `Keys::generate + EventBuilder::text_note + sign_with_keys`. | Used by S3/S4/S5 ffi-stress harness. Schnorr sign cost ~30–50 µs/event. |

---

## 11. Android JNI shim (`nmp-android-ffi/src/lib.rs`)

The JNI layer is not part of the C ABI surface — it calls the Rust-path
functions (not `extern "C"` forward-declares) so the compiler includes the
symbol bodies in the cdylib CGU. Five `extern "system"` JNI exports:

| JNI symbol | Maps to | Notes |
|---|---|---|
| `Java_org_nmp_android_KernelBridge_nativeNew` | `nmp_app_new` + channel setup | Returns `jlong` handle owning a boxed `Session`. |
| `Java_org_nmp_android_KernelBridge_nativeStart` | `nmp_app_start` | `visible_limit` + `emit_hz` passed as `jint`. |
| `Java_org_nmp_android_KernelBridge_nativeStop` | `nmp_app_stop` | — |
| `Java_org_nmp_android_KernelBridge_nativeNextUpdate` | blocks on mpsc channel (250 ms timeout) | Returns `jstring` or null on timeout/disconnect. Kotlin reader thread drains. |
| `Java_org_nmp_android_KernelBridge_nativeFree` | `nmp_app_free` + channel teardown | Clears callback before freeing `Session`; `Box::from_raw` on handle. |

---

## 12. Boundary-crossing types

| Type | Role | Allocator | Freer | Thread |
|---|---|---|---|---|
| `*mut NmpApp` | Opaque handle | Rust (`Box::into_raw` in `nmp_app_new`) | Rust (`Box::from_raw` in `nmp_app_free`; `Drop` joins threads) | Created on caller thread; actor/listener on own OS threads |
| `*const c_char` (inputs) | C string args (pubkey, uri, content, …) | Caller | Caller; Rust copies into owned `String` synchronously, never frees the C buffer | Read synchronously on calling thread |
| `*mut c_char` (output) | `dispatch_capability` return value only | Rust (`CString::into_raw`) | Caller must call `nmp_app_free_string` | Calling thread |
| `*mut c_void` | Callback context for `set_update_callback`, `set_lifecycle_callback`, `set_capability_callback` | Caller; stored as `usize`, never dereffed by Rust | Caller-owned | Passed back on the relevant callback thread |
| `c_uint` | Config scalars (`visible_limit`, `emit_hz`) | By value | n/a | Calling thread; `0` = use default |
| `UpdateCallback` | `extern "C" fn(*mut c_void, *const c_char)` | Caller supplies fn pointer | n/a | Invoked on update-listener thread; payload valid only for call duration |
| `CapabilityCallback` | `extern "C" fn(*mut c_void, *const c_char) -> *mut c_char` | Caller supplies fn pointer; return value is Rust-freed | Rust frees via `CString::from_raw` inside `dispatch_capability` | Invoked on the thread calling `dispatch_capability` |
| `LifecycleObserverFn` | `extern "C" fn(*mut c_void, u32)` | Caller supplies fn pointer | n/a | Invoked on actor thread |

---

## 13. D6 / D7 compliance audit

**D6** ("errors never cross FFI as exceptions"): all production symbols
early-return silently on invalid input; fire-and-forget `let _ = app.tx.send(...)`
discards dead-channel results. The one symbol that returns a value
(`dispatch_capability`) returns a populated error envelope — never NULL for
valid inputs, never a Rust panic or exception. D6 holds for all 39.

**D7** ("capabilities report; kernel decides"): caller-side code reports facts
(scenePhase, URI to open, pubkey to follow, BOLT-11 to pay). The kernel decides
policy (when to reconcile NIP-77, how to route relays, which identity signs).
`nmp_app_publish_unsigned_event` enforces D7 by ignoring caller-supplied
`pubkey` and deriving from the active signing identity.

| Symbol | D6 (no throw across FFI) | D7 (no policy from shell) | Notes |
|---|---|---|---|
| `nmp_app_new` | PASS (pointer or OOM) | PASS | |
| `nmp_app_free` | PASS | PASS | Double-free is UB, not a throw |
| `nmp_app_set_update_callback` | PASS | PASS | |
| `nmp_app_start` | PASS | PASS | |
| `nmp_app_configure` | PASS | PASS | |
| `nmp_app_stop` | PASS | PASS | |
| `nmp_app_reset` | PASS | PASS | |
| `nmp_signer_broker_init` | PASS | PASS | |
| `nmp_app_cancel_bunker_handshake` | PASS | PASS | |
| `nmp_app_lifecycle_foreground` | PASS | PASS | Shell reports fact; kernel decides NIP-77 trigger timing |
| `nmp_app_lifecycle_background` | PASS | PASS | |
| `nmp_app_set_lifecycle_callback` | PASS | PASS | |
| `nmp_app_set_capability_callback` | PASS | PASS | |
| `nmp_app_dispatch_capability` | PASS — error envelope, never NULL, never panic | PASS | Only transports envelopes |
| `nmp_app_free_string` | PASS | PASS | |
| `nmp_app_signin_nsec` | PASS | PASS | |
| `nmp_app_signin_bunker` | PASS | PASS | |
| `nmp_app_create_new_account` | PASS | PASS | |
| `nmp_app_switch_active` | PASS | PASS | |
| `nmp_app_remove_account` | PASS | PASS | |
| `nmp_app_publish_note` | PASS | PASS | |
| `nmp_app_publish_unsigned_event` | PASS | PASS — ignores caller pubkey, kernel decides signing identity | |
| `nmp_app_react` | PASS | PASS | |
| `nmp_app_follow` | PASS | PASS | |
| `nmp_app_unfollow` | PASS | PASS | |
| `nmp_app_add_relay` | PASS | PASS | |
| `nmp_app_remove_relay` | PASS | PASS | |
| `nmp_app_open_timeline` | PASS | PASS | |
| `nmp_app_open_author` | PASS | PASS | |
| `nmp_app_open_thread` | PASS | PASS | |
| `nmp_app_open_firehose_tag` | PASS | PASS | |
| `nmp_app_open_uri` | PASS | PASS | |
| `nmp_app_claim_profile` | PASS | PASS | |
| `nmp_app_release_profile` | PASS | PASS | |
| `nmp_app_close_author` | PASS | PASS | |
| `nmp_app_close_thread` | PASS | PASS | |
| `nmp_app_wallet_connect` | PASS | PASS | |
| `nmp_app_wallet_disconnect` | PASS | PASS | |
| `nmp_app_wallet_pay_invoice` | PASS | PASS | |

**Zero D6 violations. Zero D7 violations.**

---

## STOP findings

1. **`nmp_drain_updates` does not exist.** The task brief presumed a pull-side
   drain symbol. Snapshot delivery is push-only via `nmp_app_set_update_callback`.
   No action needed — the architecture is intentionally push.

2. **`nmp_app_set_capability_callback`, `nmp_app_dispatch_capability`,
   `nmp_app_free_string` are NOT declared in `NmpCore.h`** (`ios/Chirp/Chirp/Bridge/NmpCore.h`).
   These symbols exist in the Rust ABI and are needed for KeychainCapability
   (PD-019 / T96), but Chirp's C header is missing their declarations. The
   header must be updated before iOS can wire the keychain capability.

3. **`nmp_app_publish_unsigned_event` is NOT declared in `NmpCore.h`.**
   Exported from the Rust ABI, missing from the Swift header. Chirp cannot
   call it today.

4. **`nmp_app_open_uri` is NOT declared in `NmpCore.h`.**
   Exported from the Rust ABI (T80/T95), missing from the Swift header.

5. **`nmp_app_close_author` and `nmp_app_close_thread` are declared in
   `NmpCore.h` but not called from `KernelBridge.swift`.** The Chirp Swift
   bridge exposes `openAuthor`/`openThread` but provides no close path. Views
   that open an author or thread feed never deregister their interest —
   potential subscription leak on navigation-away. Tracked as a gap for the
   next Chirp bridge audit.
