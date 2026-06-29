//! `nmp-app-gallery` — composition root for the **NmpGallery** app.
//!
//! NMP's framework showcase composition root, distinguished by what it does
//! NOT carry: no app timeline projection, no Marmot policy, no wallet runtime.
//! The gallery composes the shared substrate and selected protocol features
//! explicitly through the named [`nmp_defaults`] installer surface, then exposes
//! only the app-shell adapters needed to render that framework state.
//!
//! # Surface
//!
//! Every `nmp_app_*` C-ABI symbol the iOS / Android shell needs is
//! re-exported from [`nmp_ffi`]. The Rust-path `pub use nmp_ffi::*` is what
//! drags each symbol's body into the CGU that ends up inside
//! `libnmp_app_gallery.{a,so}` — without it the `#[no_mangle]` symbols stay
//! `U` (undefined) in the archive and the platform link step fails.
//! Mirrors the `apps/notes/nmp-app-notes` pattern exactly.
//!
//! The crate adds two new `#[no_mangle]` symbols of its own:
//!
//! * [`nmp_app_gallery_register`] — explicit gallery composition installer.
//!   MUST be called once after `nmp_app_new` and BEFORE `nmp_app_start`.
//! * [`showcase::nmp_app_gallery_showcase_references_json`] — borrowed JSON
//!   pointer for the shared gallery references used by every host shell.
//! * [`nmp_app_gallery_snapshot_json_from_update_frame`] — owned JSON snapshot
//!   string decoded from the typed FlatBuffers update frame for the Gallery
//!   native shells.
//!
//! # Snapshot delivery — push only
//!
//! `nmp-core` delivers the full typed update frame via the **push** callback
//! installed through [`nmp_ffi::nmp_app_set_update_callback`]: the actor
//! serializes an `nmp.transport.UpdateFrame` on every emit tick and hands the
//! bytes to the host. There is no kernel-side **pull** accessor — the snapshot
//! state lives on the actor thread and is not safely reachable through a
//! synchronous FFI call without breaking D8. Hosts that want bespoke
//! pull-side state register a host-side projection through
//! [`nmp_ffi::nmp_app_register_snapshot_projection`] (read via the push
//! callback as well). Kernel liveness is available through the
//! [`nmp_ffi::nmp_app_is_alive`] D7 probe.
//!
//! # D0 — no protocol nouns
//!
//! `Cargo.toml` depends on the C-ABI/substrate crates (`nmp-ffi`,
//! `nmp-defaults`, `nmp-core`, `nmp-content`) and serialization only. No
//! `nmp-nip*`, no app-specific social feed crate, no `nmp-marmot`, no
//! `nmp-nwc`. The crate name does not appear in any per-NIP Cargo file.

// JNI shim for the Android shell — `Java_org_nmp_gallery_bridge_KernelBridge_*`
// symbols that `KernelBridge.kt` binds via `System.loadLibrary`. Only compiled
// when building with the `android-ffi` feature (cargo ndk build).
#[cfg(feature = "android-ffi")]
mod android;
#[cfg(feature = "android-ffi")]
mod android_push;
// ADR-0064 / Cut-B (#1756) — typed byte-doorway dispatch seam. Native-only:
// it names the `nmp_ffi` C-ABI `nmp_app_*` symbols, which exist only under the
// `native` feature (wasm uses wasm-bindgen, not the C ABI). The `android-ffi`
// JNI shell is the in-repo caller; the seam itself is reused by any native
// gallery shell that dispatches a write.
#[cfg(feature = "native")]
pub mod dispatch_bytes;
mod snapshot_json;

pub mod registry;
pub mod showcase;

// Re-export every C-ABI symbol the platform shells need. As with
// `apps/notes/nmp-app-notes/src/lib.rs`, the glob is what causes rustc to
// pull each `#[no_mangle]` body into the CGU that ends up inside
// `libnmp_app_gallery.{a,so}`. The same glob through `nmp_ffi` (rather
// than the pre-step-11 `nmp_core::*`) gets all the post-extraction
// `nmp_app_*` symbols.
//
// `#[allow(unused_imports)]` — the symbols are consumed by the C linker on
// the platform side, not by any Rust code in this crate; without the
// allow, `cargo check` warns about each re-exported item.
#[allow(unused_imports)]
pub use nmp_ffi::*;

use std::ffi::{c_char, c_void, CStr, CString};

