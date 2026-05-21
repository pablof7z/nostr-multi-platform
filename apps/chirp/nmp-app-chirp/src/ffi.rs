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
//!   (ingest) plus a `"nip29.group_chat"` snapshot projection (output). Pure
//!   consumption — no handle, no actions, no unregister.
//! - [`nmp_app_chirp_register_dm_inbox`] — wire a NIP-17 `DmInboxProjection`
//!   into the kernel: a kind:1059 raw-event observer (ingest), a
//!   `"nip17.dm_inbox"` snapshot projection (output), and a kind:1059 `#p`
//!   gift-wrap inbox interest so envelopes actually arrive.
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
use std::sync::Arc;

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::{
    ActorCommand, KernelEventObserver, KernelEventObserverId, NmpApp, RawEventObserver,
};
use nmp_nip29::group_id::GroupId;
use nmp_nip29::projection::GroupChatProjection;
use nmp_nip29::action::{
    comment_in_group_command, post_chat_message_command, react_in_group_command,
    CommentInGroupAction, CommentInGroupInput, PostChatMessageAction, PostChatMessageInput,
    ReactInGroupAction, ReactInGroupInput,
};
use nmp_nip17::{
    giftwrap_inbox_interest, publish_dm_relay_list_command, send_dm_command, DmInboxProjection,
    PublishDmRelayListAction, PublishDmRelayListInput, SendDmAction, SendDmInput,
};
use nmp_nip01::meta_timeline::Pubkey;
use nmp_nip01::{ModularTimelineProjection, ModularTimelineSpec};
use nmp_threading::ModulePolicy;

/// Register one typed `ActionModule` against `$app`'s action registry.
///
/// The validator half calls `$Action::start` (typed, uses the same `build_plan`
/// closure the executor uses); the executor half delegates to `$command`, a
/// crate-level `fn(&str) -> Result<ActorCommand, String>` that builds the
/// `UnsignedEvent` and returns a `PublishUnsignedEventToRelays` command.
///
/// Using the macro keeps the namespace string written once (`$Action::NAMESPACE`)
/// so the validator and executor can never be mismatched.
macro_rules! wire_action {
    ($app:ident, $Action:ident, $Input:ident, $command:ident) => {{
        $app.register_action_module($Action::NAMESPACE, |action_json| {
            let action: $Input = serde_json::from_str(action_json)
                .map_err(|e| ActionRejection::Invalid(e.to_string()))?;
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let mut ctx = ActionContext { now_ms };
            $Action::start(&mut ctx, action)
        });
        $app.register_action_executor(
            $Action::NAMESPACE,
            |action_json, _correlation_id, send| {
                let cmd = $command(action_json)?;
                send(cmd);
                Ok(())
            },
        );
    }};
}

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

    // SAFETY: caller guarantees `app` is a valid pointer allocated by
    // `nmp_app_new` for the duration of this call. We do not hold the
    // borrow past this function.
    let app_ref = unsafe { &*app };

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
/// `"nip29.group_chat"` (output). The group's chat messages then surface in
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
/// this returns no handle, so there is no companion `unregister`. Calling it
/// twice overwrites the `"nip29.group_chat"` snapshot key with the newer
/// projection and leaves the older event observer registered for the life of
/// the `app` (a small, bounded leak). Chirp's group-chat screen registers
/// exactly one group per `app`, so this is acceptable; a multi-group host
/// would need a handle-returning variant.
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
    let observer_id = app_ref
        .register_event_observer(Arc::clone(&projection) as Arc<dyn KernelEventObserver>);
    if observer_id.0 == 0 {
        // Observer registration failed (poisoned slot). Don't register the
        // snapshot closure for a projection that will never receive events.
        return;
    }

    // Output side: the no-argument snapshot read runs on the actor thread
    // inside each snapshot tick. The `move` consumes this last `Arc`.
    app_ref.register_snapshot_projection("nip29.group_chat", move || projection.snapshot_json());
}

