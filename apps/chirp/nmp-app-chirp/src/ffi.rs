//! Chirp per-app FFI surface.
//!
//! Four `extern "C"` symbols Swift links against:
//!
//! - [`nmp_app_chirp_register`] — instantiate `ChirpModularTimeline` with the
//!   active viewer pubkey and register it as a kernel event observer on the
//!   supplied `NmpApp`. Returns an opaque handle (boxed projection +
//!   observer id) for later snapshots / unregister.
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

use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPlan, ActionRejection, ActionStatus, UnsignedEvent,
};
use nmp_core::{ActorCommand, KernelEventObserverId, NmpApp};
use nmp_nip29::action::{JoinRequestAction, JoinRequestInput};
use nmp_nip29::kinds::KIND_JOIN_REQUEST;
use nmp_nip01::meta_timeline::Pubkey;
use nmp_nip01::{ModularTimelineProjection, ModularTimelineSpec};
use nmp_threading::ModulePolicy;

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

    // Register the NIP-29 `JoinRequestAction` module against the kernel.
    // Unlike `register_chirp_actions` (Chirp's own social verbs), this wires
    // an `ActionModule` impl that lives in the `nmp-nip29` protocol crate —
    // proving the host-extensibility seam works for NIP-crate modules too,
    // not just app-local verbs. Same `&mut NmpApp` / pre-`nmp_app_start`
    // ordering rule as `register_chirp_actions` above.
    //
    // SAFETY: same exclusive-borrow rationale as `register_chirp_actions` —
    // no other reference aliases `app` at this point.
    register_nip29_actions(unsafe { &mut *app });

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

