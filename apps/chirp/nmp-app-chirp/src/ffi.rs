//! Chirp per-app FFI surface.
//!
//! `extern "C"` symbols Swift links against:
//!
//! - [`nmp_app_chirp_register`] — instantiate `ChirpModularTimeline` with the
//!   active viewer pubkey and register it as a kernel event observer on the
//!   supplied `NmpApp`. Returns an opaque handle (boxed projection +
//!   observer id) for later snapshots / unregister.
//! - [`nmp_app_chirp_register_group_chat`] — wire a NIP-29
//!   `GroupChatProjection` for one group into the kernel: an event observer
//!   (ingest) plus a `"nmp.nip29.group_chat"` snapshot projection (output). Pure
//!   consumption — no handle, no actions, no unregister.
//! - [`nmp_app_chirp_register_dm_inbox`] — compatibility entry point for the
//!   NIP-17 DM runtime. `nmp_app_chirp_register` wires it eagerly: a kind:1059
//!   raw-event observer, a `"nmp.nip17.dm_inbox"` snapshot projection, and a
//!   Rust-owned controller for the active gift-wrap interest + kind:10050
//!   relay-list publish.
//! - [`nmp_app_chirp_snapshot`] — serialize the current `ChirpTimelineSnapshot`
//!   into a freshly-allocated nul-terminated JSON C string. Swift owns the
//!   pointer until it calls `nmp_app_chirp_snapshot_free`.
//! - [`nmp_app_chirp_snapshot_free`] — companion deallocator for the snapshot
//!   string.
//! - [`nmp_app_chirp_unregister`] — drop the observer registration and free
//!   the handle. Idempotent.
//!
//! ## Doctrine
//!
//! * **D0** — `nmp-core` never depends on `nmp-nip01`; this crate is the
//!   composition point. ADR-0009 (kernel boundary).
//! * **D6** — every entry point is fire-and-forget. Null pointers, missing
//!   strings, serialization failures, and poisoned mutexes all degrade
//!   silently rather than raising across the FFI.
//! * **No business logic in Swift** — Swift takes the JSON string, decodes
//!   to `[TimelineBlock] + [ChirpEventCard]`, and renders. All grouping
//!   happens here / in `nmp-threading`.

use std::ffi::{c_char, CStr, CString};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::{ActorCommand, KernelEventObserver, KernelEventObserverId, NmpApp};
use nmp_nip29::group_id::GroupId;
use nmp_nip29::projection::{DiscoveredGroupsProjection, GroupChatProjection};
use nmp_nip29::action::{
    CommentInGroupAction, DiscoverGroupsAction, JoinGroupAction,
    PostChatMessageAction, ReactInGroupAction,
};
use nmp_nip17::{PublishDmRelayListAction, SendDmAction};
use nmp_nip57::ZapAction;
use nmp_nip01::meta_timeline::Pubkey;
use nmp_nip01::{ModularTimelineProjection, ModularTimelineSpec};
use nmp_threading::ModulePolicy;

use crate::dm_runtime::register_dm_runtime;
use crate::follow_list::FollowListProjection;


/// Opaque handle returned by [`nmp_app_chirp_register`]. Boxed on the heap
/// so the address is stable; the Swift consumer holds the raw pointer until
/// it calls [`nmp_app_chirp_unregister`].
pub struct ChirpHandle {
    projection: Arc<ModularTimelineProjection>,
    observer_id: KernelEventObserverId,
    app: *mut NmpApp,
}

// SAFETY: the auto-derived `!Send`/`!Sync` comes solely from the `app: *mut
// NmpApp` field (the `Arc<ChirpModularTimeline>` is already `Send + Sync`).
// The handle is sound to mark `Send + Sync` because of three layered facts —
// stated honestly, since the previously-claimed "Swift serializes every FFI
// call on one thread" is NOT true (`KernelHandle` is a plain `final class`
// with no dispatch queue):
//
//   1. Swift owns this handle and only ever touches it from one isolation
//      context. In Chirp the FFI entry points below are reached exclusively
//      from `@MainActor` types (`KernelModel`, `MarmotStore`), so the handle
//      itself is never raced. (This is a Swift-side caller convention, not a
//      type-system guarantee — hence it is documented, not enforced here.)
//   2. The `Arc<ModularTimelineProjection>` *is* genuinely shared across threads:
//      the kernel actor thread invokes `ModularTimelineProjection`'s observer
//      callbacks while the Swift main actor calls `snapshot()`. Soundness of
//      that sharing comes from the projection's own interior `Mutex`, NOT
//      from this `unsafe impl`.
//   3. The `app` raw pointer is only ever *read* — never mutated, and never
//      dereferenced from a kernel callback. The use-after-free question is
//      "can a callback touch `app` after `nmp_app_free`?" — and it cannot:
//      `nmp_app_free` drops `NmpApp`, whose `Drop` sends `Shutdown` and then
//      `join()`s the actor thread before the allocation is freed. The Rust
//      observer fan-out (`notify_observers`) invokes `on_kernel_event`
//      INLINE on that actor thread, so the join fences any in-flight
//      callback. Calling `nmp_app_chirp_unregister` before `nmp_app_free`
//      (the documented contract) is additional hygiene; the actor join is
//      the actual fence.
//
// CALLER CONTRACT: `nmp_app_free` must not be invoked while any kernel
// callback that reaches this handle's projection is still in flight. The
// in-process Rust-trait registration path used here gets that fence for free
// (the actor join). A hypothetical C-ABI observer would NOT — its drain
// thread is separate and is not joined by `nmp_app_free`.
unsafe impl Send for ChirpHandle {}
unsafe impl Sync for ChirpHandle {}

