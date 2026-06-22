//! `nmp-app-gallery` — composition root for the **NmpGallery** app.
//!
//! Sibling of `nmp-app-chirp` and `nmp-app-notes`, distinguished by what it
//! does NOT carry: no `ModularTimelineProjection`, no Marmot, no wallet
//! runtime. The gallery is a pure framework showcase: it is assembled from the
//! canonical [`nmp_defaults::register_defaults`] one-shot and exposes only the
//! app-shell adapters needed to render that framework state.
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
//! * [`nmp_app_gallery_register`] — one-shot installer. Forwards to
//!   [`nmp_defaults::register_defaults`]. MUST be called once after
//!   `nmp_app_new` and BEFORE `nmp_app_start`.
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
//! `Cargo.toml` depends on `nmp-ffi` + `nmp-defaults` + `serde_json`
//! only. No `nmp-nip*`, no `nmp-app-chirp`, no `nmp-marmot`, no
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

use std::ffi::{c_char, c_void, CString};

/// Install the canonical NMP composition into `app`.
///
/// Forwards to [`nmp_defaults::register_defaults`] — the gallery has
/// no per-app projections, so the entire registration is "what every
/// generic Nostr app needs". MUST be called exactly once after
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
/// null). Calling this twice on the same `app` is idempotent only to the
/// extent `register_defaults` itself is idempotent — see that function's
/// doc for the per-seam behaviour (action registry rejects duplicate
/// namespaces; ingest parsers are additive; routing-substrate and
/// publish-resolver factories are last-writer-wins).
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
    nmp_defaults::register_defaults(app);
    // ADR-0053 / Workstream-E4 — the gallery is a full client (it showcases
    // every component, so it reads the full built-in set). Declare that intent
    // explicitly here: an undeclared start is the loud forgotten-wiring footgun,
    // not a silent firehose. Both gallery shells (tui, android) route through
    // this register helper, so one call covers them.
    app.consume_all_builtin_projections();
}

/// Opaque host-owned mirror of the kernel's `refs.profile` row-delta projection
/// (ADR-0063 #1671). The native shells (iOS / Android) hold one of these for the
/// lifetime of their kernel session and pass it to every
/// [`nmp_app_gallery_snapshot_json_from_update_frame`] call so per-key profile
/// deltas accumulate across frames (the `refs.profile` sidecar carries only
/// changed/cleared rows — a single frame cannot be decoded in isolation). This
/// is the sole app-side store of hydrated profiles (D4); there is no second
/// native profile cache.
pub struct GalleryRefProfileStore {
    inner: nmp_core::refs::RefProfileStore,
}

/// Allocate a fresh [`GalleryRefProfileStore`]. The caller owns the returned
/// pointer and MUST release it exactly once with
/// [`nmp_app_gallery_ref_profile_store_free`]. Never returns NULL.
#[no_mangle]
pub extern "C" fn nmp_app_gallery_ref_profile_store_new() -> *mut GalleryRefProfileStore {
    Box::into_raw(Box::new(GalleryRefProfileStore {
        inner: nmp_core::refs::RefProfileStore::new(),
    }))
}

/// Release a [`GalleryRefProfileStore`] allocated by
/// [`nmp_app_gallery_ref_profile_store_new`]. A NULL pointer is a silent no-op
/// (D6). Double-free is undefined behaviour (caller contract).
///
/// # Safety
/// `store` must be a pointer returned by
/// [`nmp_app_gallery_ref_profile_store_new`] and not already freed, or NULL.
#[no_mangle]
pub unsafe extern "C" fn nmp_app_gallery_ref_profile_store_free(
    store: *mut GalleryRefProfileStore,
) {
    if store.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(store) });
}

/// Decode one canonical typed `nmp.transport.UpdateFrame` into the Gallery
/// snapshot JSON shape consumed by the iOS and Android model layers.
///
/// ADR-0063 (#1671): the frame's `refs.profile` row-delta batch is merged into
/// `store` (the host's persistent profile mirror) before the snapshot JSON is
/// built; the rendered `refs.profile` JSON map is sourced from that store.
/// `store` MUST persist across calls for one kernel session.
///
/// Returns a heap-allocated UTF-8 JSON string on success; callers must release
/// it with [`nmp_ffi::nmp_free_string`]. Returns NULL for NULL/empty input, a
/// NULL `store`, malformed frames, or malformed typed sidecars (D6).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_gallery_snapshot_json_from_update_frame(
    store: *mut GalleryRefProfileStore,
    bytes: *const u8,
    len: usize,
) -> *mut c_char {
    if store.is_null() || bytes.is_null() || len == 0 {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees `store` is a live pointer from
    // `nmp_app_gallery_ref_profile_store_new`; access is serialised by the
    // single-threaded host decode path (the update callback dispatches to the
    // main actor / thread before calling this).
    let store = unsafe { &mut *store };
    let frame = unsafe { std::slice::from_raw_parts(bytes, len) };
    let Ok(json) = snapshot_json::snapshot_json_from_update_frame(frame, &mut store.inner) else {
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
        // `register_defaults` via the gallery's one-shot. The only test that
        // exercises a real-app registration (the null-path test above covers
        // the D6 degrade). `nmp_app_new` is passive; `nmp_app_start` spawns
        // the actor before the D7 liveness probe can report alive.
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
