//! `nmp-app-gallery` — composition root for the **NmpGallery** app.
//!
//! Sibling of `nmp-app-chirp` and `nmp-app-notes`, distinguished by what it
//! does NOT carry: no `ModularTimelineProjection`, no Marmot, no wallet
//! runtime. The gallery is a pure framework demonstrator — its single value
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
//! The crate adds three new `#[no_mangle]` symbols of its own:
//!
//! * [`nmp_app_gallery_register`] — one-shot installer. Forwards to
//!   [`nmp_app_template::register_defaults`]. MUST be called once after
//!   `nmp_app_new` and BEFORE `nmp_app_start`.
//! * [`nmp_app_gallery_snapshot`] — pull-side status accessor. Returns a
//!   small JSON envelope with the kernel-alive bit and an empty
//!   `projections` map (see the snapshot semantics note below).
//! * [`nmp_app_gallery_snapshot_free`] — companion deallocator for the
//!   string returned by [`nmp_app_gallery_snapshot`]. Mirrors
//!   `nmp_app_chirp_snapshot_free` and `nmp_broker_free_string`.
//!
//! # Snapshot semantics — read this before extending the schema
//!
//! `nmp-core` delivers the full kernel snapshot via the **push** callback
//! installed through [`nmp_ffi::nmp_app_set_update_callback`]: the actor
//! serializes a `KernelSnapshot` on every emit tick and hands the JSON to
//! the host. There is no kernel-side **pull** accessor — the snapshot
//! state lives on the actor thread and is not safely reachable through a
//! synchronous FFI call without breaking D8.
//!
//! Chirp's `nmp_app_chirp_snapshot` is NOT a kernel pull — it serializes
//! the projection-local `ModularTimelineSnapshot` that `nmp-nip01`'s
//! projection maintains under its own mutex. The gallery has no per-app
//! projection, so there is no equivalent pull target.
//!
//! `nmp_app_gallery_snapshot` therefore returns a deliberately small
//! envelope: the kernel-alive bit (pulled through [`nmp_ffi::nmp_app_is_alive`]),
//! a schema version, and an empty `projections` object. Hosts that need
//! full kernel state continue to consume the push callback; hosts that
//! want bespoke pull-side state register a host-side projection through
//! [`nmp_ffi::nmp_app_register_snapshot_projection`] (read via the push
//! callback as well).
//!
//! This is a documentation choice, not a hack: the alternative — caching
//! the last pushed update in a process-static map keyed on the `NmpApp`
//! pointer — would compete with the host's own update callback and break
//! the single-writer contract on `update_callback`. Better to be explicit
//! about the substrate's push/pull asymmetry.
//!
//! # D0 — no protocol nouns
//!
//! `Cargo.toml` depends on `nmp-ffi` + `nmp-app-template` + `serde_json`
//! only. No `nmp-nip*`, no `nmp-app-chirp`, no `nmp-marmot`, no
//! `nmp-nwc`. The crate name does not appear in any per-NIP Cargo file.

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