/// Dispatch a gallery action through the typed byte doorway (ADR-0064 / Cut-B,
/// #1756).
///
/// The iOS shell passes `namespace` (the action's HOST namespace, e.g.
/// `nmp.publish`) and `body_json` (the canonical serde action body). This
/// function encodes the typed `ActionPayload` bytes via
/// [`dispatch_bytes::dispatch_action_bytes_for`] and dispatches them through
/// `nmp_app_dispatch_action_bytes`. No JSON crosses the FFI to the kernel.
///
/// Returns a heap-allocated JSON envelope string the caller MUST free via
/// `nmp_free_string`:
/// * `{"correlation_id":"<id>"}` — accepted and enqueued.
/// * `{"error":"<message>"}` — unknown namespace, malformed body, or kernel
///   rejection.
///
/// D6: a null `app`, null/empty `namespace`, or null `body_json` returns an
/// `{"error":"…"}` envelope, never NULL or a crash.
///
/// # Safety
/// `app` must be a valid pointer from `nmp_app_new` (or null). `namespace`
/// and `body_json` must be valid UTF-8 NUL-terminated C strings, or null.
#[cfg(feature = "native")]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_gallery_dispatch_action_bytes(
    app: *mut nmp_ffi::NmpApp,
    namespace: *const c_char,
    body_json: *const c_char,
) -> *mut c_char {
    let namespace = if namespace.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(namespace) }
            .to_string_lossy()
            .into_owned()
    };
    let body_json = if body_json.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(body_json) }
            .to_string_lossy()
            .into_owned()
    };
    let result = dispatch_bytes::dispatch_action_bytes_for(app, &namespace, &body_json);
    let envelope = match result {
        Ok(correlation_id) => format!(r#"{{"correlation_id":{}}}"#, json_escape(&correlation_id)),
        Err(error) => format!(r#"{{"error":{}}}"#, json_escape(&error)),
    };
    CString::new(envelope)
        .unwrap_or_else(|_| {
            CString::new(r#"{"error":"dispatch result encoding failed"}"#).unwrap_or_default()
        })
        .into_raw()
}

/// JSON-escape a string (adds surrounding quotes + backslash escapes).
/// Falls back to `""` on failure (D6: failures are data, never panics).
#[cfg(feature = "native")]
fn json_escape(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// Install the explicit gallery composition into `app`.
///
/// Wires the gallery's explicit substrate/protocol composition through the
/// named ADR-0069 installers. MUST be called exactly once after
/// [`nmp_ffi::nmp_app_new`] and BEFORE [`nmp_ffi::nmp_app_start`].
///
/// `app` is typed as `*mut c_void` to mirror the host-facing C signature
/// (`void nmp_app_gallery_register(void *app)`); the body casts to
/// `*mut NmpApp` after the null check.
///
/// # Doctrine
///
/// * **D6** — a null `app` is a silent no-op. A bad registration argument
///   never crashes the host.
///
/// # Safety
///
/// `app` must be a valid pointer returned by [`nmp_ffi::nmp_app_new`] (or
/// null). Calling this twice on the same `app` is a composition bug: action
/// namespaces and single-slot factories are last-writer-wins, while ingest
/// parsers and observers are additive.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_gallery_register(app: *mut c_void) {
    if app.is_null() {
        return;
    }
    // SAFETY: caller guarantees `app` is a valid pointer from
    // `nmp_app_new`. The cast is sound because `nmp_app_gallery_register`'s
    // C signature is `void(void *)` — Swift / Kotlin pass the same opaque
    // pointer they got back from `nmp_app_new`.
    let app = unsafe { &mut *(app as *mut nmp_ffi::NmpApp) };
    register_gallery_composition(app);
    // ADR-0053 / Workstream-E4 — the gallery is a full client (it showcases
    // every component, so it reads the full built-in set). Declare that intent
    // explicitly here: an undeclared start is the loud forgotten-wiring footgun,
    // not a silent firehose. Both gallery shells (tui, android) route through
    // this register helper, so one call covers them.
    app.consume_all_builtin_projections();
}

fn register_gallery_composition(app: &mut impl nmp_core::substrate::AppHost) {
    let nmp_defaults::NmpDefaults {
        coverage_gate,
        search_defaults,
        ..
    } = nmp_defaults::NmpDefaults::default();

    let _mailbox_cache = nmp_defaults::register_substrate(app, coverage_gate);
    nmp_defaults::register_nip50_protocol_defaults(app);
    let _social_handles = nmp_defaults::register_social_protocol_defaults(app, search_defaults);
    nmp_defaults::register_dm_protocol_defaults(app);
    nmp_defaults::register_longform_projection(app);
}

/// Opaque host-owned mirrors of the kernel's `refs.profile` / `refs.event`
/// row-delta projections (ADR-0063 #1671). The native shells (iOS / Android)
/// hold one of these for the lifetime of their kernel session and pass it to
/// every [`nmp_app_gallery_snapshot_json_from_update_frame`] call so per-key
/// ref deltas accumulate across frames (each sidecar carries only
/// changed/cleared rows — a single frame cannot be decoded in isolation).
///
/// D4: this is the sole app-side mirror of hydrated ref facts. Gallery JSON is
/// materialised from these stores; native never keeps a second merge cache.
pub struct GalleryRefStores {
    pub(crate) profiles: nmp_core::refs::RefProfileStore,
    pub(crate) events: nmp_core::refs::RefEventStore,
}

impl GalleryRefStores {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            profiles: nmp_core::refs::RefProfileStore::new(),
            events: nmp_core::refs::RefEventStore::new(),
        }
    }
}

