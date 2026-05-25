//! The `pub extern "C"` registration entry points Swift links against to wire
//! Chirp projections (timeline, group chat, group discovery, DM inbox, follow
//! list) and action namespaces into an [`NmpApp`].

use std::ffi::c_char;
use std::sync::{Arc, Mutex};

use nmp_core::KernelEventObserver;
use nmp_ffi::NmpApp;
use nmp_nip01::meta_timeline::Pubkey;
use nmp_nip01::{ModularTimelineProjection, ModularTimelineSpec};
use nmp_nip29::group_id::GroupId;
use nmp_nip29::register::{wire_group_chat, wire_group_discovery};
use nmp_threading::ModulePolicy;

use nmp_nip02::FollowListProjection;

use super::actions::register_nip29_actions;
use super::handle::ChirpHandle;
use super::helpers::c_string_opt;

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

    // Inherit the canonical NMP composition through one call — NIP-02 /
    // NIP-17 / NIP-57 / NIP-65 action modules, the kind:10050 ingest
    // parser, the production routing substrate
    // (`GenericOutboxRouter` + `InMemoryMailboxCache`), the D2 coverage
    // hook, and the DM-inbox + zap-receipts runtime controllers. This
    // is the closure of V-48: a new Nostr app calls
    // `nmp_app_template::register_defaults` instead of re-deriving the
    // 130 LOC of wiring this used to live as.
    //
    // SAFETY: caller guarantees `app` is a valid pointer from
    // `nmp_app_new`. No other reference aliases it here — the `&*app`
    // borrow further down is taken only after this exclusive borrow is
    // dropped.
    nmp_app_template::register_defaults(unsafe { &mut *app });

    // Chirp-specific: register the NIP-29 group-chat `ActionModule`s
    // against the kernel. Lives in this crate (not the template) because
    // NIP-29 is not part of the canonical NMP composition every Nostr
    // app inherits — a notes-only app would not register it.
    //
    // SAFETY: same exclusive-borrow rationale as the
    // `register_defaults` call above — no other reference aliases `app`
    // at this point.
    register_nip29_actions(unsafe { &mut *app });

    // SAFETY: caller guarantees `app` is a valid pointer allocated by
    // `nmp_app_new` for the duration of this call. We do not hold the
    // borrow past this function.
    let app_ref = unsafe { &*app };

    // Wire the NIP-57 `ZapsAggregateProjection` — a `KernelEventObserver`
    // that indexes incoming kind:9735 zap receipts by their `["e", target]`
    // tag so a timeline surface can show per-row zap counts + total msats
    // without opening a per-target `ZapsView` for every visible note.
    //
    // Pure consumption — registers as an event observer (ingest) and exposes
    // its `snapshot_json` read under `"nmp.nip57.zaps"` (output). No
    // action, no handle, no swap slot: `nmp_app_chirp_register` is called
    // once at app init, so a fire-and-forget registration is sufficient.
    // Mirrors `register_inbox_projection` in `dm_runtime.rs`.
    //
    // D6 — silent skip on a poisoned observer slot. Zap counts are a
    // non-essential feed affordance; their absence must not fail the whole
    // Chirp registration. The `ModularTimelineProjection` below remains the
    // single fatal-on-failure observer (its absence breaks the timeline).
    let zaps_proj = Arc::new(nmp_nip57::ZapsAggregateProjection::new());
    let zaps_observer_id = app_ref
        .register_event_observer(Arc::clone(&zaps_proj) as Arc<dyn KernelEventObserver>);
    if zaps_observer_id.0 != 0 {
        app_ref.register_snapshot_projection("nmp.nip57.zaps", move || zaps_proj.snapshot_json());
    }

    let viewer: Pubkey = c_string_opt(viewer_pubkey).unwrap_or_default();
    let spec = ModularTimelineSpec {
        viewer,
        kinds: vec![nmp_nip01::KIND_SHORT_NOTE, nmp_nip18::KIND_REPOST],
        authors: None,
        policy: ModulePolicy::default(),
    };

    let projection = Arc::new(ModularTimelineProjection::new(&spec));
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

    // Delegate the observer + snapshot-projection wiring (and the
    // singleton-slot idempotency dance) to `nmp_nip29::register::wire_group_chat`.
    // Thin-shell rule: this FFI symbol only parses C strings and calls the
    // typed host-wiring helper that lives in the protocol crate.
    wire_group_chat(app_ref, group_id);
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

    // Delegate observer + snapshot-projection wiring to the typed host-wiring
    // helper in the protocol crate. Thin-shell rule: this FFI symbol only
    // parses the C string and calls `nmp_nip29::register::wire_group_discovery`.
    wire_group_discovery(app_ref, relay_url);
}

/// Wire the NIP-17 DM runtime into `app`.
///
/// Rust observes the active local-key slot and relay-edit rows on snapshot
/// ticks, then owns the active-account kind:1059 gift-wrap interest,
/// kind:10050 relay-list publish, and `"nmp.nip17.dm_inbox"` projection — no
/// viewer pubkey is required at the FFI boundary.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_register_dm_inbox(app: *mut NmpApp) {
    if app.is_null() {
        return;
    }
    // SAFETY: caller guarantees `app` is a valid pointer from `nmp_app_new`,
    // live for the duration of this call. The borrow is not held past return.
    let app_ref = unsafe { &*app };
    nmp_app_template::runtimes::register_dm_runtime(app_ref);
}

/// Wire a [`FollowListProjection`] for the active account into `app`.
///
/// This is **pure consumption** of the NIP-02 kind:3 contact list. It
/// constructs a [`FollowListProjection`] bound to `active_pubkey`, plugs it
/// into the kernel as a [`KernelEventObserver`] (ingest), and registers its
/// `snapshot_json` read under the snapshot key `"nmp.follow_list"` (output).
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
/// account overwrites the `"nmp.follow_list"` snapshot key with an
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
    let active_pubkey_slot = Arc::new(Mutex::new(pubkey_opt));

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
    app_ref.register_snapshot_projection("nmp.follow_list", move || projection.snapshot_json());
}
