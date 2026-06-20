# FFI Surface Reference

> **Reviewed:** 2026-06-13. The production C/JNI ABI lives in
> `crates/nmp-ffi`; `nmp-core` owns the actor/kernel and FlatBuffers transport
> types. Update callbacks carry binary `nmp.transport.UpdateFrame` (`NMPU`)
> frames only; the old JSON runtime snapshot path is gone.

The native runtime ships a flat `extern "C"` raw C ABI regardless of Rust module layout.
Most production functions accept a `*mut NmpApp` opaque handle and return void
(or `*mut c_char` for `dispatch_capability`). Init-only configuration symbols
return `NmpConfigStatus` codes so post-start wiring mistakes are loud while
remaining FFI-safe: `0` ok, `1` null app, `2` already started, `3` unavailable.
The callers are **Chirp** (iOS, via `NmpCore.h`) and **Android** (via
`nmp-android-ffi` JNI shim which calls through Rust paths, not direct C ABI).
Pulse was deleted in HB50.

This document describes the hand-maintained public surface. Treat exact symbol
counts as generated-check territory; the live tree exports additional app,
Android JNI, signer-broker, event-observer, snapshot-projection, and Marmot
helper symbols.

---

## 1. Lifecycle init (`nmp-ffi/src/lib.rs`)