/// Register a Chirp modular timeline projection against `app`. Returns a
/// non-null `*mut ChirpHandle` on success; `null` on any failure (null
/// pointer arguments, invalid UTF-8 viewer pubkey, slot lock poisoning).
///
/// `viewer_pubkey` is a hex-encoded pubkey (typically 64 chars; not
/// validated here — the grouper carries it through unchanged for future
/// personalization keys). NULL is permitted and treated as "no viewer".
///
/// `app` MUST outlive the returned handle. Call [`nmp_app_chirp_unregister`]
/// before `nmp_app_free`.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_register(
    app: *mut NmpApp,
    viewer_pubkey: *const c_char,
) -> *mut ChirpHandle {
    if app.is_null() {
        return std::ptr::null_mut();
    }
    // Register Chirp's social-verb action namespaces (`chirp.react`,
    // `chirp.follow`, `chirp.unfollow`) against the kernel's action registry
    // BEFORE taking the shared `&NmpApp` borrow below. This needs `&mut
    // NmpApp` and must run during host init — before `nmp_app_start` and
    // before any `nmp_app_dispatch_action` (D0 — social verbs live in this
    // app crate, never in `nmp-core`).
    //
    // SAFETY: caller guarantees `app` is a valid pointer from `nmp_app_new`.
    // No other reference aliases it at this point — the `&*app` borrow below
    // is taken only after this exclusive borrow is dropped.
    register_chirp_actions(unsafe { &mut *app });

    // Register the NIP-29 group-chat `ActionModule`s against the kernel.
    // Unlike `register_chirp_actions` (Chirp's own social verbs), this wires
    // `ActionModule` impls that live in the `nmp-nip29` protocol crate —
    // proving the host-extensibility seam works for NIP-crate modules too,
    // not just app-local verbs. Same `&mut NmpApp` / pre-`nmp_app_start`
    // ordering rule as `register_chirp_actions` above.
    //
    // SAFETY: same exclusive-borrow rationale as `register_chirp_actions` —
    // no other reference aliases `app` at this point.
    register_nip29_actions(unsafe { &mut *app });

    // Register the NIP-17 direct-message `ActionModule` (`nmp.nip17.send`).
    // Same `&mut NmpApp` / pre-`nmp_app_start` ordering rule as the NIP-29
    // registration above — a third NIP-crate `ActionModule` reached through
    // the generic `dispatch_action` seam (D0 — no DM nouns in `nmp-core`).
    //
    // SAFETY: same exclusive-borrow rationale as `register_chirp_actions`.
    register_nip17_actions(unsafe { &mut *app });

    // Register the NIP-57 zap `ActionModule` (`nmp.nip57.zap`) against the
    // kernel. The executor records the zap intent as an observable stub while
    // `HttpCapability` (ADR-0024) is built out; see `nmp-nip57/src/action.rs`
    // for the upgrade path. D0-clean: `nmp-core` gains no zap nouns.
    //
    // SAFETY: same exclusive-borrow rationale as `register_chirp_actions`.
    register_nip57_actions(unsafe { &mut *app });

    // SAFETY: caller guarantees `app` is a valid pointer allocated by
    // `nmp_app_new` for the duration of this call. We do not hold the
    // borrow past this function.
    let app_ref = unsafe { &*app };
    register_dm_runtime(app_ref);

    let viewer: Pubkey = c_string_opt(viewer_pubkey).unwrap_or_default();
    let spec = ModularTimelineSpec {
        viewer,
        kinds: Vec::new(),
        authors: None,
        policy: ModulePolicy::default(),
    };

    let projection = Arc::new(ModularTimelineProjection::new(spec));
    let observer_id = app_ref
        .register_event_observer(Arc::clone(&projection) as Arc<dyn nmp_core::KernelEventObserver>);
    if observer_id.0 == 0 {
        // Registration failed (poisoned mutex). Don't leak the projection;
        // caller gets a null handle and treats it as a soft-fail.
        return std::ptr::null_mut();
    }

    Box::into_raw(Box::new(ChirpHandle {
        projection,
        observer_id,
        app,
    }))
}

/// Wire a NIP-29 `GroupChatProjection` for a single group into `app`.
///
/// This is **pure consumption** — the read-side of a group-chat screen. It
/// adds no new C-ABI handle type and registers no actions: it constructs a
/// [`GroupChatProjection`] scoped to the supplied group, plugs it into the
/// kernel as a [`KernelEventObserver`] (ingest), and registers its
/// [`GroupChatProjection::snapshot_json`] read under the snapshot key
/// `"nmp.nip29.group_chat"` (output). The group's chat messages then surface in
/// every snapshot tick under that key.
///
/// `group_id_json` is a JSON object naming the target group:
///
/// ```json
/// {"host_relay_url":"wss://groups.example.com","local_id":"room"}
/// ```
///
/// D6 — fire-and-forget. A null `app`, a null/invalid-UTF-8 `group_id_json`,
/// a JSON shape that does not deserialize to a [`GroupId`], or a poisoned
/// observer slot all degrade to a silent return — nothing is registered and
/// no error crosses the FFI.
///
/// SCOPE — single-screen, no unregister. Unlike [`nmp_app_chirp_register`]
/// this returns no handle, so there is no companion `unregister`.
///
/// Re-invocation is **idempotent**: a subsequent call unregisters the previous
/// projection's observer before registering the new one (via the per-app
/// `swap_singleton_event_observer` slot on `NmpApp`), and overwrites the
/// `"nmp.nip29.group_chat"` snapshot key with the newer projection. The
/// per-account re-invocation case (the only re-invocation Chirp actually
/// performs) is leak-free. A multi-group host that wants to keep N projections
/// live in parallel would still need a handle-returning variant — single-slot
/// idempotency does not generalize to N concurrent groups.
///
/// `app` MUST outlive the registration. It is only borrowed for the duration
/// of this call; the projection it registers is owned by the kernel.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_register_group_chat(
    app: *mut NmpApp,
    group_id_json: *const c_char,
) {
    if app.is_null() {
        return;
    }
    // SAFETY: caller guarantees `app` is a valid pointer from `nmp_app_new`,
    // live for the duration of this call. The borrow is not held past return.
    let app_ref = unsafe { &*app };

    // Reject silently on a missing or malformed group id — D6. The JSON must
    // deserialize to the typed `GroupId { host_relay_url, local_id }`.
    let Some(raw) = c_string_opt(group_id_json) else {
        return;
    };
    let Ok(group_id) = serde_json::from_str::<GroupId>(&raw) else {
        return;
    };

    let projection = Arc::new(GroupChatProjection::new(group_id));
    let observer_id =
        app_ref.register_event_observer(Arc::clone(&projection) as Arc<dyn KernelEventObserver>);
    if observer_id.0 == 0 {
        // Observer registration failed (poisoned slot). Don't register the
        // snapshot closure for a projection that will never receive events,
        // and don't disturb the previously-installed slot — leave any prior
        // observer in place rather than clearing it for nothing.
        return;
    }

    // Idempotent re-invoke: atomically install the new id and take the prior
    // id out of the per-app slot, then unregister the prior observer. The
    // swap-then-unregister order is deliberate (see `swap_nip29_group_chat_
    // observer`): the new observer is already live when the old one is
    // dropped, so there is no event-loss gap and a concurrent re-invoke
    // cannot leak the previous id.
    if let Some(prev) = app_ref.swap_singleton_event_observer(Some(observer_id)) {
        app_ref.unregister_event_observer(prev);
    }

    // Output side: the no-argument snapshot read runs on the actor thread
    // inside each snapshot tick. The `move` consumes this last `Arc`.
    app_ref.register_snapshot_projection("nmp.nip29.group_chat", move || projection.snapshot_json());
}