/// Wire a NIP-17 [`DmInboxProjection`] for the local account into `app`.
///
/// This is the **receive side** of NIP-17 private DMs. It constructs a
/// `DmInboxProjection` bound to the kernel's shared local-keys slot
/// (`NmpApp::nip17_local_keys`), plugs it in as a [`RawEventObserver`]
/// (kind:1059 tap — ingest), and registers its `snapshot_json` read under the
/// snapshot key `"nip17.dm_inbox"` (output). Decrypted conversations then
/// surface on every kernel snapshot tick under that key.
///
/// Unlike the NIP-29 group-chat projection there is no `GroupId` argument —
/// the DM inbox is global (every conversation the local account participates
/// in).
///
/// `viewer_pubkey` is the active account's hex pubkey. When non-null and
/// non-empty it is used to push a kind:1059 `#p <pubkey>` gift-wrap inbox
/// interest so the kernel actually opens a REQ for incoming envelopes —
/// WITHOUT it the projection is wired but inert (no events ever arrive).
/// NULL is permitted (app-startup call before sign-in); in that case the
/// projection still decrypts once the shared key slot is populated, but no
/// REQ is opened until the caller re-invokes with a concrete pubkey.
///
/// CALLER CONTRACT — the host MUST re-invoke this after sign-in / account
/// switch so the kind:1059 interest is pushed for the now-active account. The
/// interest id is deterministic per-pubkey, so a re-invoke for the same
/// account is an idempotent no-op; a re-invoke for a new account adds that
/// account's interest.
///
/// D6 — fire-and-forget. A null `app` or a poisoned observer slot degrades to
/// a silent return.
///
/// SCOPE — single-use, no unregister. Calling it twice registers a second
/// event observer (a small, bounded leak) and overwrites the
/// `"nip17.dm_inbox"` snapshot key with the newer projection. Chirp calls it
/// once at startup (then re-invokes only to push the interest after sign-in —
/// idempotent on the projection side because the snapshot key is just
/// overwritten with an equivalent projection).
///
/// `app` MUST outlive the registration; it is only borrowed for this call.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_register_dm_inbox(
    app: *mut NmpApp,
    viewer_pubkey: *const c_char,
) {
    if app.is_null() {
        return;
    }
    // SAFETY: caller guarantees `app` is a valid pointer from `nmp_app_new`,
    // live for the duration of this call. The borrow is not held past return.
    let app_ref = unsafe { &*app };

    // The projection reads the kernel's shared local-keys slot to decrypt
    // each incoming gift-wrap — it needs no pubkey argument itself.
    let local_keys = app_ref.nip17_local_keys();
    let projection = Arc::new(DmInboxProjection::new(local_keys));

    // Ingest side: the kind:1059 raw-event tap. `Arc::clone … as Arc<dyn …>`
    // (NOT `Arc::new(projection)`, which would double-box).
    let observer_id = app_ref.register_raw_event_observer(
        DmInboxProjection::kind_filter(),
        Arc::clone(&projection) as Arc<dyn RawEventObserver>,
    );
    if observer_id.0 == 0 {
        // Raw-observer registration failed (poisoned slot). Don't register
        // the snapshot closure for a projection that will never see events.
        return;
    }

    // Output side: the no-argument snapshot read runs on the actor thread
    // inside each snapshot tick. The `move` consumes this last `Arc`.
    app_ref.register_snapshot_projection("nip17.dm_inbox", move || {
        projection.snapshot_json()
    });

    // Push the kind:1059 `#p <pubkey>` gift-wrap inbox interest so the kernel
    // opens a REQ for incoming envelopes. Without this the projection is
    // registered but inert. A NULL / empty pubkey means "not signed in yet"
    // — the caller re-invokes after sign-in (see the CALLER CONTRACT above).
    // The interest id is deterministic per-pubkey, so the re-invoke de-dupes.
    if let Some(pubkey) = c_string_opt(viewer_pubkey).filter(|s| !s.is_empty()) {
        app_ref.push_interest(giftwrap_inbox_interest(&pubkey));
    }
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
/// Namespace constants for Chirp-specific action verbs. Written once here so
/// the `register_action_module` and `register_action_executor` calls in
/// `register_chirp_actions` stay in sync — a mismatch would register a
/// validator under one name and an executor under another, silently breaking
/// dispatch. Mirrors the `$Action::NAMESPACE` convention from `wire_action!`.
const NS_REACT: &str = "chirp.react";
const NS_FOLLOW: &str = "chirp.follow";
const NS_UNFOLLOW: &str = "chirp.unfollow";

fn register_chirp_actions(app: &mut NmpApp) {
    // chirp.react — kind:7 reaction.
    app.register_action_module(NS_REACT, |action_json| {
        serde_json::from_str::<ReactAction>(action_json)
            .map(|_| ())
            .map_err(|e| ActionRejection::Invalid(e.to_string()))
    });
    app.register_action_executor(NS_REACT, |action_json, _correlation_id, send| {
        let a: ReactAction =
            serde_json::from_str(action_json).map_err(|e| e.to_string())?;
        send(ActorCommand::React {
            target_event_id: a.target_event_id,
            reaction: a.reaction,
        });
        Ok(())
    });

    // chirp.follow — append `pubkey` to the active account's kind:3 set.
    app.register_action_module(NS_FOLLOW, |action_json| {
        serde_json::from_str::<PubkeyAction>(action_json)
            .map(|_| ())
            .map_err(|e| ActionRejection::Invalid(e.to_string()))
    });
    app.register_action_executor(NS_FOLLOW, |action_json, _correlation_id, send| {
        let a: PubkeyAction =
            serde_json::from_str(action_json).map_err(|e| e.to_string())?;
        send(ActorCommand::Follow { pubkey: a.pubkey });
        Ok(())
    });

    // chirp.unfollow — remove `pubkey` from the kind:3 set.
    app.register_action_module(NS_UNFOLLOW, |action_json| {
        serde_json::from_str::<PubkeyAction>(action_json)
            .map(|_| ())
            .map_err(|e| ActionRejection::Invalid(e.to_string()))
    });
    app.register_action_executor(NS_UNFOLLOW, |action_json, _correlation_id, send| {
        let a: PubkeyAction =
            serde_json::from_str(action_json).map_err(|e| e.to_string())?;
        send(ActorCommand::Unfollow { pubkey: a.pubkey });
        Ok(())
    });
}

/// Register the 3 NIP-29 group-chat action namespaces against `app`'s action
/// registry.
///
/// This wires `ActionModule` impls that live in the `nmp-nip29` protocol
/// crate rather than in this app crate, proving the host-extensibility seam
/// (`register_action_module` / `register_action_executor`) is not limited to
/// Chirp's own social verbs — any NIP crate's typed `ActionModule` can be
/// reached through the generic `dispatch_action` path, without `nmp-core`
/// learning any NIP-29 group nouns (D0).
///
/// Every namespace gets BOTH halves `nmp_app_dispatch_action` requires:
///
/// * the **module** validator delegates straight to the typed
///   `<Action>::start` — the crate's real validation (host-pin enforcement,
///   tag construction, `validate_no_unpinned_h`) runs; it is not re-imitated
///   here;
/// * the **executor** delegates to the typed `<name>_command` function from
///   `nmp-nip29`. That function consults the SAME `build_plan` closure the
///   validator uses, so the wire shape can never drift between validate and
///   execute. It builds the action's `UnsignedEvent` and enqueues
///   [`ActorCommand::PublishUnsignedEventToRelays`], host-pinned to the
///   group's own relay (`PublishPlan::pin_to`). The actor signs with the
///   active account and routes to exactly that relay, bypassing the NIP-65
///   outbox resolver — a group event must reach the group's host relay,
///   never the author's kind:10002 outbox.
///
/// Namespaces come from each `<Action>::NAMESPACE` constant — the single
/// source of truth: `nmp.nip29.post_chat_message`, `nmp.nip29.react_in_group`,
/// `nmp.nip29.comment_in_group`. The shared [`wire_action!`] macro ensures
/// validator and executor are always registered against the same constant,
/// preventing namespace mismatch.
///
/// SCOPE: NIP-29 ships only its relay-group chat surface in v1 — the admin /
/// membership / artifact / discussion / share executors were deleted (no
/// group-administration UI is planned; Marmot MLS covers private groups).
fn register_nip29_actions(app: &mut NmpApp) {
    wire_action!(app, PostChatMessageAction, PostChatMessageInput, post_chat_message_command);
    wire_action!(app, ReactInGroupAction, ReactInGroupInput, react_in_group_command);
    wire_action!(app, CommentInGroupAction, CommentInGroupInput, comment_in_group_command);
}

/// Register the NIP-17 direct-message `ActionModule` (`nmp.nip17.send`) against
/// `app`'s action registry.
///
/// Wires the typed [`SendDmAction`] from the `nmp-nip17` protocol crate
/// through the same host-extensibility seam the NIP-29 actions use. The
/// executor delegates to `nmp_nip17::send_dm_command`, which builds the
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
/// `ActorCommand::PublishUnsignedEventToRelays` with an EMPTY relay set —
/// kind:10050 is a NIP-65 replaceable event and the actor routes empty-relay
/// publishes through the NIP-65 outbox (the author's kind:10002 write relays).
fn register_nip17_actions(app: &mut NmpApp) {
    wire_action!(app, SendDmAction, SendDmInput, send_dm_command);
    wire_action!(
        app,
        PublishDmRelayListAction,
        PublishDmRelayListInput,
        publish_dm_relay_list_command
    );
}

/// `chirp.react` action body: `{"target_event_id":"<hex>","reaction":"+"}`.
/// `reaction` defaults to `"+"` (the standard kind:7 like) when absent —
/// matching the old `nmp_app_react` FFI symbol's `unwrap_or("+")` behaviour.
#[derive(serde::Deserialize)]
struct ReactAction {
    target_event_id: String,
    #[serde(default = "default_reaction")]
    reaction: String,
}

fn default_reaction() -> String {
    "+".to_string()
}

/// `chirp.follow` / `chirp.unfollow` action body: `{"pubkey":"<hex>"}`.
#[derive(serde::Deserialize)]
struct PubkeyAction {
    pubkey: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::nmp_app_free;
    use nmp_core::nmp_app_new;
    use nmp_nip29::kinds::KIND_CHAT_MESSAGE;

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
    /// This proves the host-extensibility seam (`register_action_module` /
    /// `register_action_executor`) works for NIP-crate modules, not just
    /// Chirp's app-local social verbs — without `nmp-core` learning any
    /// NIP-29 group nouns (D0).
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
        let parsed = dispatch(app, "nmp.nip29.post_chat_message", r#"{"content":"no group"}"#);
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
    /// own host relay — proving the executor → actor channel is connected
    /// end-to-end (the `register_action_executor` closure is a thin
    /// `send(cmd)` shim over the function exercised here).
    #[test]
    fn nip29_post_chat_message_executor_emits_host_pinned_publish_command() {
        let body = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"rust-nostr"},"content":"hello"}"#;
        let cmd = post_chat_message_command(body).expect("well-formed chat message");

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

    /// A malformed body (missing the required `group`) is rejected — the
    /// executor never fabricates a command from an unverified shape (D6).
    #[test]
    fn nip29_post_chat_message_executor_rejects_malformed_body() {
        assert!(
            post_chat_message_command(r#"{"content":"no group"}"#).is_err(),
            "chat message without `group` must be rejected"
        );
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
    /// `"nip29.group_chat"` is proven end-to-end by the generic seam tests in
    /// `nmp-core` (`snapshot_registry_tests.rs`) and the projection's own
    /// tests in `nmp-nip29`; this asserts the Chirp-side wiring call is sound.
    #[test]
    fn register_group_chat_runs_for_well_formed_group() {
        let app = nmp_app_new();
        let group = CString::new(
            r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#,
        )
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

        let recipient =
            "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        let body = format!(
            r#"{{"recipient_pubkey":"{recipient}","content":"hello over NIP-17"}}"#
        );
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
    /// observer + snapshot-projection registration + interest push) without
    /// panicking across the FFI boundary, with and without a viewer pubkey.
    #[test]
    fn register_dm_inbox_runs_for_app() {
        let app = nmp_app_new();
        // NULL viewer pubkey — startup-before-sign-in call.
        nmp_app_chirp_register_dm_inbox(app, std::ptr::null());
        // Concrete viewer pubkey — pushes the kind:1059 `#p` interest.
        let pubkey = CString::new(
            "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee",
        )
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

    /// D6: a null `app`, a null `group_id_json`, and a malformed
    /// `group_id_json` (valid JSON, wrong fields) all degrade to a silent
    /// no-op — the function must never panic across the FFI boundary.
    #[test]
    fn register_group_chat_null_and_malformed_input_are_silent_noops() {
        let group = CString::new(
            r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#,
        )
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
