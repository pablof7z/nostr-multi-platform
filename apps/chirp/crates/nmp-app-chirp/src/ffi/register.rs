//! The `pub extern "C"` registration entry points Swift links against to wire
//! Chirp projections (timeline, group chat, group discovery, DM inbox, follow
//! list) and action namespaces into an [`NmpApp`].

use std::ffi::c_char;

use nmp_core::__ffi_internal::is_hex_pubkey;
use nmp_core::substrate::RoutingFactoryRegistrar;
use nmp_ffi::NmpApp;
use nmp_nip01::meta_timeline::Pubkey;

use nmp_nip02::register_follow_state_runtime;

use super::actions::{register_chirp_zap_identifier_action, register_nip29_actions};
use super::handle::ChirpHandle;
use super::helpers::c_string_opt;

/// Status code returned by [`nmp_app_chirp_register`].
///
/// Laid out as `#[repr(u32)]` so it maps to a plain `uint32_t` in C / Swift
/// without any platform-specific enum sizing surprises.
///
/// **Discriminant stability contract** — numeric values are part of the C ABI
/// and must never be renumbered.  Add new variants at the end only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum NmpRegisterStatus {
    /// Registration succeeded. `handle_out` is non-null and ready to use.
    Ok = 0,
    /// The `app` pointer was null. `handle_out` is left as null.
    NullApp = 1,
    /// `viewer_pubkey` was non-null but did not parse as a 64-char
    /// case-insensitive hex pubkey (32 bytes).  `handle_out` is left as null.
    ///
    /// A null `viewer_pubkey` is always accepted ("no viewer set"); only a
    /// *non-null malformed* value triggers this status.
    InvalidViewerPubkey = 2,
}