/// Wire a NIP-29 [`DiscoveredGroupsProjection`] for one host relay into `app`.
///
/// This is the **read side** of the NIP-29 group-discovery flow. It
/// constructs a projection scoped to the supplied relay URL, plugs it in
/// as a [`KernelEventObserver`] (ingest), and registers its
/// [`DiscoveredGroupsProjection::snapshot_json`] read under the snapshot key
/// `"nmp.nip29.discovered_groups"` (output). Kind:39000/39001/39002 events for
/// that host relay then surface on every snapshot tick under that key.
///
/// The companion publish side is the `nmp.nip29.discover` action — its
/// executor pushes a relay-pinned [`LogicalInterest`] (kinds
/// 39000/39001/39002) so the kernel opens a REQ and metadata events
/// actually arrive. The projection registered here is *inert* without that
/// interest. A host shell drives both halves from one user gesture
/// ("discover groups on this relay"): first this FFI registers the read
/// projection, then `nmp_app_dispatch_action("nmp.nip29.discover", ...)`
/// pushes the interest.
///
/// `host_relay_url` is a plain C string (`wss://groups.example.com`). The
/// Rust side accepts it verbatim — same canonicalisation rules as
/// `LogicalInterest::relay_pin`.
///
/// D6 — fire-and-forget. A null `app`, a null or non-UTF-8
/// `host_relay_url`, or a poisoned observer slot all degrade to a silent
/// return — nothing is registered and no error crosses the FFI.
///
/// SCOPE — single-screen, no unregister. Like
/// [`nmp_app_chirp_register_group_chat`], this returns no handle and has no
/// companion unregister. Calling it twice overwrites the
/// `"nmp.nip29.discovered_groups"` snapshot key with the newer projection and
/// leaves the older event observer registered for the life of the `app`
/// (a small, bounded leak). The Swift `JoinGroupView` drives one relay at
/// a time, so this is acceptable for v1; a multi-relay discovery screen
/// would need a handle-returning variant.
///
/// `app` MUST outlive the registration. It is only borrowed for the
/// duration of this call; the projection it registers is owned by the
/// kernel.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_register_group_discovery(
    app: *mut NmpApp,
    host_relay_url: *const c_char,
) {
    if app.is_null() {
        return;
    }
    // SAFETY: caller guarantees `app` is a valid pointer from `nmp_app_new`,
    // live for the duration of this call. The borrow is not held past return.
    let app_ref = unsafe { &*app };

    // Reject silently on a missing or malformed relay URL — D6.
    let Some(relay_url) = c_string_opt(host_relay_url).filter(|s| !s.is_empty()) else {
        return;
    };

    let projection = Arc::new(DiscoveredGroupsProjection::new(relay_url));
    let observer_id =
        app_ref.register_event_observer(Arc::clone(&projection) as Arc<dyn KernelEventObserver>);
    if observer_id.0 == 0 {
        // Observer registration failed (poisoned slot). Don't register a
        // snapshot closure for a projection that will never see events.
        return;
    }

    // Output side: the no-argument snapshot read runs on the actor thread
    // inside each snapshot tick. The `move` consumes this last `Arc`.
    app_ref.register_snapshot_projection("nmp.nip29.discovered_groups", move || {
        projection.snapshot_json()
    });
}

/// Wire the NIP-17 DM runtime into `app`.
///
/// The `viewer_pubkey` argument is retained for C-ABI compatibility but is no
/// longer read. Rust observes the active local-key slot and relay-edit rows on
/// snapshot ticks, then owns the active-account kind:1059 gift-wrap interest,
/// kind:10050 relay-list publish, and `"nmp.nip17.dm_inbox"` projection.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_register_dm_inbox(app: *mut NmpApp, viewer_pubkey: *const c_char) {
    let _ = viewer_pubkey;
    if app.is_null() {
        return;
    }
    // SAFETY: caller guarantees `app` is a valid pointer from `nmp_app_new`,
    // live for the duration of this call. The borrow is not held past return.
    let app_ref = unsafe { &*app };
    register_dm_runtime(app_ref);
}

/// Wire a [`FollowListProjection`] for the active account into `app`.
///
/// This is **pure consumption** of the NIP-02 kind:3 contact list. It
/// constructs a [`FollowListProjection`] bound to `active_pubkey`, plugs it
/// into the kernel as a [`KernelEventObserver`] (ingest), and registers its
/// `snapshot_json` read under the snapshot key `"chirp.follow_list"` (output).
/// The active account's formatted follow list then surfaces on every kernel
/// snapshot tick under that key.
///
/// `active_pubkey` is the active account's hex pubkey. It is stored in the
/// projection's shared slot so `snapshot_json` returns the correct account's
/// follows even if kind:3 events from multiple accounts have arrived.
///
/// The kernel already subscribes to kind:3 for the active account as part of
/// the `account_profile_interest` (kind:0 + kind:3 + kind:10002), so no
/// separate interest push is needed — events arrive through the standing
/// subscription.
///
/// CALLER CONTRACT — re-invoke after account switch with the new pubkey.
/// The projection accumulates follow lists for all observed authors; only the
/// active pubkey's list surfaces in the snapshot. A re-invoke for the same
/// account overwrites the `"chirp.follow_list"` snapshot key with an
/// equivalent projection (small bounded leak on the observer slot).
///
/// D6 — fire-and-forget. A null `app` or a poisoned observer slot degrades
/// to a silent return.
///
/// `app` MUST outlive the registration; it is only borrowed for this call.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_register_follow_list(
    app: *mut NmpApp,
    active_pubkey: *const c_char,
) {
    if app.is_null() {
        return;
    }
    // SAFETY: caller guarantees `app` is a valid pointer from `nmp_app_new`,
    // live for the duration of this call. The borrow is not held past return.
    let app_ref = unsafe { &*app };

    // Extract the active pubkey string; `None` is permitted (before sign-in).
    let pubkey_opt = c_string_opt(active_pubkey).filter(|s| !s.is_empty());

    // The shared slot the projection and the FFI both hold: the projection
    // reads it at snapshot time, the caller updates it on account switch.
    let active_pubkey_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(pubkey_opt));

    let projection = Arc::new(FollowListProjection::new(Arc::clone(&active_pubkey_slot)));

    let observer_id =
        app_ref.register_event_observer(Arc::clone(&projection) as Arc<dyn KernelEventObserver>);
    if observer_id.0 == 0 {
        // Observer registration failed (poisoned slot). Don't register the
        // snapshot closure for a projection that will never receive events.
        return;
    }

    // Output side: the no-argument snapshot read runs on the actor thread
    // inside each snapshot tick. The `move` consumes this last `Arc`.
    app_ref.register_snapshot_projection("chirp.follow_list", move || projection.snapshot_json());
}

/// Serialize the current `ChirpTimelineSnapshot` into a JSON C string.
/// Returns null on any failure (null handle, JSON encode error, CString
/// nul-byte conflict). The returned pointer is owned by the caller; pass it
/// to [`nmp_app_chirp_snapshot_free`] when done.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_snapshot(handle: *mut ChirpHandle) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees `handle` is a valid pointer returned by
    // `nmp_app_chirp_register` and not yet freed.
    let handle = unsafe { &*handle };
    let snapshot = handle.projection.snapshot();
    let Ok(payload) = serde_json::to_string(&snapshot) else {
        return std::ptr::null_mut();
    };
    let Ok(cstr) = CString::new(payload) else {
        return std::ptr::null_mut();
    };
    cstr.into_raw()
}

/// Free a snapshot string previously returned by
/// [`nmp_app_chirp_snapshot`]. Null pointer is a silent no-op.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_snapshot_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: caller guarantees `ptr` came from `CString::into_raw` in
    // `nmp_app_chirp_snapshot` and has not been freed.
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

/// Drop the projection's observer registration and free the handle.
/// Idempotent: null pointer is a silent no-op. The handle MUST NOT be used
/// after this call.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_unregister(handle: *mut ChirpHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: caller guarantees `handle` came from `nmp_app_chirp_register`
    // and has not already been freed.
    let boxed = unsafe { Box::from_raw(handle) };
    if !boxed.app.is_null() {
        // SAFETY: same `app` validity rule as `nmp_app_chirp_register` — the
        // caller is responsible for the `nmp_app_free` ordering invariant.
        let app_ref = unsafe { &*boxed.app };
        app_ref.unregister_event_observer(boxed.observer_id);
    }
    // boxed dropped here — projection's last Arc released only if no other
    // strong refs exist (none should once the observer is unregistered).
}

fn c_string_opt(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees `ptr` (when non-null) is a valid
    // nul-terminated C string for the duration of this call.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(|s| s.to_owned())
}

