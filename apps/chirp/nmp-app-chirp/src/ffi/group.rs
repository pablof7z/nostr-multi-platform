//! NIP-29 group-chat and group-discovery FFI registration entry points.
//!
//! Extracted from `ffi/register.rs` to keep each file under the 500-LOC cap
//! (AGENTS.md). Exported from `ffi/mod.rs` alongside the rest of the
//! `pub extern "C"` surface.

use std::ffi::c_char;

use nmp_ffi::NmpApp;
use nmp_nip29::group_id::GroupId;
use nmp_nip29::register::{close_group_discovery, open_group_discovery, wire_group_chat};
use nmp_nip29::register::GroupDiscoveryHandle;

use super::helpers::c_string_opt;

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

/// Open a NIP-29 group-discovery session for one host relay.
///
/// This is the **read side** of the NIP-29 group-discovery flow. It
/// constructs a [`DiscoveredGroupsProjection`] scoped to the supplied relay
/// URL, plugs it in as a [`KernelEventObserver`] (ingest), and registers its
/// snapshot read under `"nmp.nip29.discovered_groups"` (output).
/// Kind:39000/39001/39002 events for that relay then surface on every
/// snapshot tick under that key.
///
/// The companion publish side is the `nmp.nip29.discover` action — its
/// executor pushes a relay-pinned `LogicalInterest` so the kernel opens a
/// REQ and metadata events actually arrive. This FFI symbol registers only
/// the *read* side; both halves are needed for events to surface (the read
/// projection is inert without the dispatch).
///
/// Returns a heap-allocated opaque handle the caller MUST free via
/// `nmp_app_chirp_close_group_discovery`. A null `app`, null/non-UTF-8
/// `host_relay_url`, or poisoned observer slot returns NULL (D6).
///
/// `app` MUST outlive the handle. Call `nmp_app_chirp_close_group_discovery`
/// before `nmp_app_free`.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_open_group_discovery(
    app: *mut NmpApp,
    host_relay_url: *const c_char,
) -> *mut GroupDiscoveryHandle {
    if app.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees `app` is a valid pointer from `nmp_app_new`,
    // live for the duration of this call and the returned handle.
    let app_ref = unsafe { &*app };

    let Some(relay_url) = c_string_opt(host_relay_url).filter(|s| !s.is_empty()) else {
        return std::ptr::null_mut();
    };

    // Thin-shell rule: parse C string, delegate to typed protocol helper.
    match open_group_discovery(app_ref, relay_url) {
        Some(handle) => Box::into_raw(Box::new(handle)),
        None => std::ptr::null_mut(),
    }
}

/// Close a NIP-29 group-discovery session opened by
/// `nmp_app_chirp_open_group_discovery`.
///
/// Unregisters the event observer and removes the
/// `"nmp.nip29.discovered_groups"` typed snapshot projection so no stale
/// group catalog is emitted after the discover screen is dismissed. The
/// handle memory is reclaimed; the pointer MUST NOT be used after this call.
///
/// D6 — a null `handle` is a silent no-op.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_close_group_discovery(handle: *mut GroupDiscoveryHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: `handle` is a valid pointer returned by
    // `nmp_app_chirp_open_group_discovery` and must not be used after this
    // call. `Box::from_raw` takes ownership; `close_group_discovery` tears
    // down the observer + projection before the box is dropped.
    let handle = unsafe { *Box::from_raw(handle) };
    close_group_discovery(handle);
}