/// Pull-side status snapshot for the gallery app.
///
/// Returns a NUL-terminated JSON C string with the shape:
///
/// ```json
/// {
///   "schema": "nmp.gallery.snapshot/1",
///   "alive":  true,
///   "projections": {}
/// }
/// ```
///
/// The returned pointer is heap-owned by the caller; pass it to
/// [`nmp_app_gallery_snapshot_free`] when done. Null on any failure (null
/// `app`, allocation failure, JSON encode error). See the crate-level doc
/// for the rationale on why this is intentionally small — kernel state
/// reaches the host via the **push** callback
/// ([`nmp_ffi::nmp_app_set_update_callback`]), not through this pull
/// accessor.
///
/// # Doctrine
///
/// * **D6** — a null `app` is a silent no-op (returns null). An encode
///   failure is also a silent null return (never a panic across the C
///   ABI).
/// * **D8** — the body performs only an [`nmp_ffi::nmp_app_is_alive`]
///   atomic read and a `serde_json::to_string` call; no I/O, no mutex
///   waits, no actor round-trip.
///
/// # Safety
///
/// `app` must be a valid pointer returned by [`nmp_ffi::nmp_app_new`] (or
/// null).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_gallery_snapshot(app: *mut c_void) -> *mut c_char {
    if app.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: the cast mirrors `nmp_app_gallery_register`. `is_alive`
    // takes `*mut NmpApp` and degrades silently on null (D6); we pass the
    // already-validated non-null pointer through.
    let alive = nmp_ffi::nmp_app_is_alive(app as *mut nmp_ffi::NmpApp) != 0;
    let payload = serde_json::json!({
        "schema": "nmp.gallery.snapshot/1",
        "alive": alive,
        // Intentionally empty in v1 — host-side projections registered via
        // `nmp_app_register_snapshot_projection` are pulled from the push
        // callback. Future extensions (e.g. a snapshot of the active
        // account pubkey, or the relay-edit row count) belong here.
        "projections": {},
    });
    let Ok(json) = serde_json::to_string(&payload) else {
        return std::ptr::null_mut();
    };
    let Ok(cstr) = CString::new(json) else {
        return std::ptr::null_mut();
    };
    cstr.into_raw()
}

/// Free a snapshot string previously returned by
/// [`nmp_app_gallery_snapshot`]. Idempotent: a null pointer is a silent
/// no-op.
///
/// # Safety
///
/// `ptr` must have come from [`nmp_app_gallery_snapshot`] (specifically,
/// from `CString::into_raw`) and must not have been freed already.
/// Passing any other pointer is undefined behaviour.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_gallery_snapshot_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: caller guarantees `ptr` came from `CString::into_raw` in
    // `nmp_app_gallery_snapshot` and has not been freed.
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn register_tolerates_null_app() {
        // D6 contract: every `nmp_app_*` symbol degrades silently on NULL.
        nmp_app_gallery_register(std::ptr::null_mut());
    }

    #[test]
    fn snapshot_null_app_returns_null() {
        // D6 contract — null input ⇒ null output, never a panic.
        let ptr = nmp_app_gallery_snapshot(std::ptr::null_mut());
        assert!(ptr.is_null(), "null app must produce null snapshot");
    }

    #[test]
    fn snapshot_free_null_is_noop() {
        // Idempotent free contract — mirrors `nmp_app_chirp_snapshot_free`.
        nmp_app_gallery_snapshot_free(std::ptr::null_mut());
    }

    #[test]
    fn register_with_real_app_then_snapshot_roundtrip() {
        // Smoke-test the whole composition path: build a real `NmpApp`,
        // run `register_defaults` via the gallery's one-shot, then
        // confirm the snapshot pull returns a well-formed JSON envelope.
        // Uses `test-support` to reach `nmp_app_new` / `nmp_app_free`
        // through normal Rust paths.
        let app = nmp_ffi::nmp_app_new();
        assert!(!app.is_null(), "nmp_app_new must produce a non-null app");

        nmp_app_gallery_register(app as *mut c_void);

        let snap = nmp_app_gallery_snapshot(app as *mut c_void);
        assert!(!snap.is_null(), "snapshot must succeed on a live app");

        // SAFETY: `snap` came from `CString::into_raw` in
        // `nmp_app_gallery_snapshot`; valid until we free it below.
        let json = unsafe { CStr::from_ptr(snap) }
            .to_string_lossy()
            .into_owned();
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("snapshot must be valid JSON");
        assert_eq!(
            parsed.get("schema").and_then(|v| v.as_str()),
            Some("nmp.gallery.snapshot/1"),
            "snapshot must carry the gallery schema tag"
        );
        assert!(
            parsed.get("alive").and_then(|v| v.as_bool()).is_some(),
            "snapshot must carry the alive bit"
        );
        assert!(
            parsed.get("projections").and_then(|v| v.as_object()).is_some(),
            "snapshot must carry an (empty) projections object"
        );

        nmp_app_gallery_snapshot_free(snap);
        nmp_ffi::nmp_app_free(app);
    }
}