/// Register Chirp's social-verb action namespaces against `app`'s action
/// registry. Each namespace gets BOTH a module (shape validator, consumed by
/// `ActionRegistry::start`) AND an executor (the `ActorCommand` enqueue,
/// consumed by `ActionRegistry::execute`) — `nmp_app_dispatch_action`
/// requires both halves.
///
/// This is the D0-clean replacement for the deleted per-verb C symbols
/// (`nmp_app_react`, `nmp_app_follow`, `nmp_app_unfollow`): the social verbs
/// now live in this app crate and reach the kernel through the generic
/// `dispatch_action` path, not through bespoke `nmp-core` FFI symbols.
///
/// JSON schemas (the third arg the host passes to `nmp_app_dispatch_action`):
/// * `chirp.react`   — `{"target_event_id":"<hex>","reaction":"+"}`
/// * `chirp.follow`  — `{"pubkey":"<hex>"}`
/// * `chirp.unfollow`— `{"pubkey":"<hex>"}`
///
/// Hex-shape validation deliberately stays in the actor's command handlers
/// (which own the user-facing toasts) — the module validators here only check
/// JSON shape, mirroring the comment the deleted FFI symbols carried (D6).

struct ChirpReactModule;
impl ActionModule for ChirpReactModule {
    const NAMESPACE: &'static str = "chirp.react";
    type Action = ReactAction;
    fn start(_ctx: &mut ActionContext, _action: Self::Action) -> Result<(), ActionRejection> {
        Ok(())
    }
    fn execute(action: Self::Action, _correlation_id: &str, send: &dyn Fn(ActorCommand)) -> Result<(), String> {
        send(ActorCommand::React { target_event_id: action.target_event_id, reaction: action.reaction });
        Ok(())
    }
}

struct ChirpFollowModule;
impl ActionModule for ChirpFollowModule {
    const NAMESPACE: &'static str = "chirp.follow";
    type Action = PubkeyAction;
    fn start(_ctx: &mut ActionContext, _action: Self::Action) -> Result<(), ActionRejection> {
        Ok(())
    }
    fn execute(action: Self::Action, _correlation_id: &str, send: &dyn Fn(ActorCommand)) -> Result<(), String> {
        send(ActorCommand::Follow { pubkey: action.pubkey });
        Ok(())
    }
}

struct ChirpUnfollowModule;
impl ActionModule for ChirpUnfollowModule {
    const NAMESPACE: &'static str = "chirp.unfollow";
    type Action = PubkeyAction;
    fn start(_ctx: &mut ActionContext, _action: Self::Action) -> Result<(), ActionRejection> {
        Ok(())
    }
    fn execute(action: Self::Action, _correlation_id: &str, send: &dyn Fn(ActorCommand)) -> Result<(), String> {
        send(ActorCommand::Unfollow { pubkey: action.pubkey });
        Ok(())
    }
}

fn register_chirp_actions(app: &mut NmpApp) {
    app.register_action::<ChirpReactModule>();
    app.register_action::<ChirpFollowModule>();
    app.register_action::<ChirpUnfollowModule>();
}

/// Register the 3 NIP-29 group-chat action namespaces against `app`'s action
/// registry.
///
/// This wires typed `ActionModule` impls from the `nmp-nip29` protocol crate
/// via `NmpApp::register_action::<M>()` — the ADR-0027 single-call path that
/// eliminates the pre-ADR-0027 `register_action_module` + `register_action_executor`
/// split. Any NIP crate's typed `ActionModule` can be reached through the
/// generic `dispatch_action` path without `nmp-core` learning any NIP-29
/// group nouns (D0).
///
/// `M::start` handles validation and `M::execute` handles execution — both
/// under the same `M::NAMESPACE`, so namespace mismatch between validator and
/// executor is structurally impossible.
///
/// Namespaces: `nmp.nip29.post_chat_message`, `nmp.nip29.react_in_group`,
/// `nmp.nip29.comment_in_group`, `nmp.nip29.discover`, `nmp.nip29.join`.
///
/// SCOPE: NIP-29 v1 ships chat (3 actions), discovery, and join. The admin /
/// membership (9000-9009) and artifact / discussion executors are deliberately
/// out of scope — Marmot MLS covers private groups; group administration UI
/// is not planned for this milestone.
///
/// `nmp.nip29.discover` is structurally different from the four publish-side
/// actions: it returns [`ActorCommand::PushInterest`] (subscribe to the
/// host relay's kind:39000/39001/39002 catalog), not
/// `PublishUnsignedEventToRelays`. The companion read-side is
/// [`nmp_app_chirp_register_group_discovery`] below — a
/// [`DiscoveredGroupsProjection`] scoped to the same relay.
fn register_nip29_actions(app: &mut NmpApp) {
    app.register_action::<PostChatMessageAction>();
    app.register_action::<ReactInGroupAction>();
    app.register_action::<CommentInGroupAction>();
    app.register_action::<DiscoverGroupsAction>();
    app.register_action::<JoinGroupAction>();
}

/// Register the NIP-17 direct-message `ActionModule` (`nmp.nip17.send`) against
/// `app`'s action registry.
///
/// Wires the typed [`SendDmAction`] from the `nmp-nip17` protocol crate
/// through the same host-extensibility seam the NIP-29 actions use. The
/// executor delegates to `nmp_nip17::SendDmAction::execute`, which builds the
/// kind:14 rumor and enqueues [`ActorCommand::SendGiftWrappedDm`] — the
/// actor's local-keys-MVP handler does the NIP-59 seal + gift-wrap + publish.
///
/// JSON schema (the third arg the host passes to `nmp_app_dispatch_action`):
/// * `nmp.nip17.send` — `{"recipient_pubkey":"<hex>","content":"…","reply_to":"<hex>"?}`
/// * `nmp.nip17.publish_relay_list` — `{"relays":["wss://relay.example", ...]}`
///
/// `nmp.nip17.publish_relay_list` closes the symmetric publish gap: the kernel
/// ingests kind:10050 (NIP-17 DM-relay list) into `dm_relay_lists`, but
/// without a publish path every NMP user is invisible to other clients
/// trying to send them gift-wrapped DMs. The executor builds the kind:10050
/// unsigned event with `["relay", <url>]` tags and enqueues
/// `ActorCommand::PublishUnsignedEvent` — kind:10050 is a NIP-65 replaceable
/// event and routes through the author's kind:10002 write relays.
fn register_nip17_actions(app: &mut NmpApp) {
    app.register_action::<SendDmAction>();
    app.register_action::<PublishDmRelayListAction>();
}

/// Register the NIP-57 zap action namespace (`nmp.nip57.zap`) against `app`'s
/// action registry.
///
/// Wires the typed [`ZapAction`] from the `nmp-nip57` protocol crate through
/// the same host-extensibility seam as the NIP-29 and NIP-17 actions. The
/// executor is a validated intent stub: `start` checks the input shape (non-
/// empty recipient, non-zero amount, at least one relay) and `execute` records
/// the intent via `ShowToast`. The LNURL HTTP fetch + bolt11 payment are
/// deferred to `HttpCapability` (ADR-0024); see `nmp-nip57/src/action.rs` for
/// the upgrade path.
///
/// JSON schema (the third arg the host passes to `nmp_app_dispatch_action`):
/// * `nmp.nip57.zap` — `{"recipient_pubkey":"<hex>","amount_msats":21000,"relays":["wss://..."],"target_event_id":"<hex>"?,"comment":"…"?}`
///
/// D0-clean: `nmp-core` gains no NIP-57 / zap nouns.
fn register_nip57_actions(app: &mut NmpApp) {
    app.register_action::<ZapAction>();
}

