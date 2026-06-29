# FFI Surface Reference

> **Reviewed:** 2026-06-29. The current transitional C/JNI ABI lives in
> `crates/nmp-ffi`; `nmp-ffi` delegates runtime ownership to
> `nmp-native-runtime` (ADR-0068). `nmp-core` owns the actor/kernel
> and FlatBuffers transport types. Update callbacks carry binary
> `nmp.transport.UpdateFrame` (`NMPU`) frames only; the old JSON runtime snapshot
> path is gone.
>
> **M14 / ADR-0030 target:** native hosts should converge on one public UniFFI
> binding surface. UniFFI carries the object/verb/callback shape; FlatBuffers
> (`NMPD` action envelopes and `NMPU` update frames) remain the byte payloads.
> Browser/wasm is not part of this native ABI consolidation because its public
> runtime surface is `wasm-bindgen` in `nmp-browser-runtime`.
>
> **M14-0 proof (issue #2129, 2026-06-26):** The Android app-loop lane in Chirp
> migrated from JNI to UniFFI before Chirp was extracted to
> `github.com/pablof7z/chirp` (#2295/#2303). That proof showed UniFFI can carry
> lifecycle, update callbacks, and `Vec<u8>`/`ByteArray` FlatBuffers payloads.
> The proof artifacts are no longer in this repository.

The current in-tree native delivery surface ships a flat `extern "C"` raw C ABI
regardless of Rust module layout. Those symbols are implemented in `nmp-ffi`,
which is now the ABI shell over `nmp-native-runtime`: the native runtime owns the
handle, actor lifecycle, runtime slots, and typed Rust builder. Treat this as the
current compatibility surface while #2125 is open, not as the long-term native
public API.
Most production functions accept a `*mut NmpApp` opaque handle and return void
(or `*mut c_char` for `dispatch_capability`). Init-only configuration symbols
return `NmpConfigStatus` codes so post-start wiring mistakes are loud while
remaining FFI-safe: `0` ok, `1` null app, `2` already started, `3` unavailable.
In-tree callers include native app/staticlib crates such as Gallery. External
apps may also consume this surface until their binding layer moves to UniFFI.
Pulse was deleted in HB50, and Chirp's platform bridge code now lives in the
external Chirp repository.

This document describes the hand-maintained public surface. Treat exact symbol
counts as generated-check territory; the live tree exports additional app,
Android JNI, NIP-46 actor-lane, event-observer, snapshot-projection, and Marmot
helper symbols.

---

## 1. Lifecycle init (`nmp-ffi/src/lib.rs`)

| Symbol | Signature | Behavior | Callers | Threading | D6 | D7 |
|---|---|---|---|---|---|---|
| `nmp_app_new` | `() -> *mut NmpApp` | Allocate a passive kernel handle, command channel, and update-listener thread. The actor is spawned by the first `nmp_app_start`. | Native C/JNI consumers | Called on caller thread; returns non-null or crashes (OOM). Listener runs on its own OS thread; actor runs only after start. | n/a — returns pointer, cannot error across FFI | n/a |
| `nmp_app_free` | `(app: *mut NmpApp)` | Reclaim handle via `Box::from_raw`; `Drop` sends `Shutdown` and joins both threads (synchronous). | Native C/JNI consumers | Synchronous on calling thread. NOT idempotent on double-free (UB). | null is no-op | n/a |
| `nmp_app_set_update_callback` | `(app, context: *mut c_void, callback: Option<fn(*mut c_void, *const u8, usize)>)` | Register push callback for FlatBuffers update frames. `None` unregisters. | Native C/JNI consumers | Callback fires on update-listener thread. Payload bytes are valid only for the call duration — callee must copy before returning. | null app / poisoned lock → early return | D7-clean: transport only |
| `nmp_app_start` | `(app, visible_limit: c_uint, emit_hz: c_uint)` | Spawn the actor on first call, then send `ActorCommand::Start`; clamps `visible_limit` to 1–500 (0 → default), `emit_hz` to 1–12 (0 → default). | Native C/JNI consumers | Fire-and-forget | null → early return | n/a |
| `nmp_app_configure` | `(app, visible_limit: c_uint, emit_hz: c_uint)` | Same as `start` but sends `ActorCommand::Configure` (hot-reconfigure without re-init). | Native C/JNI consumers | Fire-and-forget | null → early return | n/a |
| `nmp_app_stop` | `(app)` | Send `ActorCommand::Stop`. | Native C/JNI consumers | Fire-and-forget | null → early return | n/a |
| `nmp_app_reset` | `(app)` | Send `ActorCommand::Reset`; clears kernel state. | Native C/JNI consumers | Fire-and-forget | null → early return | n/a |

---

## 2. NIP-46 actor-lane ABI entrypoints (`nmp-ffi/src/signer_broker.rs`)

PR-B2 (#2119): `nmp-signer-broker` is deleted. NIP-46 is now driven through
the actor-relay lane (`nmp-nip46-runtime`) — the same shared relay socket the
kernel uses for all outbound Nostr traffic. D0 is preserved: `nmp-core` still
does not depend on `nmp-signers`; native C callers reach the lane through
`nmp-ffi` symbols (above `nmp-core` in the DAG) behind the `signer-broker` cargo
feature. Runtime ownership lives in `nmp-native-runtime`; `nmp-ffi` exposes the
C-ABI entrypoints and marshals native-runtime state.

| Symbol | Signature | Behavior | Callers | Threading | D6 | D7 |
|---|---|---|---|---|---|---|
| `nmp_signer_broker_init` | `(app: *mut NmpApp) -> uint32_t` | Register the NIP-46 actor-lane runtime on `app`: installs a `Nip46Interceptor` + `Nip46ConnectedHook` and a per-app bunker hook. Idempotent pre-start. Must be called once after `nmp_app_new`, before `nmp_app_start`. Post-start calls return `NmpConfigStatus_AlreadyStarted`. | Native C/JNI consumers with signer-broker feature | Called on caller thread; all I/O routes through the actor's relay-worker thread. | null → `NmpConfigStatus_NullApp` | D7-clean: hooks a URI handler; decides no policy |
| `nmp_app_cancel_bunker_handshake` | `(app: *mut NmpApp)` | Cancel any in-flight NIP-46 handshake. Clears the runtime and unregisters the persistent subscription on each relay. Idempotent. | Native C/JNI consumers | Synchronous, posts actor commands | null → no-op | n/a |

---

## 3. App-lifecycle callbacks (`ffi/lifecycle.rs`)

scenePhase → kernel bridge. Swift observes `@Environment(\.scenePhase)` and
calls `foreground`/`background`; the kernel decides what each phase means (D7).
`.inactive` has NO symbol — the shell silently drops it.

| Symbol | Signature | Behavior | Callers | Threading | D6 | D7 |
|---|---|---|---|---|---|---|
| `nmp_app_lifecycle_foreground` | `(app: *mut NmpApp)` | Report `scenePhase == .active`. Actor folds into `LifecyclePhase::Foreground` and fires the registered observer on a Background→Foreground (or first-after-boot) transition. Repeated calls debounce to no-op. | Native C/JNI lifecycle adapters | Fire-and-forget; observer fires on actor thread | null → early return | D7-clean: shell reports fact; kernel decides meaning |
| `nmp_app_lifecycle_background` | `(app: *mut NmpApp)` | Report `scenePhase == .background`. Sends `LifecyclePhase::Background`. No built-in consumer reacts today but hook is present for future policy. | Native C/JNI lifecycle adapters | Fire-and-forget | null → early return | D7-clean |
| `nmp_app_set_lifecycle_callback` | `(app: *mut NmpApp, context: *mut c_void, callback: Option<fn(*mut c_void, u32)>)` | Register observer for meaningful phase transitions. Phase codes: `0`=Foreground, `1`=Background. `None` unregisters. Callback executes on actor thread; re-registering inside the callback is legal (mutex released before invoke). Exposed for test harnesses and shell consumers that need a native lifecycle observer. | Native C/JNI lifecycle adapters | Callback fires on actor thread | null app / poisoned lock → early return | D7-clean: transport only |

---

## 4. Capability socket (`ffi/capability.rs`)

Routes kernel `CapabilityRequest` JSON to a registered native handler (e.g.
Swift `KeychainCapability.handleJSON(_:)`) and returns a `CapabilityEnvelope`
JSON. This is the seam for PD-019 / T96 keychain capability.

These symbols exist in the Rust ABI and are declared by native bridge headers or
JNI adapters where used. The shell registers its raw capability handler before
`start()`.

| Symbol | Signature | Behavior | Callers | Threading | D6 | D7 |
|---|---|---|---|---|---|---|
| `nmp_app_set_capability_callback` | `(app: *mut NmpApp, context: *mut c_void, callback: Option<fn(*mut c_void, *const c_char) -> *mut c_char>)` | Register the native capability handler. `None` unregisters. A request received while unregistered yields an error envelope, never a crash. | Native capability bridge | Synchronous registration; callback invoked on the thread that calls `dispatch_capability` | null app / poisoned lock → early return | D7-clean: socket transports envelopes, decides no policy |
| `nmp_app_dispatch_capability` | `(app: *mut NmpApp, request_json: *const c_char) -> *mut c_char` | Route a `CapabilityRequest` JSON to the registered handler, return a heap-allocated `CapabilityEnvelope` JSON string. MUST be released via `nmp_free_string`. Returns a populated error envelope on missing handler, malformed request, or NULL handler return — never NULL for valid app+request. | Native capability bridge | Synchronous on calling thread | Never returns NULL for non-null app+request; error is data | D7-clean: pure transport |
| `nmp_free_string` | `(ptr: *mut c_char)` | Release any Rust-allocated `*mut c_char` returned by any NMP FFI function. null is a no-op (D6). This is the canonical and ONLY heap-string release symbol. | All callers of FFI functions that return `*mut c_char` | Synchronous | null → no-op | n/a |

---

## 5. Action dispatch — identity / account / relay / publish control

Most command symbols are fire-and-forget. `nmp_app_dispatch_action_bytes` is the
production one-door user/app action entrypoint: it accepts a FlatBuffers
`DispatchEnvelope`, returns an acceptance/error JSON string for the enqueue
step, and terminal outcomes surface later via snapshots (`action_stages`,
`last_error_toast`, `publish_queue`). Per-verb social and publish symbols are
not part of the production surface.

| Symbol | Signature | Behavior | Callers | D6 | D7 |
|---|---|---|---|---|---|
| `nmp_app_dispatch_action_bytes` | `(app, ptr: *const u8, len: usize) -> *mut c_char` | Validate a `DispatchEnvelope`, route by action namespace, and enqueue a typed app/protocol action. Returns `{"correlation_id":...}` or `{"error":...}`; caller frees with `nmp_free_string`. | Native/TUI/protocol app modules | non-null app never returns NULL; invalid input returns error JSON | D7-clean: shell transports action data, Rust owns execution policy |
| `nmp_app_ack_action_stage` | `(app, correlation_id: *const c_char)` | Acknowledge a terminal `action_stages` entry after the host has reacted to it. | Native/TUI action UIs | invalid → early return | n/a |
| `nmp_app_retry_publish` | `(app, handle: *const c_char)` | Retry a failed publish by publish handle. Control-plane symbol; content publish actions still go through the byte action doorway. | Native/TUI publish UI | invalid → early return | n/a |
| `nmp_app_cancel_action` | `(app, correlation_id: *const c_char)` | Cancel an in-flight action by host-supplied `correlation_id`. The kernel reverse-resolves the publish handle so the `Cancelled` terminal lands under the original correlation_id (PD-036). (`nmp_app_cancel_publish` is deleted.) | Native/TUI publish UI | invalid → early return | n/a |
| `nmp_app_signin_nsec` | `(app, secret: *const c_char, make_active: u8)` | Register a raw nsec signer. `make_active != 0` makes it the active account; `0` registers a secondary signer. | Native/TUI/tests | invalid → early return | n/a |
| `nmp_app_register_agent_nsec` | `(app, secret: *const c_char)` | Register a persisted app-managed local signer. It is signable by explicit pubkey but hidden from account projections and rejected by active-account switching. | App/protocol modules with app-owned keys | invalid → early return | D7-clean: shell imports key bytes once; Rust owns role, persistence, and signing policy |
| `nmp_app_signin_bunker` | `(app, uri: *const c_char, make_active: u8)` | Initiate NIP-46 bunker connect via `uri`; the `make_active` flag is carried through the async handshake. Driven by the NIP-46 actor-lane runtime over the shared relay lane when `nmp_signer_broker_init` was called. | Native/TUI | invalid → early return | n/a |
| `nmp_app_create_new_account` | `(app, profile_json: *const c_char, relays_json: *const c_char, mls: bool, make_active: u8)` | Generate a fresh keypair, publish kind:0/contact/relay metadata from supplied JSON, optionally initialize MLS, and optionally make it active. | Native/TUI | invalid JSON/input → toast or early return | n/a |
| `nmp_app_switch_active` | `(app, identity_id: *const c_char)` | Switch the active signing identity. | Native account UI | invalid → early return | n/a |
| `nmp_app_remove_account` | `(app, identity_id: *const c_char)` | Remove account from the identity store. | Native account UI | invalid → early return | n/a |
| `nmp_app_add_relay` | `(app, url: *const c_char, role: *const c_char)` | Add a relay. `role` NULL defaults to `"both"`. | Native relay UI | null/empty url → early return | n/a |
| `nmp_app_remove_relay` | `(app, url: *const c_char)` | Remove a relay by URL. | Native relay UI | invalid → early return | n/a |

> **#1740 step 8 — RETIRED:** the raw `nmp_app_open_contact_feed` /
> `nmp_app_close_contact_feed` C-ABI active-follows shims are DELETED. The only
> live feed doorway is the typed feed-session pair below: open with serialized
> `FeedParams`, close with the returned serialized `FeedHandle`. Native hosts
> do not hand-author relay filters for product feeds. The
> `NmpApp::declare_active_follows_feed` / `clear_active_follows_feed` Rust
> methods are also DELETED; active-follows is one ReducedSource instance, not a
> helper verb.

The active-follows feed declaration is not a raw kind-list escape hatch.
App/protocol composition code opens it through Rust typed `FeedParams` with
`FeedScope::ActiveUserFollows`. The source compiler reduces that source into
lower-level child interests and recompiles them when the active account or source
event changes. The caller supplies primary content kinds only and never passes
concrete follow pubkeys. Protocol adapters derive repost-wrapper acquisition
from those primary declarations and reject wrapper kinds if they are supplied as
primary kinds. `nmp-core` never stores a default "social timeline is kind:1"
policy; the primary-kind decision belongs above the kernel. Feed components that
need profiles, missing repost targets, relation counts, or other secondary data
claim those dependencies independently.

Threading: dispatch/enqueue symbols run on the calling thread and hand work to
the actor asynchronously; none wait for a state result.

---

## 6. Snapshot pull — typed feed / timeline (`ffi/feed.rs`, `ffi/timeline.rs`)

There is **no `nmp_drain_updates` pull symbol**. Snapshot delivery is push-only
via the `nmp_app_set_update_callback` registration. All timeline commands below
are fire-and-forget dispatches that cause subsequent snapshot emissions.

| Symbol | Signature | Behavior | Callers | D6 | D7 |
|---|---|---|---|---|---|
| UniFFI `loadOlderFeed` | `(key: String)` | Viewport command for an already-registered feed controller. The host reports "load older" by projection/feed key; Rust owns paging policy and appends through the normal snapshot/projection path. | feed views | invalid key → no-op | D7-clean: shell reports viewport intent; Rust owns page policy |
| UniFFI `openUri` | `(uri: String)` | Route a `nostr:` URI or bare NIP-19 entity. Kernel resolves the entity and pushes `ViewOpened` or `UriRejected` via snapshot. T80/T95. | Native URI adapters | invalid → silent no-op | D7-clean: kernel decides routing |

V-68 / V-112 (ADR-0042): `nmp_app_open_author`, `nmp_app_close_author`,
`nmp_app_open_thread`, and `nmp_app_close_thread` were **removed** (BREAKING,
v0.3.1). Author/thread feed sessions go through Rust `NmpApp::open_feed` with
`FeedScope::Authors` / `FeedScope::Referrer`, then close with the returned
`FeedHandle` through `NmpApp::close_feed`. The feed compiler registers the flat
projection, observed-projection sink, typed sidecar, acquisition interests, and
cached replay under the declared projection key; handle close tears down that
whole session.
SLICE-NS-READ-001: public C feed open/close was retired. App/staticlib Rust
composition owns typed feed-session open/close helpers; native shells keep only
rendering/progress commands such as UniFFI `loadOlderFeed`.

> **DELETED:** `nmp_app_claim_profile` and `nmp_app_release_profile` are removed.
> Profile hydration uses the typed resolve-ref surface (`nmp_app_resolve_profile_ref` /
> `nmp_app_release_profile_ref`) documented in §7.

---

## 6a. Replaceable event freshness (F-TTL)

Lazy TTL re-verification for replaceable Nostr events (kind:0 profiles, kind:10002 mailboxes, parameterized replaceables). The kernel automatically tracks when each replaceable should be re-fetched based on kind-specific TTLs. Force-refresh is exposed as a `force` argument on the existing claim functions (see §6) — **not** a standalone symbol.

There is no dedicated F-TTL symbol. Profile freshness uses `nmp_app_resolve_profile_ref` (see §7); `nmp_app_resolve_ref` uses `shape: int` (projection selector) — force-refresh does not apply to immutable event keys and is implicit for addressable ones via the TTL gate:

> **DELETED:** `nmp_app_claim_profile` is removed. Profile resolution is now through
> `nmp_app_resolve_profile_ref` / `nmp_app_release_profile_ref` (§7).

| Symbol | Signature | Behavior | Callers | D6 | D7 |
|---|---|---|---|---|---|
| `nmp_app_resolve_ref` / `nmp_app_resolve_ref_with_metadata` (namespace=1/event) | Bare: `(app, namespace: int, key: *const c_char, consumer_id: *const c_char, shape: int, liveness: int)`. Metadata variant adds `metadata_json: *const c_char`. | Refcount a raw event-key claim (namespace=1). `key` is a lowercase 64-hex event-id (`nevent`/`note`), `"kind:pubkey:d"` coordinate (`naddr`), or `"i:<external-id>"` NIP-73 external ref — **not** a `nostr:` URI. `shape`: `2`=event.embed, `3`=event.raw. App-owned URI adapters call the metadata variant after decoding NIP-19/NIP-21 so relay TLVs (`{"hints":[...]}`) and nevent author TLV (`{"author":"<hex>"}`) seed the same raw-key resolver. For cached `naddr` (addressable) identities, run the TTL gate as above; for immutable event-id keys `shape` selects the projection but there is no TTL record. | Native event/embed consumers | unparseable key, malformed metadata, or unknown shape/namespace → early return | n/a |
| `nmp_app_release_ref` (namespace=1/event) | `(app, namespace: int, key: *const c_char, consumer_id: *const c_char)` | Release a previously claimed event-key reference (namespace=1). `key` must match exactly what was passed to `nmp_app_resolve_ref`. Kernel decrements the per-consumer refcount and drops the row when no consumers remain. | Native event/embed consumers | invalid args → early return | n/a |

**Note:** force-refresh replaces the removed `nmp_app_refresh_replaceable` symbol (ADR-0041). `force != 0` is semantically "treat `check_again_after` as 0 for this claim", driving an immediate re-verification REQ. TTL management is otherwise transparent: the framework auto-re-verifies after kind-specific timeouts (default: kind:0 = 1h, kind:10002 = 6h).

See also: `docs/design/replaceable-freshness.md` (F-TTL design + lifecycle).

---

## 8. NIP-47 Wallet Connect

> **DELETED (2026-06-29 inventory, #2387):** `nmp_app_wallet_connect`,
> `nmp_app_wallet_disconnect`, and `nmp_app_wallet_pay_invoice` (and
> `ffi/wallet.rs`) have been removed. NIP-47 wallet actions now go through the
> typed byte action doorway (`nmp_app_dispatch_action_bytes` with `nmp.wallet.*`
> namespaces) after the host registers modules from `nmp-nip47`. See
> `nmp-nip47/src/action/mod.rs` for the current action-based interface.

---

## 9. Cancellation (`nmp-ffi/src/signer_broker.rs`)

`nmp_app_cancel_bunker_handshake` — documented in section 2 (NIP-46 actor-lane runtime).
No `_drop` or `_cancel` symbols exist outside that module.

---

## 10. Diagnostics (`nmp-ffi/src/debug_info.rs`)

`nmp_app_debug_info(app, domain: c_int) -> *mut c_char` — unified pull accessor
(#1726). `domain 0` = routing trace, `domain 1` = composition report,
`domain 2` = both merged. Unknown domain → `{}`. Never NULL for non-null app.
Caller frees with `nmp_free_string`. Replaces the former split symbols
`nmp_app_recent_routing_decisions` and `nmp_app_composition_report`.

Telemetry for the diagnostics screen also rides the standard update-callback
FlatBuffers frame (relay connection state, NIP-77 reconciler counters, publish
queue, profile interest refcounts).

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

## 12. App-owned Android JNI shims

> **Updated (#2387 inventory, 2026-06-29):** `nmp-android-ffi` no longer exists
> as a standalone crate. The in-tree Android JNI live in
> `apps/nmp-gallery/crates/nmp-app-gallery/src/android/` (feature
> `android-ffi`), serving the NmpGallery app. These are **app-owned** symbols,
> not a framework binding. The `Java_org_nmp_gallery_bridge_KernelBridge_*`
> symbols map directly to `nmp_app_*` functions from `nmp_ffi` plus
> gallery-specific adapters.

JNI shims are app-owned delivery glue, not the C ABI surface. The old Chirp
JNI/app-loop bridge was replaced by UniFFI in #2149 and then moved with Chirp to
the external repository. The current in-tree example is Gallery under
`apps/nmp-gallery/crates/nmp-app-gallery/src/android/`. Where an app still uses
JNI before #2125, update delivery remains push-only: Android registers a
listener object and Rust invokes `onUpdate(ByteArray)` from the kernel
update-listener thread after copying the borrowed FlatBuffers frame.

| JNI symbol | Maps to | Notes |
|---|---|---|
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeNew` (`android/mod.rs:43`) | `nmp_app_new` + callback setup | Returns `jlong` handle owning a boxed `GallerySession`. |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeFree` (`android/mod.rs:81`) | `nmp_app_free` + session teardown | Clears callbacks before free. |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeGalleryRegister` (`android/mod.rs:94`) | `nmp_app_gallery_register` | Gallery composition install. |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeShowcaseReferencesJson` (`android/mod.rs:105`) | `crate::showcase::raw_json()` | Returns showcase reference JSON; no session handle needed (static). |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeRegistryJson` (`android/mod.rs:116`) | `crate::registry::raw_json()` | Returns gallery registry JSON; no session handle needed (static). |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeDecodeSnapshotJson` (`android/mod.rs:127`) | `nmp_app_gallery_snapshot_json_from_update_frame` | Gallery projection decode. |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeStart` (`android/mod.rs:164`) | `nmp_app_start` | `visible_limit` + `emit_hz` passed as `jint`. |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeStop` (`android/mod.rs:186`) | `nmp_app_stop` | — |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeResolveProfileRef` (`android/mod.rs:200`) | `nmp_app_resolve_profile_ref` | ADR-0063 (#1671): typed profile-ref resolution for visible gallery authors. Supersedes deleted `nativeClaimProfile`. |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeReleaseProfileRef` (`android/mod.rs:221`) | `nmp_app_release_profile_ref` | ADR-0063: release a profile ref previously registered via `nativeResolveProfileRef`. |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeSetUpdateListener` (`android/mod.rs:246`) | `nmp_app_set_update_callback` via session push listener | Registers Kotlin listener; frames pushed as `ByteArray`. |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeClearUpdateListener` (`android/mod.rs:259`) | clears push listener | — |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeDispatchAction` (`android/mod.rs:286`) | `dispatch_action_bytes_for` | Gallery typed byte dispatch. |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeSetSignerRequestListener` (`android/mod.rs:331`) | session signer-request push listener | Registers a JNI listener for NIP-55 signer-request events; pass `null` to deregister. |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeClearSignerRequestListener` (`android/mod.rs:356`) | clears signer-request listener | Clears the listener without freeing the session. |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeSignInNip55` (`android/mod.rs:372`) | `nmp_app_signin_nip55` | NIP-55 adapter. |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeDeliverSignerResponse` (`android/mod.rs:396`) | `nmp_app_deliver_external_signer_response` | External signer response. |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeResolveEventRef` (`android/event_refs.rs:77`) | `nmp_app_resolve_event_embed_with_metadata` | URI→event-ref adapter. |
| `Java_org_nmp_gallery_bridge_KernelBridge_nativeReleaseEvent` (`android/event_refs.rs:105`) | `nmp_app_release_event_ref` | Event ref release. |

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
User-authored publish/social actions enter through `nmp_app_dispatch_action_bytes`;
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
| `nmp_app_dispatch_action_bytes` | PASS — acceptance/error JSON, never NULL for non-null app | PASS | Single user/app action door |
| `nmp_app_ack_action_stage` | PASS | PASS | |
| `nmp_app_retry_publish` | PASS | PASS | Publish lifecycle control |
| `nmp_app_cancel_action` | PASS | PASS | Publish/action lifecycle control (`nmp_app_cancel_publish` deleted) |
| `nmp_app_add_relay` | PASS | PASS | |
| `nmp_app_remove_relay` | PASS | PASS | |
| UniFFI `loadOlderFeed` | PASS | PASS | Viewport command only; Rust owns feed page policy |
| UniFFI `openUri` | PASS | PASS | |
| `nmp_app_resolve_ref` (namespace=1/event) | PASS | PASS | Unified event-ref resolution (`nmp_app_claim_profile` deleted) |
| `nmp_app_release_ref` (namespace=1/event) | PASS | PASS | Unified event-ref release (`nmp_app_release_profile` deleted) |
| `nmp_app_debug_info` | PASS — `{}` for unknown domain/null app, never NULL for non-null app | PASS | Diagnostic pull accessor |
| ~~`nmp_app_wallet_connect/disconnect/pay_invoice`~~ | DELETED | DELETED | Removed; NIP-47 goes through byte action doorway |

**Zero D6 violations. Zero D7 violations.**

---

## Current findings

1. **`nmp_drain_updates` does not exist.** The task brief presumed a pull-side
   drain symbol. Snapshot delivery is push-only via `nmp_app_set_update_callback`.
   No action needed — the architecture is intentionally push.

2. **RESOLVED (#2387, 2026-06-29):** Full generated-symbol audit completed. See
   the inventory comment on #2125 for the complete classified table (56
   migrate-to-UniFFI, 6 deleted-from-code, 4 measured-internal-exceptions pending
   #2388, 26 app-owned/external). Stale wallet / claim_profile / cancel_publish
   entries corrected in this doc update.

3. **RESOLVED (V-68 / V-112, ADR-0042):** `nmp_app_open_author`,
   `nmp_app_close_author`, `nmp_app_open_thread`, and `nmp_app_close_thread`
   were removed in v0.3.1. The prior open-without-close subscription-leak gap is
   structurally closed by the typed feed-session C pair above: the opaque feed
   handle owns the registered projection, observed-projection sink, acquisition
   interests, and teardown recipe, so close never re-derives a raw filter.
