//! `nmp-app-gallery` — composition root for the **NmpGallery** app.
//!
//! Sibling of `nmp-app-chirp` and `nmp-app-notes`, distinguished by what it
//! does NOT carry: no `ModularTimelineProjection`, no Marmot, no wallet
//! runtime. The gallery is a pure framework showcase — its single value
//! is showing that an NMP-based Nostr app can be assembled from the
//! substrate primitives alone, via the canonical
//! [`nmp_app_template::register_defaults`] one-shot.
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
//!   [`nmp_app_template::register_defaults`]. MUST be called once after
//!   `nmp_app_new` and BEFORE `nmp_app_start`.
//! * [`showcase::nmp_app_gallery_showcase_references_json`] — borrowed JSON
//!   pointer for the shared gallery references used by every host shell.
//!
//! # Snapshot delivery — push only
//!
//! `nmp-core` delivers the full kernel snapshot via the **push** callback
//! installed through [`nmp_ffi::nmp_app_set_update_callback`]: the actor
//! serializes a `KernelSnapshot` on every emit tick and hands the JSON to
//! the host. There is no kernel-side **pull** accessor — the snapshot
//! state lives on the actor thread and is not safely reachable through a
//! synchronous FFI call without breaking D8. Hosts that want bespoke
//! pull-side state register a host-side projection through
//! [`nmp_ffi::nmp_app_register_snapshot_projection`] (read via the push
//! callback as well). Kernel liveness is available through the
//! [`nmp_ffi::nmp_app_is_alive`] D7 probe.
//!
//! # D0 — no protocol nouns
//!
//! `Cargo.toml` depends on `nmp-ffi` + `nmp-app-template` + `serde_json`
//! only. No `nmp-nip*`, no `nmp-app-chirp`, no `nmp-marmot`, no
//! `nmp-nwc`. The crate name does not appear in any per-NIP Cargo file.

// JNI shim for the Android shell — `Java_org_nmp_gallery_bridge_KernelBridge_*`
// symbols that `KernelBridge.kt` binds via `System.loadLibrary`. Only compiled
// when building with the `android-ffi` feature (cargo ndk build).
#[cfg(feature = "android-ffi")]
mod android;

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

use std::ffi::c_void;

/// Install the canonical NMP composition into `app`.
///
/// Forwards to [`nmp_app_template::register_defaults`] — the gallery has
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
    nmp_app_template::register_defaults(app);
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
        // the D6 degrade). Liveness reaches the host via the push callback
        // (`nmp_app_set_update_callback`) and the `nmp_app_is_alive` probe;
        // there is no pull-side snapshot symbol to assert against.
        let app = nmp_ffi::nmp_app_new();
        assert!(!app.is_null(), "nmp_app_new must produce a non-null app");

        nmp_app_gallery_register(app as *mut c_void);
        assert!(
            nmp_ffi::nmp_app_is_alive(app as *mut nmp_ffi::NmpApp) != 0,
            "registered app must report alive via the D7 probe"
        );

        nmp_ffi::nmp_app_free(app);
    }
}