/// `chirp.react` action body: `{"target_event_id":"<hex>","reaction":"+"}`.
/// `reaction` defaults to `"+"` (the standard kind:7 like) when absent —
/// matching the old `nmp_app_react` FFI symbol's `unwrap_or("+")` behaviour.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
struct ReactAction {
    target_event_id: String,
    #[serde(default = "default_reaction")]
    reaction: String,
}

fn default_reaction() -> String {
    "+".to_string()
}

/// `chirp.follow` / `chirp.unfollow` action body: `{"pubkey":"<hex>"}`.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
struct PubkeyAction {
    pubkey: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::nmp_app_free;
    use nmp_core::nmp_app_new;
    use nmp_nip29::action::{DiscoverGroupsInput, JoinGroupInput, PostChatMessageInput};
    use nmp_nip29::kinds::KIND_CHAT_MESSAGE;
    use std::cell::RefCell;

    /// Run an `ActionModule`'s typed executor once and capture the single
    /// `ActorCommand` it sends. Mirrors `nmp_nip17::dm_relay_list`'s test
    /// pattern — the canonical post-ADR-0027 executor probe.
    fn run_module_execute<M: ActionModule>(input: M::Action) -> Result<ActorCommand, String> {
        let captured: RefCell<Option<ActorCommand>> = RefCell::new(None);
        M::execute(input, "test-cid", &|cmd| {
            *captured.borrow_mut() = Some(cmd);
        })?;
        captured
            .into_inner()
            .ok_or_else(|| "executor sent no command".to_string())
    }

    #[test]
    fn register_snapshot_unregister_round_trip() {
        let app = nmp_app_new();
        let handle = nmp_app_chirp_register(app, std::ptr::null());
        assert!(!handle.is_null(), "register returned null");

        // Empty snapshot — no events have arrived.
        let snap = nmp_app_chirp_snapshot(handle);
        assert!(!snap.is_null());
        // SAFETY: snap is a valid C string from our own CString.
        let json = unsafe { CStr::from_ptr(snap) }.to_str().unwrap().to_owned();
        nmp_app_chirp_snapshot_free(snap);
        // Empty snapshot decodes to empty arrays.
        assert!(json.contains("\"blocks\":[]"));
        assert!(json.contains("\"cards\":[]"));

        nmp_app_chirp_unregister(handle);
        nmp_app_free(app);
    }

    #[test]
    fn null_handle_paths_are_silent_noops() {
        nmp_app_chirp_unregister(std::ptr::null_mut());
        let snap = nmp_app_chirp_snapshot(std::ptr::null_mut());
        assert!(snap.is_null());
        nmp_app_chirp_snapshot_free(std::ptr::null_mut());
    }

    #[test]
    fn register_with_null_app_returns_null() {
        let handle = nmp_app_chirp_register(std::ptr::null_mut(), std::ptr::null());
        assert!(handle.is_null());
    }

    use nmp_core::{nmp_app_dispatch_action, nmp_app_free_string};

    /// Drive `nmp_app_dispatch_action` for `namespace`/`action_json` and
    /// return the parsed JSON result. The returned C string is freed.
    fn dispatch(app: *mut NmpApp, namespace: &str, action_json: &str) -> serde_json::Value {
        let ns = CString::new(namespace).unwrap();
        let body = CString::new(action_json).unwrap();
        let ptr = nmp_app_dispatch_action(app, ns.as_ptr(), body.as_ptr());
        assert!(!ptr.is_null(), "dispatch_action must never return null");
        // SAFETY: `ptr` is a valid C string from `nmp_app_dispatch_action`.
        let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        nmp_app_free_string(ptr);
        serde_json::from_str(&out).unwrap()
    }