/// Allocate fresh gallery ref stores. The caller owns the returned pointer and
/// MUST release it exactly once with [`nmp_app_gallery_ref_stores_free`]. Never
/// returns NULL.
#[no_mangle]
pub extern "C" fn nmp_app_gallery_ref_stores_new() -> *mut GalleryRefStores {
    Box::into_raw(Box::new(GalleryRefStores::new()))
}

/// Release [`GalleryRefStores`] allocated by
/// [`nmp_app_gallery_ref_stores_new`]. A NULL pointer is a silent no-op
/// (D6). Double-free is undefined behaviour (caller contract).
///
/// # Safety
/// `store` must be a pointer returned by
/// [`nmp_app_gallery_ref_stores_new`] and not already freed, or NULL.
#[no_mangle]
pub unsafe extern "C" fn nmp_app_gallery_ref_stores_free(store: *mut GalleryRefStores) {
    if store.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(store) });
}

/// Decode one canonical typed `nmp.transport.UpdateFrame` into the Gallery
/// snapshot JSON shape consumed by the iOS and Android model layers.
///
/// ADR-0063 (#1671): the frame's `refs.profile` / `refs.event` row-delta
/// batches are merged into `stores` (the host's persistent ref mirrors) before
/// the snapshot JSON is built; `refs.profile` is rendered from the profile
/// store and the derived `refs.event.envelopes` render map is built from the
/// event store.
/// `store` MUST persist across calls for one kernel session.
///
/// Returns a heap-allocated UTF-8 JSON string on success; callers must release
/// it with [`nmp_ffi::nmp_free_string`]. Returns NULL for NULL/empty input, a
/// NULL `store`, malformed frames, or malformed typed sidecars (D6).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_gallery_snapshot_json_from_update_frame(
    stores: *mut GalleryRefStores,
    bytes: *const u8,
    len: usize,
) -> *mut c_char {
    if stores.is_null() || bytes.is_null() || len == 0 {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees `stores` is a live pointer from
    // `nmp_app_gallery_ref_stores_new`; access is serialised by the
    // single-threaded host decode path (the update callback dispatches to the
    // main actor / thread before calling this).
    let stores = unsafe { &mut *stores };
    let frame = unsafe { std::slice::from_raw_parts(bytes, len) };
    let Ok(json) = snapshot_json::snapshot_json_from_update_frame(
        frame,
        &mut stores.profiles,
        &mut stores.events,
    ) else {
        return std::ptr::null_mut();
    };
    CString::new(json)
        .unwrap_or_else(|_| c"{}".to_owned())
        .into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_tolerates_null_app() {
        // D6 contract: every `nmp_app_*` symbol degrades silently on NULL.
        nmp_app_gallery_register(std::ptr::null_mut());
    }

    #[test]
    fn register_with_real_app_smoke() {
        // Smoke-test the composition path: build a real `NmpApp` and run
        // the explicit gallery composition via the gallery's one-shot. The
        // only test that exercises a real-app registration (the null-path test
        // above covers the D6 degrade). `nmp_app_new` is passive;
        // `nmp_app_start` spawns the actor before the D7 liveness probe can
        // report alive.
        let app = nmp_ffi::nmp_app_new();
        assert!(!app.is_null(), "nmp_app_new must produce a non-null app");

        nmp_app_gallery_register(app as *mut c_void);
        nmp_ffi::nmp_app_start(app as *mut nmp_ffi::NmpApp, 256, 4);
        assert!(
            nmp_ffi::nmp_app_is_alive(app as *mut nmp_ffi::NmpApp) != 0,
            "registered app must report alive via the D7 probe"
        );

        nmp_ffi::nmp_app_free(app);
    }
}