| Symbol | Signature | Behavior | Callers | Threading | D6 | D7 |
|---|---|---|---|---|---|---|
| `nmp_app_new` | `() -> *mut NmpApp` | Allocate a passive kernel handle, command channel, and update-listener thread. The actor is spawned by the first `nmp_app_start`. | Chirp, Android JNI (`nativeNew`) | Called on caller thread; returns non-null or crashes (OOM). Listener runs on its own OS thread; actor runs only after start. | n/a — returns pointer, cannot error across FFI | n/a |
| `nmp_app_free` | `(app: *mut NmpApp)` | Reclaim handle via `Box::from_raw`; `Drop` sends `Shutdown` and joins both threads (synchronous). | Chirp deinit, Android JNI (`nativeFree`) | Synchronous on calling thread. NOT idempotent on double-free (UB). | null is no-op | n/a |
| `nmp_app_set_update_callback` | `(app, context: *mut c_void, callback: Option<fn(*mut c_void, *const u8, usize)>)` | Register push callback for FlatBuffers update frames. `None` unregisters. | Chirp, Android JNI | Callback fires on update-listener thread. Payload bytes are valid only for the call duration — callee must copy before returning. | null app / poisoned lock → early return | D7-clean: transport only |
| `nmp_app_start` | `(app, _events_per_second: c_uint, visible_limit: c_uint, emit_hz: c_uint)` | Spawn the actor on first call, then send `ActorCommand::Start`; clamps `visible_limit` to 1–500 (0 → default), `emit_hz` to 1–12 (0 → default). `_events_per_second` is a dead pre-v1 ABI slot; remove it rather than copying it into new callers (tracked in [#1609](https://github.com/pablof7z/nostr-multi-platform/issues/1609)). | Chirp, Android JNI (`nativeStart`) | Fire-and-forget | null → early return | n/a |
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
| `nmp_signer_broker_init` | `(app: *mut NmpApp) -> uint32_t` | Construct the app-scoped `BunkerBroker`, register the `bunker://` hook. Idempotent pre-start. Must be called once after `nmp_app_new`, before `nmp_app_start` and any `nmp_app_signin_bunker`. Post-start calls return `NmpConfigStatus_AlreadyStarted` and record `DroppedLateWiring` in the composition ledger. | Chirp boot, Android JNI (`nativeNew`) | Called on caller thread; broker runs a worker thread internally. | null → `NmpConfigStatus_NullApp` | D7-clean: hooks a URI handler; decides no policy |
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

These symbols exist in the Rust ABI and are declared in
`ios/Chirp/Chirp/Bridge/NmpCore.h`. Chirp registers the keychain capability
handler before `start()`.

| Symbol | Signature | Behavior | Callers | Threading | D6 | D7 |
|---|---|---|---|---|---|---|
| `nmp_app_set_capability_callback` | `(app: *mut NmpApp, context: *mut c_void, callback: Option<fn(*mut c_void, *const c_char) -> *mut c_char>)` | Register the native capability handler. `None` unregisters. A request received while unregistered yields an error envelope, never a crash. | Chirp | Synchronous registration; callback invoked on the thread that calls `dispatch_capability` | null app / poisoned lock → early return | D7-clean: socket transports envelopes, decides no policy |
| `nmp_app_dispatch_capability` | `(app: *mut NmpApp, request_json: *const c_char) -> *mut c_char` | Route a `CapabilityRequest` JSON to the registered handler, return a heap-allocated `CapabilityEnvelope` JSON string. MUST be released via `nmp_free_string`. Returns a populated error envelope on missing handler, malformed request, or NULL handler return — never NULL for valid app+request. | Chirp via `KernelBridge.registerCapabilityHandler` | Synchronous on calling thread | Never returns NULL for non-null app+request; error is data | D7-clean: pure transport |
| `nmp_free_string` | `(ptr: *mut c_char)` | Release any Rust-allocated `*mut c_char` returned by any NMP FFI function. null is a no-op (D6). This is the canonical and ONLY heap-string release symbol — replaces the retired `nmp_app_free_string` and `nmp_broker_free_string`. | All callers of FFI functions that return `*mut c_char` | Synchronous | null → no-op | n/a |

---

## 5. Action dispatch — identity / account / relay / publish control

Most command symbols are fire-and-forget. `nmp_app_dispatch_action` is the
one-door user/app action entrypoint: it returns an acceptance/error JSON string
for the enqueue step, and terminal outcomes surface later via snapshots
(`action_stages`, `last_error_toast`, `publish_queue`). The old per-verb social
and publish symbols (`nmp_app_publish_note`, `nmp_app_publish_unsigned_event`,
`nmp_app_react`, `nmp_app_follow`, `nmp_app_unfollow`) are deleted.

| Symbol | Signature | Behavior | Callers | D6 | D7 |
|---|---|---|---|---|---|
| `nmp_app_dispatch_action` | `(app, namespace: *const c_char, action_json: *const c_char) -> *mut c_char` | Validate and enqueue a namespace-keyed app/protocol action. Returns `{"correlation_id":...}` or `{"error":...}`; caller frees with `nmp_free_string`. | Chirp, TUI, Android, protocol/app modules | non-null app never returns NULL; invalid input returns error JSON | D7-clean: shell transports action data, Rust owns execution policy |
| `nmp_app_ack_action_stage` | `(app, correlation_id: *const c_char)` | Acknowledge a terminal `action_stages` entry after the host has reacted to it. | Chirp/TUI action UIs | invalid → early return | n/a |
| `nmp_app_retry_publish` | `(app, handle: *const c_char)` | Retry a failed publish by publish handle. Control-plane symbol; content publish actions still go through `dispatch_action`. | Chirp/TUI publish UI | invalid → early return | n/a |
| `nmp_app_cancel_publish` | `(app, handle: *const c_char)` | Cancel an in-flight publish by publish handle. Control-plane symbol; content publish actions still go through `dispatch_action`. | Chirp/TUI publish UI | invalid → early return | n/a |
| `nmp_app_signin_nsec` | `(app, secret: *const c_char, make_active: u8)` | Register a raw nsec signer. `make_active != 0` makes it the active account; `0` registers a secondary signer. | Chirp, TUI, Android, tests | invalid → early return | n/a |
| `nmp_app_register_agent_nsec` | `(app, secret: *const c_char)` | Register a persisted app-managed local signer. It is signable by explicit pubkey but hidden from account projections and rejected by active-account switching. | App/protocol modules with app-owned keys | invalid → early return | D7-clean: shell imports key bytes once; Rust owns role, persistence, and signing policy |
| `nmp_app_signin_bunker` | `(app, uri: *const c_char, make_active: u8)` | Initiate NIP-46 bunker connect via `uri`; the `make_active` flag is carried through the async handshake. Routed through signer-broker if `nmp_signer_broker_init` was called. | Chirp, TUI, Android | invalid → early return | n/a |
| `nmp_app_create_new_account` | `(app, profile_json: *const c_char, relays_json: *const c_char, mls: bool, make_active: u8)` | Generate a fresh keypair, publish kind:0/contact/relay metadata from supplied JSON, optionally initialize MLS, and optionally make it active. | Chirp, TUI, Android | invalid JSON/input → toast or early return | n/a |
| `nmp_app_switch_active` | `(app, identity_id: *const c_char)` | Switch the active signing identity. | Chirp | invalid → early return | n/a |
| `nmp_app_remove_account` | `(app, identity_id: *const c_char)` | Remove account from the identity store. | Chirp | invalid → early return | n/a |
| `nmp_app_add_relay` | `(app, url: *const c_char, role: *const c_char)` | Add a relay. `role` NULL defaults to `"both"`. | Chirp | null/empty url → early return | n/a |
| `nmp_app_remove_relay` | `(app, url: *const c_char)` | Remove a relay by URL. | Chirp | invalid → early return | n/a |
| `nmp_app_open_contact_feed` | `(app, primary_kinds_json: *const c_char)` | Legacy compatibility shim. Delegates to `NmpApp::declare_active_follows_feed`; current Rust app/defaults code must use the active-follows declaration method or app wrapper instead. | legacy/native callers only | null app/kinds → early return; malformed → toast | n/a |
| `nmp_app_close_contact_feed` | `(app)` | Legacy compatibility shim. Delegates to `NmpApp::clear_active_follows_feed`. | legacy/native callers only | null → silent no-op | n/a |

The active-follows feed declaration is not a raw kind-list escape hatch. The
current Rust app API is `NmpApp::declare_active_follows_feed(primary_kinds)`.
The caller supplies primary content kinds only and selects the active account's
reactive follows perspective; it never passes concrete follow pubkeys. The
protocol adapter derives repost-wrapper acquisition from those primary
declarations and rejects wrapper kinds if they are supplied as primary kinds.
`nmp-core` never stores a default "social timeline is kind:1" policy; the
primary-kind decision belongs above the kernel. Feed components that need
profiles, missing repost targets, relation counts, or other secondary data claim
those dependencies independently.

Threading: dispatch/enqueue symbols run on the calling thread and hand work to
the actor asynchronously; none wait for a state result.

---

## 6. Snapshot pull — timeline / profile interest (`ffi/timeline.rs`)

There is **no `nmp_drain_updates` pull symbol**. Snapshot delivery is push-only
via the `nmp_app_set_update_callback` registration. All timeline commands below
are fire-and-forget dispatches that cause subsequent snapshot emissions.

| Symbol | Signature | Behavior | Callers | D6 | D7 |
|---|---|---|---|---|---|
| `nmp_app_open_interest` | `(app, filter_json: *const c_char, consumer_id: *const c_char, scope: uint32_t)` | M2 (ADR-0042). Register (or attach an owner to) a generic tailing interest from a verbatim NIP-01 REQ filter. This is the raw filter escape hatch; app feed surfaces should prefer declared feeds so wrapper provenance and reactive sources stay explicit. `scope`: 0 = ActiveAccount, 1 = Global. Replaces `open_firehose_tag`; Chirp hashtag feeds now use the app-owned tag-feed seam, which declares primary `[1]`, derives NIP-18 wrapper acquisition, and opens the compiled `#t` filter at scope 1. | chirp-tui (`open_tag`); Chirp via `openInterest` | malformed filter → toast + no-op | n/a |
| `nmp_app_close_interest` | `(app, filter_json: *const c_char, consumer_id: *const c_char, scope: uint32_t)` | M2 (ADR-0042). Detach one owner from a feed interest opened with `open_interest`; drops the live sub on the last owner's close. Same filter/consumer/scope as the open. | chirp-tui; Chirp via `closeInterest` | malformed filter → no-op | n/a |
| `nmp_app_open_uri` | `(app, uri: *const c_char)` | Route a `nostr:` URI or bare NIP-19 entity. Kernel resolves the entity and pushes `ViewOpened` or `UriRejected` via snapshot. T80/T95. | declared in `NmpCore.h`; no Chirp UI caller today | null/invalid → silent no-op | D7-clean: kernel decides routing |
| `nmp_app_claim_profile` | `(app, pubkey: *const c_char, consumer_id: *const c_char, force: int, liveness: int)` | Increment refcount for a profile (kind:0) interest. Kernel registers a kind:0 `LogicalInterest` and emits metadata while any consumer holds a claim. `force != 0` bypasses the TTL freshness gate. `liveness`: `0` = CacheOk (serve from cache; OneShot fetch on miss; no live sub), non-zero = Live (Tailing kind:0 sub for reactive profile edits). Mixed claims on one pubkey resolve to Tailing. Validates hex pubkey. | Chirp | any invalid arg → early return | n/a |
| `nmp_app_release_profile` | `(app, pubkey: *const c_char, consumer_id: *const c_char)` | Decrement refcount. When refcount reaches zero, kernel stops fetching. Validates hex pubkey. | Chirp | any invalid arg → early return | n/a |

V-68 / V-112 (ADR-0042): `nmp_app_open_author`, `nmp_app_close_author`,
`nmp_app_open_thread`, and `nmp_app_close_thread` were **removed** (BREAKING,
v0.3.1). Author/thread feeds go through the generic `nmp_app_open_interest` /
`nmp_app_close_interest` pair for relay admission. Chirp's app crate composes
the view by registering app-owned FlatFeeds under `nmp.feed.author.<pubkey>` /
`nmp.feed.thread.<event_id>` and unregistering those dynamic keys on close;
profile hydration uses `nmp_app_claim_profile`.

---

## 6a. Replaceable event freshness (F-TTL)

Lazy TTL re-verification for replaceable Nostr events (kind:0 profiles, kind:10002 mailboxes, parameterized replaceables). The kernel automatically tracks when each replaceable should be re-fetched based on kind-specific TTLs. Force-refresh is exposed as a `force` argument on the existing claim functions (see §6) — **not** a standalone symbol.

There is no dedicated F-TTL symbol. The two claim functions carry a trailing `force: int`:

| Symbol | Signature | Behavior | Callers | D6 | D7 |
|---|---|---|---|---|---|
| `nmp_app_claim_profile` | `(app, pubkey: *const c_char, consumer_id: *const c_char, force: int, liveness: int)` | Refcount a kind:0 profile claim via the registry chokepoint. When the profile is cached, run the TTL freshness gate: re-verify only if `check_again_after` has elapsed (`force == 0`) or unconditionally (`force != 0`, profile screen / pull-to-refresh). `liveness`: `0` = CacheOk (feed avatars — OneShot fetch on miss, no live sub), non-zero = Live (profile screen — Tailing kind:0 sub, reactive edits). | Chirp (avatars: force=0/liveness=0; profile screen: liveness=1) | non-hex pubkey → early return | n/a |
| `nmp_app_claim_event` | `(app, uri: *const c_char, consumer_id: *const c_char, force: int)` | Refcount a `nostr:` URI claim. For cached `naddr` (addressable) identities, run the TTL gate as above; for immutable `nevent`/`note` URIs `force` is a silent no-op (no TTL record). | Chirp embed sink (force=0) | unparseable URI → early return | n/a |
| `nmp_app_release_event` | `(app, uri: *const c_char, consumer_id: *const c_char)` | Release a previously claimed `nostr:` URI. Kernel decrements the per-consumer refcount and drops the row when no consumers remain. | Chirp embed sink | invalid args → early return | n/a |

**Note:** force-refresh replaces the removed `nmp_app_refresh_replaceable` symbol (ADR-0041). `force != 0` is semantically "treat `check_again_after` as 0 for this claim", driving an immediate re-verification REQ. TTL management is otherwise transparent: the framework auto-re-verifies after kind-specific timeouts (default: kind:0 = 1h, kind:10002 = 6h).

See also: `docs/design/replaceable-freshness.md` (F-TTL design + lifecycle).

---

## 8. NIP-47 Wallet Connect (`ffi/wallet.rs`)

All fire-and-forget. Outcomes surface via snapshot `wallet_status` and
`last_error_toast` fields.

| Symbol | Signature | Behavior | Callers | D6 | D7 |
|---|---|---|---|---|---|
| `nmp_app_wallet_connect` | `(app, uri: *const c_char)` | Parse a `nostr+walletconnect://` URI, subscribe for kind:23195 responses, send initial `get_info` + `get_balance`. Replaces any existing connection. | Chirp | invalid → early return | D7-clean: caller passes URI, kernel decides protocol |
| `nmp_app_wallet_disconnect` | `(app)` | Send CLOSE to NWC relay, clear wallet state. | Chirp | null → early return | n/a |
| `nmp_app_wallet_pay_invoice` | `(app, bolt11: *const c_char, amount_msats_json: *const c_char)` | Pay a BOLT-11 invoice. `amount_msats_json` NULL uses the invoice's embedded amount. | Chirp | null/invalid bolt11 → early return | D7-clean: payment amount policy stays with caller's intent |

---

## 9. Cancellation (`nmp-signer-broker/src/ffi.rs`)

`nmp_app_cancel_bunker_handshake` — documented in section 2 (Signer broker).
No `_drop` or `_cancel` symbols exist outside the broker crate.

---

## 10. Diagnostics

No dedicated diagnostic FFI symbols exist. Telemetry for the diagnostics screen
rides on the standard update-callback FlatBuffers frame: the snapshot envelope
and typed projection sidecars include relay connection state, NIP-77 reconciler
counters, publish queue, and profile interest refcounts. No separate diag entry
point.

---

## 11. Test-support-only (`nmp-ffi/src/testing.rs`)

Both gated on `#[cfg(any(test, feature = "test-support"))]`. Never part of the
production ABI — shipping Swift/C never sees them. D0 gate: production code
constructs a `VerifiedEvent` only via `try_from_raw` (full Schnorr + id-hash);
`from_raw_unchecked` is accessible only through these symbols.

| Symbol | Args | VerifiedEvent path | Notes |
|---|---|---|---|
| `nmp_app_inject_pre_verified_events` | `(app, base_id_prefix: *const c_char, base_created_at: u64, count: u32)` | `from_raw_unchecked` — bypasses Schnorr (placeholder 128-zero sig). | Legacy perf-harness only. Prefer `inject_signed_events` for new harnesses. null prefix → `"stress"`. |
| `nmp_app_inject_signed_events` | `(app, base_created_at: u64, count: u32)` | `try_from_raw` — full Schnorr via `Keys::generate + EventBuilder::text_note + sign_with_keys`. | Used by S3/S4/S5 ffi-stress harness. Schnorr sign cost ~30–50 µs/event. |

---

## 12. Android JNI shim (`nmp-android-ffi/src/lib.rs`)

The JNI layer is not part of the C ABI surface — it calls the Rust-path
functions (not `extern "C"` forward-declares) so the compiler includes the
symbol bodies in the cdylib CGU. Update delivery is push-only: Android registers
a listener object and Rust invokes `onUpdate(ByteArray)` from the kernel
update-listener thread after copying the borrowed FlatBuffers frame.

| JNI symbol | Maps to | Notes |
|---|---|---|
| `Java_org_nmp_android_KernelBridge_nativeNew` | `nmp_app_new` + channel setup | Returns `jlong` handle owning a boxed `Session`. |
| `Java_org_nmp_android_KernelBridge_nativeStart` | `nmp_app_start` | `visible_limit` + `emit_hz` passed as `jint`. |
| `Java_org_nmp_android_KernelBridge_nativeStop` | `nmp_app_stop` | — |
| `Java_org_nmp_android_KernelBridge_nativeSetUpdateListener` | `nmp_app_set_update_callback` via `Session::set_push_listener` | Registers a Kotlin listener. Frames are pushed as `ByteArray`; pass `null` to deregister. |
| `Java_org_nmp_android_KernelBridge_nativeClearUpdateListener` | clear push listener | Clears the listener without freeing the session. |
| `Java_org_nmp_android_KernelBridge_nativeFree` | `nmp_app_free` + channel teardown | Clears callback before freeing `Session`; `Box::from_raw` on handle. |

---

## 13. Boundary-crossing types

| Type | Role | Allocator | Freer | Thread |
|---|---|---|---|---|
| `*mut NmpApp` | Opaque handle | Rust (`Box::into_raw` in `nmp_app_new`) | Rust (`Box::from_raw` in `nmp_app_free`; `Drop` joins started actor + listener) | Created on caller thread; listener starts at allocation, actor starts on first `nmp_app_start` |
| `*const c_char` (inputs) | C string args (pubkey, uri, content, …) | Caller | Caller; Rust copies into owned `String` synchronously, never frees the C buffer | Read synchronously on calling thread |
| `*mut c_char` (output) | Return value of any FFI function that yields a heap string | Rust (`CString::into_raw`) | Caller must call `nmp_free_string` | Calling thread |
| `*mut c_void` | Callback context for `set_update_callback`, `set_lifecycle_callback`, `set_capability_callback` | Caller; stored as `usize`, never dereffed by Rust | Caller-owned | Passed back on the relevant callback thread |
| `c_uint` | Config scalars (`visible_limit`, `emit_hz`) | By value | n/a | Calling thread; `0` = use default |
| `UpdateCallback` | `extern "C" fn(*mut c_void, *const u8, usize)` | Caller supplies fn pointer | n/a | Invoked on update-listener thread; FlatBuffers payload valid only for call duration |
| `CapabilityCallback` | `extern "C" fn(*mut c_void, *const c_char) -> *mut c_char` | Caller supplies fn pointer; return value is Rust-freed | Rust frees via `CString::from_raw` inside `dispatch_capability` | Invoked on the thread calling `dispatch_capability` |
| `LifecycleObserverFn` | `extern "C" fn(*mut c_void, u32)` | Caller supplies fn pointer | n/a | Invoked on actor thread |

---

## 14. D6 / D7 compliance audit

**D6** ("errors never cross FFI as exceptions"): all production symbols
early-return silently on invalid input; fire-and-forget `let _ = app.tx.send(...)`
discards dead-channel results. The one symbol that returns a value
(`dispatch_capability`) returns a populated error envelope — never NULL for
valid inputs, never a Rust panic or exception. D6 holds for the production
surface documented here.

**D7** ("capabilities report; kernel decides"): caller-side code reports facts
(scenePhase, URI to open, pubkey to follow, BOLT-11 to pay). The kernel decides
policy (when to reconcile NIP-77, how to route relays, which identity signs).
User-authored publish/social actions enter through `nmp_app_dispatch_action`;
the Rust action modules derive signing identity and routing policy.

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
| `nmp_free_string` | PASS | PASS | |
| `nmp_app_signin_nsec` | PASS | PASS | |
| `nmp_app_signin_bunker` | PASS | PASS | |
| `nmp_app_create_new_account` | PASS | PASS | |
| `nmp_app_switch_active` | PASS | PASS | |
| `nmp_app_remove_account` | PASS | PASS | |
| `nmp_app_dispatch_action` | PASS — acceptance/error JSON, never NULL for non-null app | PASS | Single user/app action door |
| `nmp_app_ack_action_stage` | PASS | PASS | |
| `nmp_app_retry_publish` | PASS | PASS | Publish lifecycle control |
| `nmp_app_cancel_publish` | PASS | PASS | Publish lifecycle control |
| `nmp_app_add_relay` | PASS | PASS | |
| `nmp_app_remove_relay` | PASS | PASS | |
| `nmp_app_open_contact_feed` | PASS | PASS | Legacy shim to `NmpApp::declare_active_follows_feed`; not the current app-facing primitive |
| `nmp_app_close_contact_feed` | PASS | PASS | Legacy shim to `NmpApp::clear_active_follows_feed` |
| `nmp_app_open_interest` | PASS | PASS | M2 (ADR-0042) — generic feed-subscription replacement for `open_firehose_tag` and the removed `open_author`/`open_thread` |
| `nmp_app_close_interest` | PASS | PASS | M2 (ADR-0042) |
| `nmp_app_open_uri` | PASS | PASS | |
| `nmp_app_claim_profile` | PASS | PASS | |
| `nmp_app_release_profile` | PASS | PASS | |
| `nmp_app_claim_event` | PASS | PASS | |
| `nmp_app_release_event` | PASS | PASS | |
| `nmp_app_wallet_connect` | PASS | PASS | |
| `nmp_app_wallet_disconnect` | PASS | PASS | |
| `nmp_app_wallet_pay_invoice` | PASS | PASS | |

**Zero D6 violations. Zero D7 violations.**

---

## Current findings

1. **`nmp_drain_updates` does not exist.** The task brief presumed a pull-side
   drain symbol. Snapshot delivery is push-only via `nmp_app_set_update_callback`.
   No action needed — the architecture is intentionally push.

2. **This reference still needs a generated symbol audit.** The live tree now
   exports more symbols than the old T143 count, including app-specific Chirp,
   Marmot, event observer, raw tap, action dispatch, and snapshot projection
   helpers.

3. **RESOLVED (V-68 / V-112, ADR-0042):** `nmp_app_open_author`,
   `nmp_app_close_author`, `nmp_app_open_thread`, and `nmp_app_close_thread`
   were removed in v0.3.1. The prior open-without-close subscription-leak gap
   is structurally closed: the generic `nmp_app_open_interest` /
   `nmp_app_close_interest` pair is symmetric and refcounted in the planner's
   interest registry.