/// A trivially-`Pending` [`ActionPlan`] for a fire-and-forget social verb.
/// Chirp's social verbs (`react` / `follow` / `unfollow`) have no multi-step
/// lifecycle — the action is accepted, an `ActorCommand` is enqueued, and the
/// outcome surfaces through the kernel snapshot. So every module validator
/// here returns the same shape: `initial_status: Pending`, no deadline.
fn fire_and_forget_plan() -> ActionPlan<serde_json::Value> {
    ActionPlan {
        initial_step: serde_json::Value::String("Pending".to_string()),
        initial_status: ActionStatus::Pending,
        deadline_ms: None,
    }
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
fn register_chirp_actions(app: &mut NmpApp) {
    // chirp.react — kind:7 reaction.
    app.register_action_module("chirp.react", |action_json| {
        serde_json::from_str::<ReactAction>(action_json)
            .map(|_| fire_and_forget_plan())
            .map_err(|e| ActionRejection::Invalid(e.to_string()))
    });
    app.register_action_executor("chirp.react", |action_json, _correlation_id, send| {
        let a: ReactAction =
            serde_json::from_str(action_json).map_err(|e| e.to_string())?;
        send(ActorCommand::React {
            target_event_id: a.target_event_id,
            reaction: a.reaction,
        });
        Ok(())
    });

    // chirp.follow — append `pubkey` to the active account's kind:3 set.
    app.register_action_module("chirp.follow", |action_json| {
        serde_json::from_str::<PubkeyAction>(action_json)
            .map(|_| fire_and_forget_plan())
            .map_err(|e| ActionRejection::Invalid(e.to_string()))
    });
    app.register_action_executor("chirp.follow", |action_json, _correlation_id, send| {
        let a: PubkeyAction =
            serde_json::from_str(action_json).map_err(|e| e.to_string())?;
        send(ActorCommand::Follow { pubkey: a.pubkey });
        Ok(())
    });

    // chirp.unfollow — remove `pubkey` from the kind:3 set.
    app.register_action_module("chirp.unfollow", |action_json| {
        serde_json::from_str::<PubkeyAction>(action_json)
            .map(|_| fire_and_forget_plan())
            .map_err(|e| ActionRejection::Invalid(e.to_string()))
    });
    app.register_action_executor("chirp.unfollow", |action_json, _correlation_id, send| {
        let a: PubkeyAction =
            serde_json::from_str(action_json).map_err(|e| e.to_string())?;
        send(ActorCommand::Unfollow { pubkey: a.pubkey });
        Ok(())
    });
}

/// Register NIP-29 group action namespaces against `app`'s action registry.
///
/// This is the first wiring of an `ActionModule` impl that lives in a NIP
/// protocol crate (`nmp-nip29`) rather than in this app crate. It proves the
/// host-extensibility seam (`register_action_module` / `register_action_executor`)
/// is not limited to Chirp's own social verbs — any NIP crate's typed
/// `ActionModule` can be reached through the generic `dispatch_action` path.
///
/// Only `nip29.join_request` is wired here (the namespace constant comes from
/// [`JoinRequestAction::NAMESPACE`], the single source of truth — the kernel
/// `nmp-core` is NOT edited to hardcode it). The other 14 `nmp-nip29`
/// `ActionModule` impls (`admin`, `content`, `composed`) follow the same
/// pattern when their app surfaces need them.
///
/// JSON schema (the third arg to `nmp_app_dispatch_action`):
/// * `nip29.join_request` —
///   `{"group":{"host_relay_url":"wss://…","local_id":"…"},
///     "invite_code":null,"referrer_event_id":null,"reason":null}`
///
/// The **module** validator delegates straight to the typed
/// [`JoinRequestAction::start`] — so the crate's real validation logic
/// (host-pin enforcement, tag construction, `validate_no_unpinned_h`) runs;
/// it is not re-imitated here. The resulting `ActionPlan<MembershipStep>` is
/// erased to `ActionPlan<serde_json::Value>` for the host seam.
///
/// The **executor** builds the kind:9021 join-request `UnsignedEvent` from the
/// typed `JoinRequestInput` and enqueues
/// [`ActorCommand::PublishUnsignedEventToRelays`], pinned to the group's own
/// host relay (`input.group.host_relay_url`). The actor signs with the active
/// account and routes to exactly that relay, bypassing the NIP-65 outbox
/// resolver — a group event must reach the group's host relay, never the
/// author's kind:10002 outbox. This is the first executor to drive a real
/// `ActorCommand` from a NIP-crate `ActionModule`.
fn register_nip29_actions(app: &mut NmpApp) {
    // Module (validator): delegate to the typed `nmp-nip29` `ActionModule`.
    app.register_action_module(JoinRequestAction::NAMESPACE, |action_json| {
        let action: JoinRequestInput = serde_json::from_str(action_json)
            .map_err(|e| ActionRejection::Invalid(e.to_string()))?;
        let mut ctx = ActionContext { now_ms: 0 };
        let plan = JoinRequestAction::start(&mut ctx, action)?;
        // Erase the typed `MembershipStep` to `serde_json::Value` for the
        // host-seam `ActionPlan` shape. `MembershipStep` is a unit struct so
        // this is infallible in practice; treat an encode error as invalid.
        let initial_step = serde_json::to_value(&plan.initial_step)
            .map_err(|e| ActionRejection::Invalid(e.to_string()))?;
        Ok(ActionPlan {
            initial_step,
            initial_status: plan.initial_status,
            deadline_ms: plan.deadline_ms,
        })
    });

    // Executor: build the kind:9021 join-request event and enqueue a
    // host-pinned publish. The actor signs with the active account and routes
    // to the group's own host relay (D3 Explicit opt-out). The closure is a
    // thin shim over `nip29_join_request_command` so the action→command
    // mapping is unit-testable without the FFI / actor channel.
    app.register_action_executor(
        JoinRequestAction::NAMESPACE,
        |action_json, _correlation_id, send| {
            let cmd = nip29_join_request_command(action_json)?;
            send(cmd);
            Ok(())
        },
    );
}

/// Map a validated `nip29.join_request` action JSON to the
/// [`ActorCommand::PublishUnsignedEventToRelays`] that publishes the kind:9021
/// join-request event, pinned to the group's own host relay.
///
/// Split out of the `register_action_executor` closure so the action→command
/// mapping is directly unit-testable (the closure half only adds the `send`
/// call). Returns `Err` on a malformed body — defence in depth, even though
/// the module validator already rejected it.
fn nip29_join_request_command(action_json: &str) -> Result<ActorCommand, String> {
    // Re-decode the typed input — the executor owns its own parse so it never
    // trusts an upstream shape it did not verify.
    let input: JoinRequestInput =
        serde_json::from_str(action_json).map_err(|e| e.to_string())?;

    // Build the kind:9021 tags. This mirrors `JoinRequestAction::start`
    // (`nmp-nip29/src/action/membership.rs`): an `["h", local_id]` group tag,
    // plus optional `["code", …]` and `["e", referrer]` rows. The tag layout
    // is the protocol crate's contract; until `ActionModule` exposes a
    // `PublishPlan`-returning executor hook this is a local re-build, not a
    // re-imagining of the validation logic.
    let mut tags: Vec<Vec<String>> =
        vec![vec!["h".to_string(), input.group.local_id.clone()]];
    if let Some(code) = input.invite_code.clone() {
        tags.push(vec!["code".to_string(), code]);
    }
    if let Some(evt) = input.referrer_event_id.clone() {
        tags.push(vec!["e".to_string(), evt]);
    }
    let content = input.reason.clone().unwrap_or_default();

    // `pubkey` is a placeholder — the actor derives it from the active
    // identity at sign time and overwrites this field (see
    // `ActorCommand::PublishUnsignedEventToRelays` / `publish_unsigned_*`).
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let unsigned = UnsignedEvent {
        pubkey: String::new(),
        kind: KIND_JOIN_REQUEST,
        tags,
        content,
        created_at,
    };

    // Pin to the group's own host relay — a NIP-29 group event must NOT route
    // via the author's NIP-65 outbox.
    Ok(ActorCommand::PublishUnsignedEventToRelays {
        event: unsigned,
        relays: vec![input.group.host_relay_url.clone()],
    })
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
    /// `JoinRequestAction` — an `ActionModule` impl living in the `nmp-nip29`
    /// protocol crate, NOT this app crate — is reachable through the generic
    /// `dispatch_action` path. A well-formed `JoinRequestInput` yields a
    /// 32-hex `correlation_id` (both the typed module validator and the
    /// executor are wired); a malformed body is rejected with `error`.
    ///
    /// This proves the host-extensibility seam (`register_action_module` /
    /// `register_action_executor`) works for NIP-crate modules, not just
    /// Chirp's app-local social verbs — without `nmp-core` learning any
    /// NIP-29 group nouns (D0).
    #[test]
    fn nip29_join_request_dispatches_through_action_registry() {
        let app = nmp_app_new();
        let handle = nmp_app_chirp_register(app, std::ptr::null());
        assert!(!handle.is_null());

        // Well-formed join request: a host-pinned group + optional fields.
        // The typed `JoinRequestAction::start` builds the `["h", local_id]`
        // tag and enforces the host pin — a missing pin would reject here.
        let body = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"rust-nostr"},"invite_code":"abc123","referrer_event_id":null,"reason":"hello"}"#;
        let parsed = dispatch(app, "nip29.join_request", body);
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
        assert_eq!(id.len(), 32, "correlation id should be 32 hex");

        // Minimal body — only the required `group` field; the rest default.
        let minimal = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"rust-nostr"}}"#;
        let parsed = dispatch(app, "nip29.join_request", minimal);
        assert!(
            parsed.get("correlation_id").is_some(),
            "minimal join request should succeed: {parsed}"
        );

        // Malformed shape (missing the required `group`) is rejected by the
        // typed module validator surfaced through the host seam (D6).
        let parsed = dispatch(app, "nip29.join_request", r#"{"reason":"no group"}"#);
        assert!(
            parsed.get("error").is_some(),
            "join request without `group` must be rejected: {parsed}"
        );

        nmp_app_chirp_unregister(handle);
        nmp_app_free(app);
    }

    /// THE EXECUTOR PROOF: the NIP-29 join-request executor is no longer a
    /// no-op — it maps a validated `JoinRequestInput` to a concrete
    /// [`ActorCommand::PublishUnsignedEventToRelays`] pinned to the group's
    /// own host relay. This is the first NIP-crate `ActionModule` executor to
    /// drive a real `ActorCommand`; it proves the executor → actor channel is
    /// connected end-to-end (the `register_action_executor` closure is a thin
    /// `send(cmd)` shim over the function exercised here).
    #[test]
    fn nip29_join_request_executor_emits_host_pinned_publish_command() {
        let body = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"rust-nostr"},"invite_code":"abc123","referrer_event_id":"deadbeef","reason":"hello"}"#;
        let cmd = nip29_join_request_command(body).expect("well-formed join request");

        match cmd {
            ActorCommand::PublishUnsignedEventToRelays { event, relays } => {
                // Pinned to EXACTLY the group's host relay — never the
                // author's NIP-65 outbox.
                assert_eq!(relays, vec!["wss://groups.example.com".to_string()]);
                // kind:9021 join request, host-pin `["h", local_id]` tag.
                assert_eq!(event.kind, KIND_JOIN_REQUEST);
                assert!(
                    event
                        .tags
                        .iter()
                        .any(|t| t == &vec!["h".to_string(), "rust-nostr".to_string()]),
                    "must carry the ['h', local_id] group tag, got {:?}",
                    event.tags
                );
                // Optional fields surface as `code` / `e` tags + content.
                assert!(event
                    .tags
                    .iter()
                    .any(|t| t == &vec!["code".to_string(), "abc123".to_string()]));
                assert!(event
                    .tags
                    .iter()
                    .any(|t| t == &vec!["e".to_string(), "deadbeef".to_string()]));
                assert_eq!(event.content, "hello");
                // `pubkey` is a placeholder — the actor derives it at sign time.
                assert!(event.pubkey.is_empty());
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    /// The minimal body (only the required `group`) still produces a valid
    /// host-pinned command: empty content, just the `["h", local_id]` tag.
    #[test]
    fn nip29_join_request_executor_handles_minimal_body() {
        let minimal = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"rust-nostr"}}"#;
        let cmd = nip29_join_request_command(minimal).expect("minimal join request");
        match cmd {
            ActorCommand::PublishUnsignedEventToRelays { event, relays } => {
                assert_eq!(relays, vec!["wss://groups.example.com".to_string()]);
                assert_eq!(event.kind, KIND_JOIN_REQUEST);
                assert_eq!(event.tags.len(), 1, "only the ['h', …] tag");
                assert_eq!(event.content, "");
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    /// A malformed body (missing the required `group`) is rejected — the
    /// executor never fabricates a command from an unverified shape (D6).
    #[test]
    fn nip29_join_request_executor_rejects_malformed_body() {
        assert!(
            nip29_join_request_command(r#"{"reason":"no group"}"#).is_err(),
            "join request without `group` must be rejected"
        );
    }
}