/// Register a Chirp modular timeline projection against `app`.
///
/// Returns an [`NmpRegisterStatus`] discriminant as `u32`.  On
/// [`NmpRegisterStatus::Ok`] the opaque handle is written through
/// `handle_out`; on any failure `*handle_out` is left unchanged (the caller
/// should initialise it to `NULL` before calling).
///
/// ## `viewer_pubkey`
///
/// * `NULL` — permitted and treated as "no viewer set"; registration
///   proceeds with an empty viewer identity.
/// * Non-null — **must** be a 64-character case-insensitive hexadecimal
///   string representing a 32-byte Nostr public key.  Any other value causes
///   the function to return [`NmpRegisterStatus::InvalidViewerPubkey`] and
///   leaves `*handle_out` as null (D6 — explicit error, no silent fallback).
///
/// ## SAFETY
///
/// * `app` must be a valid non-null `*mut NmpApp` from `nmp_app_new()`.
/// * `handle_out` must be a valid non-null `*mut *mut ChirpHandle`; passing
///   null is a programmer-error contract violation and returns
///   [`NmpRegisterStatus::NullApp`] without writing through the pointer or
///   leaking the handle allocation.
/// * `app` MUST outlive the returned handle. Call
///   [`nmp_app_chirp_unregister`] before `nmp_app_free`.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_register(
    app: *mut NmpApp,
    viewer_pubkey: *const c_char,
    handle_out: *mut *mut ChirpHandle,
) -> u32 {
    if app.is_null() {
        return NmpRegisterStatus::NullApp as u32;
    }

    // V-73 (D6): validate a non-null viewer_pubkey at the FFI boundary.
    // A null viewer_pubkey is explicitly permitted ("no viewer"); a non-null
    // value that does not parse as a 64-char lowercase hex pubkey is an
    // explicit caller error and must NOT silently fall back to the empty
    // identity.
    let viewer: Pubkey = match c_string_opt(viewer_pubkey) {
        None => String::new(),                    // null pointer → no viewer
        Some(s) if s.is_empty() => String::new(), // empty string → no viewer
        Some(s) => {
            if !is_hex_pubkey(&s) {
                return NmpRegisterStatus::InvalidViewerPubkey as u32;
            }
            s
        }
    };

    // Inherit canonical NMP composition once and keep runtime handles the
    // Chirp feed needs for app-level wiring. Shared NMP defaults intentionally
    // carry no operator relay URLs; Chirp threads its app-owned search relay
    // policy through this composition seam.
    let default_handles = nmp_defaults::register_defaults_with_handles(
        unsafe { &mut *app },
        nmp_defaults::NmpDefaults {
            search_defaults: nmp_defaults::SearchDefaults::with_default_relays(
                nmp_chirp_config::chirp_default_search_relays(),
            ),
            client_identity: Some(nmp_nip89::ClientIdentity {
                name: "Chirp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                handler: None,
            }),
            attach_client_tag: true,
            ..Default::default()
        },
    );

    // #1493 P9 — Chirp's `nostrconnect://` NIP-46 perm policy (leaf-app product
    // policy; NMP owns no default), set at this single pre-start chokepoint.
    let perms = nmp_chirp_config::chirp_nostrconnect_perms().to_string();
    RoutingFactoryRegistrar::set_nostrconnect_perms(unsafe { &*app }, perms);

    // Chirp-specific: register the NIP-29 group-chat `ActionModule`s
    // against the kernel. Lives in this crate (not the template) because
    // NIP-29 is not part of the canonical NMP composition every Nostr
    // app inherits — a notes-only app would not register it.
    //
    // SAFETY: same exclusive-borrow rationale as the
    // `register_defaults` call above — no other reference aliases `app`
    // at this point.
    register_nip29_actions(unsafe { &mut *app });
    register_chirp_zap_identifier_action(unsafe { &mut *app });

    // Visible timeline rows claim their relation streams through the same
    // dispatch_action door as all other app verbs. The action module lives in
    // nmp-relations (the cross-protocol social-relation crate) because its
    // subscription shape spans reactions/reposts/zaps and is reusable by any
    // note app.
    //
    // SAFETY: same exclusive-borrow rationale as `register_nip29_actions`.
    nmp_relations::register_visible_note_relation_actions(unsafe { &mut *app });

    // V-38: register the NIP-47 wallet stack (action modules + runtime
    // installation + status projection) when the `wallet` feature is on.
    // The crate `nmp-nip47` owns the runtime, the three connect / disconnect
    // / pay_invoice action modules, and the `"wallet"` projection wiring;
    // Chirp drives the registration here so a single call covers the
    // host-side glue. Other action modules (NIP-02 / NIP-17 / NIP-57 /
    // NIP-65) are wired transitively by `nmp_defaults::register_defaults`
    // above.
    //
    // SAFETY: same exclusive-borrow rationale as `register_chirp_actions`.
    #[cfg(feature = "wallet")]
    crate::wallet_runtime::register_nip47_wallet(unsafe { &mut *app });

    // SAFETY: caller guarantees `app` is a valid pointer allocated by
    // `nmp_app_new` for the duration of this call. We do not hold the
    // borrow past this function.
    let app_ref = unsafe { &*app };

    // #626: wire the NIP-29 group-create defaults projection so Chirp's
    // app-owned suggested public-group relay URL surfaces under
    // `"nmp.nip29.group_defaults"` as the typed `NGDF` projection instead of
    // being a hardcoded Swift `@State` literal in `NewGroupSheet`. Output-only:
    // the projection observes no kernel events, so this is a one-time
    // registration at app init. NIP-29
    // group-create is a Chirp verb, not part of the canonical NMP composition,
    // so it lives here and its operator relay policy comes from
    // `nmp-chirp-config`, not from `nmp-nip29`.
    nmp_nip29::register::wire_group_defaults_with_relay(
        app_ref,
        nmp_chirp_config::chirp_public_group_relay_url(),
    );

    // V-80 rung 7 — the product-visible cut-over. The home feed
    // (`"nmp.feed.home"`) is now produced by the OP-centric engine instead of
    // the modular timeline projection: a stream of thread ROOTS, each carrying
    // the raw attributions of follows who replied in its thread. Replies no
    // longer surface as their own feed rows; a followed user's reply to a
    // non-followed author's note surfaces THAT note tagged "↳ <follow> replied
    // in thread".
    //
    // `register_op_feed_defaults` is the one-line composition affordance from
    // `nmp-defaults`: it constructs the `ActiveFollowSet` over the app's
    // authoritative active-account slot, wires the follow predicate + event
    // lookup + actor claim sink + card builder into the `nmp-nip01` OP-feed
    // engine, and opens declared observed projections for ingest plus a
    // `FeedController` under `"nmp.feed.home"` (output). It also registers the
    // `ActiveFollowSet` through declared observed projection delivery for kind:3
    // ingest and on `NmpApp`'s identity-change observer so sign-in, switch,
    // logout, and reset proactively clear stale OP-feed state.
    let defaults = match default_handles.mute {
        Some(mute) => {
            nmp_native_runtime::register_op_feed_defaults_with_mute(app_ref, viewer, vec![1], mute)
        }
        None => nmp_native_runtime::register_op_feed_defaults(app_ref, viewer, vec![1]),
    };

    // ADR-0037 typed sidecar for nmp.feed.home IS wired:
    // `register_op_feed_defaults` step 5b (nmp-defaults
    // op_feed_defaults.rs) registers the NOFS typed-FB encoder alongside the
    // JSON projection, and iOS `TypedHomeFeedDecoder` consumes it typed-first
    // (JSON remains the ADR-0037 Commitment-4 fallback).
    // D6 — guard the write-through before allocating the handle. A null
    // `handle_out` is a programmer-error contract violation: returning an
    // error code here (instead of a segfault) is the safe, D6-compliant
    // behaviour. We reuse `NullApp` (1) — it covers all null-pointer
    // caller-contract violations; adding a new discriminant for this case
    // would widen the stable ABI surface without adding information.
    if handle_out.is_null() {
        return NmpRegisterStatus::NullApp as u32;
    }
    let handle = Box::into_raw(Box::new(ChirpHandle {
        engine: defaults.engine,
        app,
    }));
    // SAFETY: `handle_out` was verified non-null above; the pointer must be
    // a valid `*mut *mut ChirpHandle` per the function's SAFETY contract.
    unsafe { *handle_out = handle };
    NmpRegisterStatus::Ok as u32
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
    nmp_defaults::runtimes::register_dm_runtime(app_ref);
}