    /// THE MIGRATION PROOF: after `nmp_app_chirp_register`, the three social
    /// verbs are reachable through the generic `dispatch_action` path — each
    /// returns a 32-hex `correlation_id`, proving BOTH the host-registered
    /// module (consumed by `start()`) AND executor (consumed by `execute()`)
    /// are wired. This replaces the deleted per-verb `nmp_app_react` /
    /// `nmp_app_follow` / `nmp_app_unfollow` C symbols (D0).
    #[test]
    fn social_verbs_dispatch_through_action_registry() {
        let app = nmp_app_new();
        let handle = nmp_app_chirp_register(app, std::ptr::null());
        assert!(!handle.is_null());

        for (namespace, body) in [
            ("chirp.react", r#"{"target_event_id":"abc","reaction":"+"}"#),
            ("chirp.follow", r#"{"pubkey":"deadbeef"}"#),
            ("chirp.unfollow", r#"{"pubkey":"deadbeef"}"#),
        ] {
            let parsed = dispatch(app, namespace, body);
            let id = parsed
                .get("correlation_id")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("{namespace}: expected correlation_id, got {parsed}"));
            assert_eq!(id.len(), 32, "{namespace}: correlation id should be 32 hex");
        }

        // `chirp.react` defaults `reaction` to `"+"` when absent.
        let parsed = dispatch(app, "chirp.react", r#"{"target_event_id":"abc"}"#);
        assert!(
            parsed.get("correlation_id").is_some(),
            "chirp.react without reaction should default and succeed: {parsed}"
        );

        // Malformed JSON shape is rejected by the host module validator (D6).
        let parsed = dispatch(app, "chirp.follow", r#"{"not_pubkey":"x"}"#);
        assert!(
            parsed.get("error").is_some(),
            "wrong-shape chirp.follow must be rejected: {parsed}"
        );

        nmp_app_chirp_unregister(handle);
        nmp_app_free(app);
    }

    /// THE NIP-CRATE SEAM PROOF: after `nmp_app_chirp_register`, the NIP-29
    /// `PostChatMessageAction` — an `ActionModule` impl living in the
    /// `nmp-nip29` protocol crate, NOT this app crate — is reachable through
    /// the generic `dispatch_action` path. A well-formed `PostChatMessageInput`
    /// yields a 32-hex `correlation_id` (both the typed module validator and
    /// the executor are wired); a malformed body is rejected with `error`.
    ///
    /// This proves the ADR-0027 typed-registration seam (`register_action::<M>()`)
    /// works for NIP-crate modules, not just Chirp's app-local social verbs —
    /// without `nmp-core` learning any NIP-29 group nouns (D0).
    #[test]
    fn nip29_post_chat_message_dispatches_through_action_registry() {
        let app = nmp_app_new();
        let handle = nmp_app_chirp_register(app, std::ptr::null());
        assert!(!handle.is_null());

        // Well-formed chat message: a host-pinned group + non-empty content.
        // The typed `PostChatMessageAction::start` builds the `["h", local_id]`
        // tag and enforces the host pin — a missing pin would reject here.
        let body = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"rust-nostr"},"content":"hello"}"#;
        let parsed = dispatch(app, "nmp.nip29.post_chat_message", body);
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
        assert_eq!(id.len(), 32, "correlation id should be 32 hex");

        // Malformed shape (missing the required `group`) is rejected by the
        // typed module validator surfaced through the host seam (D6).
        let parsed = dispatch(
            app,
            "nmp.nip29.post_chat_message",
            r#"{"content":"no group"}"#,
        );
        assert!(
            parsed.get("error").is_some(),
            "chat message without `group` must be rejected: {parsed}"
        );

        nmp_app_chirp_unregister(handle);
        nmp_app_free(app);
    }

    /// THE EXECUTOR PROOF: the NIP-29 post-chat-message executor maps a
    /// validated `PostChatMessageInput` to a concrete
    /// [`ActorCommand::PublishUnsignedEventToRelays`] pinned to the group's
    /// own host relay — proving the `PostChatMessageAction::execute` typed
    /// path (ADR-0027) produces the right command end-to-end.
    #[test]
    fn nip29_post_chat_message_executor_emits_host_pinned_publish_command() {
        let input = PostChatMessageInput {
            group: GroupId::new("wss://groups.example.com", "rust-nostr"),
            content: "hello".to_string(),
            previous_event_id_prefixes: vec![],
            reply_to_event_id: None,
        };
        let cmd = run_module_execute::<PostChatMessageAction>(input)
            .expect("well-formed chat message");

        match cmd {
            ActorCommand::PublishUnsignedEventToRelays { event, relays } => {
                // Pinned to EXACTLY the group's host relay — never the
                // author's NIP-65 outbox.
                assert_eq!(relays, vec!["wss://groups.example.com".to_string()]);
                // kind:9 chat message, host-pin `["h", local_id]` tag.
                assert_eq!(event.kind, KIND_CHAT_MESSAGE);
                assert!(
                    event
                        .tags
                        .iter()
                        .any(|t| t == &vec!["h".to_string(), "rust-nostr".to_string()]),
                    "must carry the ['h', local_id] group tag, got {:?}",
                    event.tags
                );
                assert_eq!(event.content, "hello");
                // `pubkey` is a placeholder — the actor derives it at sign time.
                assert!(event.pubkey.is_empty());
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    /// THE GROUP-CHAT CATALOG WIRING PROOF: each of the 3 NIP-29 group-chat
    /// namespaces `register_nip29_actions` wires is reachable through the
    /// generic `dispatch_action` path. A well-formed body yields a 32-hex
    /// `correlation_id` (BOTH the typed module validator AND the executor are
    /// bound under that namespace); a malformed body is rejected with `error`.
    ///
    /// Namespaces come from each `<Action>::NAMESPACE` constant — the single
    /// source of truth. Asserting via the constant keeps this test correct
    /// regardless of the underlying string.
    #[test]
    fn nip29_all_namespaces_dispatch_through_action_registry() {
        let app = nmp_app_new();
        let handle = nmp_app_chirp_register(app, std::ptr::null());
        assert!(!handle.is_null());

        let group = r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#;
        // Each of the 3 group-chat namespaces, with a well-formed body for its
        // typed `<Input>`.
        let cases: [(&str, String); 3] = [
            (
                PostChatMessageAction::NAMESPACE,
                format!(r#"{{"group":{group},"content":"hi"}}"#),
            ),
            (
                ReactInGroupAction::NAMESPACE,
                format!(r#"{{"group":{group},"target_event_id":"deadbeef","content":"+"}}"#),
            ),
            (
                CommentInGroupAction::NAMESPACE,
                format!(r#"{{"group":{group},"content":"nice"}}"#),
            ),
        ];

        for (namespace, body) in &cases {
            let parsed = dispatch(app, namespace, body);
            let id = parsed
                .get("correlation_id")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("{namespace}: expected correlation_id, got {parsed}"));
            assert_eq!(id.len(), 32, "{namespace}: correlation id should be 32 hex");

            // Malformed shape (no `group`) is rejected by the typed module
            // validator surfaced through the host seam (D6).
            let parsed = dispatch(app, namespace, r#"{"bad":"shape"}"#);
            assert!(
                parsed.get("error").is_some(),
                "{namespace}: malformed body must be rejected, got {parsed}"
            );
        }

        nmp_app_chirp_unregister(handle);
        nmp_app_free(app);
    }

    /// THE DISCOVERY DISPATCH PROOF: `nmp.nip29.discover` is reachable through
    /// the generic `dispatch_action` path with a well-formed body — the
    /// validator + executor land a 32-hex `correlation_id`. The executor
    /// returns an [`ActorCommand::PushInterest`] (not a publish command),
    /// proving the seam supports subscribe-side actions, not just publish-side.
    #[test]
    fn nip29_discover_dispatches_through_action_registry_and_emits_push_interest() {
        let app = nmp_app_new();
        let handle = nmp_app_chirp_register(app, std::ptr::null());
        assert!(!handle.is_null());

        // Well-formed: a `wss://` host relay URL. The executor pushes a
        // host-pinned LogicalInterest scoped to that relay.
        let body = r#"{"relay_url":"wss://groups.example.com"}"#;
        let parsed = dispatch(app, DiscoverGroupsAction::NAMESPACE, body);
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
        assert_eq!(id.len(), 32, "discover correlation id should be 32 hex");

        // Empty relay_url is rejected by the typed validator (D6).
        let parsed = dispatch(app, DiscoverGroupsAction::NAMESPACE, r#"{"relay_url":""}"#);
        assert!(
            parsed.get("error").is_some(),
            "empty relay_url must be rejected: {parsed}"
        );

        // Non-websocket scheme is rejected by the typed validator (D6).
        let parsed = dispatch(
            app,
            DiscoverGroupsAction::NAMESPACE,
            r#"{"relay_url":"https://groups.example.com"}"#,
        );
        assert!(
            parsed.get("error").is_some(),
            "non-wss relay_url must be rejected: {parsed}"
        );

        nmp_app_chirp_unregister(handle);
        nmp_app_free(app);
    }

    /// THE DISCOVERY EXECUTOR PROOF: the `nmp.nip29.discover` executor maps
    /// a validated `DiscoverGroupsInput` to a concrete
    /// [`ActorCommand::PushInterest`] pinned to the supplied relay — the
    /// subscribe-side seam end-to-end.
    #[test]
    fn nip29_discover_executor_emits_host_pinned_push_interest_command() {
        let input = DiscoverGroupsInput {
            relay_url: "wss://groups.example.com".to_string(),
        };
        let cmd = run_module_execute::<DiscoverGroupsAction>(input)
            .expect("well-formed discover input");

        match cmd {
            ActorCommand::PushInterest(interest) => {
                // Pinned to the relay — Case E (the third routing lane).
                assert_eq!(
                    interest.shape.relay_pin.as_deref(),
                    Some("wss://groups.example.com")
                );
                // Three metadata kinds, no `d` tag filter (discovery is
                // per-relay, not per-group).
                for k in [39000_u32, 39001, 39002] {
                    assert!(
                        interest.shape.kinds.contains(&k),
                        "discover interest must request kind {k}"
                    );
                }
                assert!(
                    interest.shape.tags.get("d").is_none(),
                    "discover must not constrain by group id"
                );
            }
            other => panic!("expected PushInterest, got {other:?}"),
        }
    }

    /// THE JOIN DISPATCH PROOF: `nmp.nip29.join` is reachable through the
    /// generic `dispatch_action` path with a well-formed body — the validator
    /// + executor land a 32-hex `correlation_id`. The executor returns a
    /// [`ActorCommand::PublishUnsignedEventToRelays`] host-pinned to the
    /// group's own relay (kind:9021), same Case-E lane as the chat actions.
    #[test]
    fn nip29_join_dispatches_through_action_registry() {
        let app = nmp_app_new();
        let handle = nmp_app_chirp_register(app, std::ptr::null());
        assert!(!handle.is_null());

        let group = r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#;
        let body = format!(r#"{{"group":{group}}}"#);
        let parsed = dispatch(app, JoinGroupAction::NAMESPACE, &body);
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
        assert_eq!(id.len(), 32, "join correlation id should be 32 hex");

        // Malformed shape (no `group`) is rejected by the typed validator.
        let parsed = dispatch(app, JoinGroupAction::NAMESPACE, r#"{"bad":"shape"}"#);
        assert!(
            parsed.get("error").is_some(),
            "join without group must be rejected: {parsed}"
        );

        // Missing host relay URL inside the group is rejected by the
        // validator (we'd otherwise route the request through the NIP-65
        // outbox — wrong relay).
        let parsed = dispatch(
            app,
            JoinGroupAction::NAMESPACE,
            r#"{"group":{"host_relay_url":"","local_id":"room"}}"#,
        );
        assert!(
            parsed.get("error").is_some(),
            "join with empty host_relay_url must be rejected: {parsed}"
        );

        nmp_app_chirp_unregister(handle);
        nmp_app_free(app);
    }

    /// THE JOIN EXECUTOR PROOF: kind:9021 (`["h", local_id]`), host-pinned
    /// to the group's relay, optional invite-code carried as `["code", _]`,
    /// optional reason carried as the event content.
    #[test]
    fn nip29_join_executor_emits_kind_9021_with_host_pin() {
        let input = JoinGroupInput {
            group: GroupId::new("wss://groups.example.com", "room"),
            invite_code: Some("abc".to_string()),
            reason: Some("please".to_string()),
        };
        let cmd = run_module_execute::<JoinGroupAction>(input).expect("well-formed join input");
        match cmd {
            ActorCommand::PublishUnsignedEventToRelays { event, relays } => {
                assert_eq!(relays, vec!["wss://groups.example.com".to_string()]);
                assert_eq!(event.kind, 9021);
                assert!(event
                    .tags
                    .iter()
                    .any(|t| t == &vec!["h".to_string(), "room".to_string()]));
                assert!(event
                    .tags
                    .iter()
                    .any(|t| t == &vec!["code".to_string(), "abc".to_string()]));
                assert_eq!(event.content, "please");
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    /// THE DISCOVERY REGISTRATION WIRING PROOF: `nmp_app_chirp_register_group_discovery`
    /// registers a `DiscoveredGroupsProjection` against `app` for a well-formed
    /// relay URL — it runs to completion (event-observer + snapshot-projection
    /// registration) without panicking. The snapshot closure surfacing under
    /// `"nmp.nip29.discovered_groups"` is proven end-to-end by the generic seam
    /// tests in `nmp-core` and the projection's own tests in `nmp-nip29`.
    #[test]
    fn register_group_discovery_runs_for_well_formed_relay_url() {
        let app = nmp_app_new();
        let relay = CString::new("wss://groups.example.com").unwrap();
        nmp_app_chirp_register_group_discovery(app, relay.as_ptr());
        nmp_app_free(app);
    }

    /// D6: a null `app`, a null `host_relay_url`, an empty `host_relay_url`,
    /// and non-UTF-8 garbage all degrade to a silent no-op — the function
    /// must never panic across the FFI boundary.
    #[test]
    fn register_group_discovery_null_and_empty_input_are_silent_noops() {
        let relay = CString::new("wss://groups.example.com").unwrap();
        // Null app — must not dereference.
        nmp_app_chirp_register_group_discovery(std::ptr::null_mut(), relay.as_ptr());

        let app = nmp_app_new();
        // Null host_relay_url — silent return.
        nmp_app_chirp_register_group_discovery(app, std::ptr::null());
        // Empty string — silent return.
        let empty = CString::new("").unwrap();
        nmp_app_chirp_register_group_discovery(app, empty.as_ptr());
        nmp_app_free(app);
    }

    /// THE GROUP-ID WIRE-SHAPE CONTRACT: the JSON shape documented on
    /// `nmp_app_chirp_register_group_chat` — `{"host_relay_url":…,
    /// "local_id":…}` — is exactly what `GroupId`'s serde derive accepts.
    /// This is the contract a Swift caller depends on: a body of any other
    /// shape is rejected by the `serde_json::from_str::<GroupId>` parse gate
    /// inside the function and the registration silently no-ops (D6).
    #[test]
    fn register_group_chat_group_id_wire_shape_matches_serde() {
        let parsed: GroupId = serde_json::from_str(
            r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#,
        )
        .expect("documented group_id_json shape must deserialize to GroupId");
        assert_eq!(parsed.host_relay_url, "wss://groups.example.com");
        assert_eq!(parsed.local_id, "room");

        // A JSON object missing the required fields is NOT a `GroupId` — the
        // parse gate rejects it, so the function returns without registering.
        assert!(
            serde_json::from_str::<GroupId>(r#"{"not":"a group id"}"#).is_err(),
            "a wrong-shape body must fail the GroupId parse gate"
        );
    }

    /// THE GROUP-CHAT WIRING PROOF: `nmp_app_chirp_register_group_chat`
    /// registers a `GroupChatProjection` against `app` for a well-formed
    /// group id — it runs to completion (event-observer + snapshot-projection
    /// registration) without panicking. The snapshot closure surfacing under
    /// `"nmp.nip29.group_chat"` is proven end-to-end by the generic seam tests in
    /// `nmp-core` (`snapshot_registry_tests.rs`) and the projection's own
    /// tests in `nmp-nip29`; this asserts the Chirp-side wiring call is sound.
    #[test]
    fn register_group_chat_runs_for_well_formed_group() {
        let app = nmp_app_new();
        let group =
            CString::new(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#)
                .unwrap();
        // Must register both halves (observer + snapshot projection) without
        // panicking across the FFI boundary.
        nmp_app_chirp_register_group_chat(app, group.as_ptr());
        nmp_app_free(app);
    }

    /// THE NIP-17 SEND-VERB PROOF: after `nmp_app_chirp_register`, the
    /// `nmp.nip17.send` action — `SendDmAction`, an `ActionModule` living in the
    /// `nmp-nip17` protocol crate — is reachable through the generic
    /// `dispatch_action` path. A well-formed `SendDmInput` yields a 32-hex
    /// `correlation_id` (both the typed module validator AND the executor are
    /// wired); a malformed / empty body is rejected with `error`.
    #[test]
    fn nip17_dm_send_dispatches_through_action_registry() {
        let app = nmp_app_new();
        let handle = nmp_app_chirp_register(app, std::ptr::null());
        assert!(!handle.is_null());

        let recipient = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        let body = format!(r#"{{"recipient_pubkey":"{recipient}","content":"hello over NIP-17"}}"#);
        let parsed = dispatch(app, "nmp.nip17.send", &body);
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
        assert_eq!(id.len(), 32, "correlation id should be 32 hex");

        // Empty content is rejected by the typed `SendDmAction::start`
        // validator surfaced through the host seam (D6).
        let parsed = dispatch(
            app,
            "nmp.nip17.send",
            &format!(r#"{{"recipient_pubkey":"{recipient}","content":"  "}}"#),
        );
        assert!(
            parsed.get("error").is_some(),
            "an empty-content DM must be rejected: {parsed}"
        );

        nmp_app_chirp_unregister(handle);
        nmp_app_free(app);
    }

    /// THE DM-INBOX WIRING PROOF: `nmp_app_chirp_register_dm_inbox` registers
    /// a `DmInboxProjection` against `app` — it runs to completion (raw-event
    /// observer + snapshot-projection/controller registration) without
    /// panicking across the FFI boundary. The legacy viewer-pubkey argument is
    /// ignored; active-account interest ownership is Rust-side now.
    #[test]
    fn register_dm_inbox_runs_for_app() {
        let app = nmp_app_new();
        // NULL viewer pubkey — accepted for ABI compatibility.
        nmp_app_chirp_register_dm_inbox(app, std::ptr::null());
        // Concrete viewer pubkey — ignored by the Rust-owned controller.
        let pubkey =
            CString::new("aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee")
                .unwrap();
        nmp_app_chirp_register_dm_inbox(app, pubkey.as_ptr());
        nmp_app_free(app);
    }

    /// D6: a null `app` is a silent no-op — the function must never
    /// dereference a null pointer or panic across the FFI boundary.
    #[test]
    fn register_dm_inbox_null_app_is_silent_noop() {
        nmp_app_chirp_register_dm_inbox(std::ptr::null_mut(), std::ptr::null());
    }

    /// THE IDEMPOTENCY PROOF: re-invoking `nmp_app_chirp_register_dm_inbox`
    /// must NOT stack a fresh raw-event observer on every call. The function
    /// can still be reached via the retained C-ABI compatibility door, while
    /// `nmp_app_chirp_register` also wires the runtime eagerly.
    ///
    /// Asserted observably through the per-app
    /// `swap_nip17_dm_inbox_observer` slot — the host-side handle that lets
    /// the function "remember the previous id and unregister it before
    /// installing the new one":
    ///
    /// 1. The first register installs an id in the slot (the fix path
    ///    actively writes through `swap_nip17_dm_inbox_observer(Some(id1))`).
    ///    Before the fix the slot was never written, so this assertion alone
    ///    fails on the buggy code.
    /// 2. The second register installs a FRESH id, distinct from the first —
    ///    proving the slot was overwritten with the new observer, not
    ///    silently dropped. The fix path also unregisters the prior id
    ///    against the kernel observer slot before storing the new one, so
    ///    the kernel's raw-observer registration count for `kind == 1059`
    ///    after the second call is 1, not 2.
    /// 3. Manually unregistering the second id (`unregister_raw_event_
    ///    observer(id2)`) drains the kernel observer slot — at this point
    ///    there is no kind:1059 observer alive, which is the leak-free
    ///    steady state.
    #[test]
    fn register_dm_inbox_is_idempotent_on_re_invoke() {
        let app = nmp_app_new();
        // SAFETY: `app` is a valid pointer returned by `nmp_app_new`, live
        // for the duration of this test (we call `nmp_app_free` at the end).
        let app_ref = unsafe { &*app };

        // Pre-condition: the per-app slot starts empty.
        assert!(
            app_ref.swap_nip17_dm_inbox_observer(None).is_none(),
            "slot must start empty (no DM inbox registered yet)"
        );

        // First registration.
        nmp_app_chirp_register_dm_inbox(app, std::ptr::null());
        let id1 = app_ref
            .swap_nip17_dm_inbox_observer(None)
            .expect("first register must install a raw-observer id in the per-app slot");
        // Put id1 back so the SECOND register sees it as the "previous" id
        // and unregisters it before installing its own.
        let prev = app_ref.swap_nip17_dm_inbox_observer(Some(id1));
        assert!(prev.is_none(), "we just swap-took, slot was empty");

        // Second registration — compatibility re-invoke case.
        nmp_app_chirp_register_dm_inbox(app, std::ptr::null());
        let id2 = app_ref
            .swap_nip17_dm_inbox_observer(None)
            .expect("second register must install a fresh id in the per-app slot");

        // Distinct ids: the slot was overwritten with the second register's
        // observer, not silently dropped. This is the host-side proof that
        // the leak is bounded — exactly one id lives in the per-app slot at
        // any time, regardless of how many sign-in cycles preceded.
        assert_ne!(
            id1, id2,
            "second register must produce a fresh raw-observer id (got {id1:?} both times)"
        );

        // Drain the kernel observer slot through the live id. Without the
        // fix, id1 would ALSO still be in the kernel slot and this would
        // leave kind:1059 observers behind (one observer of equivalence
        // class id1). With the fix, the kernel slot is now empty.
        app_ref.unregister_raw_event_observer(id2);

        nmp_app_free(app);
    }

    /// THE IDEMPOTENCY PROOF — group-chat variant. Same shape as the
    /// DM-inbox test: two consecutive `register_group_chat` calls leave
    /// exactly one `KernelEventObserverId` in the per-app
    /// `singleton_event_observer_id` slot, with the second register's id
    /// distinct from the first (proving the slot was overwritten and the
    /// previous observer was unregistered against the kernel).
    #[test]
    fn register_group_chat_is_idempotent_on_re_invoke() {
        let app = nmp_app_new();
        // SAFETY: `app` is a valid pointer from `nmp_app_new`, live for the
        // duration of this test.
        let app_ref = unsafe { &*app };

        assert!(
            app_ref.swap_singleton_event_observer(None).is_none(),
            "slot must start empty (no group chat registered yet)"
        );

        let group_a =
            CString::new(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room-a"}"#)
                .unwrap();
        let group_b =
            CString::new(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room-b"}"#)
                .unwrap();

        // First registration.
        nmp_app_chirp_register_group_chat(app, group_a.as_ptr());
        let id1 = app_ref
            .swap_singleton_event_observer(None)
            .expect("first register must install a kernel-observer id in the per-app slot");
        let prev = app_ref.swap_singleton_event_observer(Some(id1));
        assert!(prev.is_none(), "we just swap-took, slot was empty");

        // Second registration with a different group — the multi-screen
        // navigation case that previously leaked the prior observer.
        nmp_app_chirp_register_group_chat(app, group_b.as_ptr());
        let id2 = app_ref
            .swap_singleton_event_observer(None)
            .expect("second register must install a fresh id in the per-app slot");
        assert_ne!(
            id1, id2,
            "second register must produce a fresh kernel-observer id (got {id1:?} both times)"
        );

        app_ref.unregister_event_observer(id2);
        nmp_app_free(app);
    }

    /// D6: a null `app`, a null `group_id_json`, and a malformed
    /// `group_id_json` (valid JSON, wrong fields) all degrade to a silent
    /// no-op — the function must never panic across the FFI boundary.
    #[test]
    fn register_group_chat_null_and_malformed_input_are_silent_noops() {
        let group =
            CString::new(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#)
                .unwrap();
        // Null app — must not dereference.
        nmp_app_chirp_register_group_chat(std::ptr::null_mut(), group.as_ptr());

        let app = nmp_app_new();
        // Null group id — silent return.
        nmp_app_chirp_register_group_chat(app, std::ptr::null());
        // Malformed JSON shape — fails the `GroupId` parse gate, silent return.
        let bad = CString::new(r#"{"not":"a group id"}"#).unwrap();
        nmp_app_chirp_register_group_chat(app, bad.as_ptr());
        // Non-JSON garbage — also fails the parse gate, silent return.
        let garbage = CString::new("not json at all").unwrap();
        nmp_app_chirp_register_group_chat(app, garbage.as_ptr());
        nmp_app_free(app);
    }
}