/// Wire the NIP-02 follow-list runtime into `app` (Chirp FFI entry point).
///
/// Delegates to [`nmp_nip02::register_follow_state_runtime`], which registers
/// the `"nmp.follow_list"` typed FlatBuffers snapshot projection backed by the
/// canonical [`nmp_core::substrate::ContactsLookup`] and wires a kind:3 demand
/// interest so the kernel's cache-serve pipeline populates the lookup before the
/// first snapshot tick.
///
/// ## Root-cause fix (#1630)
///
/// The prior implementation registered a local observer that kept a
/// local `HashMap` of follows. This local copy missed the startup cache-serve
/// that runs before the lazily-registered observer exists — so already-followed
/// accounts showed "Follow" on cold start. The new path reads directly from the
/// shared `ContactsLookup` (same source `Kind3Parser` writes into), so the
/// snapshot always reflects the canonical stored state. See `projection.rs` and
/// ADR-0057 for the single-source-of-truth contract.
///
/// ## `active_pubkey` parameter
///
/// Retained for ABI compatibility but **no longer used**. The projection reads
/// the kernel's authoritative `active_pubkey()` slot (populated for every
/// backend including bunker accounts) rather than a caller-supplied copy.
/// Swift callers may continue to pass the pubkey or NULL; both are ignored.
///
/// ## Wire shape preserved
///
/// The `"nmp.follow_list"` snapshot key and the `"nmp.nip02.follow_list"`
/// schema id are unchanged — no Swift decoder changes required.
///
/// D6 — a null `app` degrades to a silent return.
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

    // `active_pubkey` is retained in the signature for ABI stability but is no
    // longer used. The canonical slot comes from the kernel.
    let _ = active_pubkey;

    // Obtain the shared ContactsLookup — the same Arc that Kind3Parser writes
    // into via the ingest pipeline. Passed explicitly so register_follow_state_runtime
    // stays generic (it only depends on nmp-core traits, not nmp-ffi).
    let contacts_lookup = app_ref.contacts_lookup();

    register_follow_state_runtime(app_ref, contacts_lookup);
}
