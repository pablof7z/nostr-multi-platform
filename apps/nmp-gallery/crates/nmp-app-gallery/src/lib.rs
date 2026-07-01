//! `nmp-app-gallery` — composition root for the **NmpGallery** app.
//!
//! NMP's framework showcase composition root, distinguished by what it does
//! NOT carry: no app timeline projection, no Marmot policy, no wallet runtime.
//! The gallery composes the shared substrate and selected protocol features
//! explicitly through owner-crate installer surfaces, then exposes
//! only the app-shell adapters needed to render that framework state.
//!
//! # Surface
//!
//! The crate adds gallery-owned `#[no_mangle]` symbols:
//!
//! * [`nmp_app_gallery_register`] — explicit gallery composition installer for
//!   C-ABI `*mut NmpApp` pointers (from the raw `new_app()` path).
//! * [`nmp_app_gallery_register_uniffi`] — gallery composition bridge for UniFFI
//!   Arc pointers (bridge-private Android composition shim).
//! * [`showcase::nmp_app_gallery_showcase_references_json`] — borrowed JSON
//!   pointer for the shared gallery references used by every host shell.
//! * [`nmp_app_gallery_snapshot_json_from_update_frame`] — owned JSON snapshot
//!   string decoded from the typed FlatBuffers update frame for the Gallery
//!   native shells.
//!
//! All heap-allocated C strings returned by gallery C-ABI symbols MUST be freed
//! via [`nmp_app_gallery_free_string`] (owned by this crate's `free` module).
//!
//! # Snapshot delivery — push only
//!
//! `nmp-core` delivers the full typed update frame via the **push** listener
//! installed through `NmpApp::set_update_listener`. There is no kernel-side
//! **pull** accessor — the snapshot state lives on the actor thread and is not
//! safely reachable through a synchronous call without breaking D8.
//!
//! # D0 — no protocol nouns
//!
//! `Cargo.toml` depends on the native runtime crates (`nmp-native-runtime`,
//! `nmp-uniffi`, `nmp-core`, `nmp-content`, protocol crates) and serialization
//! only. No `nmp-nip*`, no app-specific social feed crate, no `nmp-marmot`,
//! no `nmp-nwc`.

// JNI shim for the Android shell — `Java_org_nmp_gallery_bridge_KernelBridge_*`
// symbols that `KernelBridge.kt` binds via `System.loadLibrary`. Only compiled
// when building with the `android-ffi` feature (cargo ndk build).
// Post M14 shell-2: the NmpApp lifecycle is owned by the UniFFI NmpApp Kotlin
// class; the `android` module only contains gallery-owned JNI symbols that have
// no UniFFI counterpart (composition registration, showcase/registry JSON,
// snapshot decode, and URI decoding).
#[cfg(feature = "android-ffi")]
mod android;
// ADR-0064 / Cut-B (#1756) — typed byte-doorway dispatch seam. Native-only:
// it names the `nmp-native-runtime` `dispatch_action_bytes_typed` symbol, which
// exists only under the `native` feature. The `android-ffi` JNI shell is the
// in-repo caller.
#[cfg(feature = "native")]
pub mod dispatch_bytes;
pub mod event_ref_uri;
// nmp_app_gallery_free_string — release C strings produced by gallery C-ABI entry points.
mod free;
pub use free::nmp_app_gallery_free_string;
#[cfg(feature = "native")]
mod native_kernel;
mod snapshot_json;

pub mod registry;
pub mod showcase;

use std::ffi::{c_char, c_void, CString};

const GALLERY_COMPOSITION_ROOT: &str = "nmp-app-gallery";
const GALLERY_COMPOSITION_PROVIDER: &str = "nmp_app_gallery::register_gallery_composition";

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
/// [`nmp_native_runtime::new_app`] and BEFORE `start_runtime`.
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
/// `app` must be a valid pointer returned by the composition root (or null).
/// Calling this twice on the same `app` is a composition bug.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_gallery_register(app: *mut c_void) {
    if app.is_null() {
        return;
    }
    // SAFETY: caller guarantees `app` is a valid pointer from `new_app`.
    // The cast is sound because `nmp_app_gallery_register`'s C signature is
    // `void(void *)` — Swift / Kotlin pass the same opaque pointer they got back.
    let app = unsafe { &mut *(app as *mut nmp_native_runtime::NmpApp) };
    if register_gallery_composition(app) {
        // ADR-0053 / Workstream-E4 — the gallery is a full client (it showcases
        // every component, so it reads the full built-in set). Declare that intent
        // explicitly here.
        app.consume_all_builtin_projections();
    }
}

/// Bridge-private Android shim — register gallery composition for a UniFFI `NmpApp`.
///
/// Accepts the raw `Arc<nmp_uniffi::NmpApp>` pointer produced by a generated
/// `uniffiClonePointer()` call and installs the same gallery composition that
/// [`nmp_app_gallery_register`] installs for a raw `new_app()` pointer.
/// This is the only retained raw UniFFI Arc bridge in Gallery, and it is
/// constrained to pre-start app-owned composition registration. Lifecycle,
/// storage, dispatch, and ref operations use typed UniFFI or app-owned native
/// kernel helpers instead.
///
/// Ownership semantics: the function calls `Arc::from_raw` internally to take
/// ownership of the cloned Arc. The caller MUST pass a pointer obtained from
/// `uniffiClonePointer()` (which bumps the ref-count) and MUST NOT use or free
/// the pointer after this call — the Arc's ref-count is decremented when this
/// function returns.
///
/// D6: a null pointer is a silent no-op.
///
/// # Safety
///
/// `arc_ptr` must be a valid `uniffiClonePointer()` result for an `NmpApp`
/// that has not yet been started. Calling this twice on the same `NmpApp` is
/// a composition bug.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_gallery_register_uniffi(arc_ptr: *mut c_void) {
    if arc_ptr.is_null() {
        return;
    }
    // SAFETY: `arc_ptr` is the raw Arc data pointer emitted by UniFFI's
    // `uniffiClonePointer()`. `Arc::from_raw` reconstructs the Arc and takes
    // ownership of the clone (will decrement the refcount on drop).
    let arc = unsafe { std::sync::Arc::from_raw(arc_ptr as *const nmp_uniffi::NmpApp) };
    arc.configure_pre_start_for_app_facade(|inner| {
        if register_gallery_composition(inner) {
            inner.consume_all_builtin_projections();
            // Issue #2523 / crate-boundaries.md §9 — the NIP-55 (Amber)
            // first-connect permission batch is an app-owned policy fact and must
            // be declared by the leaf composition root, never baked into the
            // shared `nmp-signers` crate. NIP-55 is Android-only; this call is a
            // no-op on iOS (no Android `ExternalSignerCapabilityBridge` ever calls
            // `signin_nip55` there).
            inner.set_external_signer_permissions(gallery_nip55_permissions());
        }
    });
    // `arc` drops here — decrements the UniFFI Arc ref-count back to 1.
}

/// Android JNI bridge — gallery composition via UniFFI Arc pointer.
///
/// Kotlin calls `nativeGalleryRegisterUniffi(Pointer.nativeValue(app.uniffiClonePointer()))`.
/// This JNI wrapper converts the `jlong` and delegates to the platform-agnostic
/// [`nmp_app_gallery_register_uniffi`].
///
/// D6: a zero `arc_ptr` is a silent no-op.
#[cfg(feature = "android-ffi")]
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeGalleryRegisterUniffi(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    arc_ptr: jni::sys::jlong,
) {
    nmp_app_gallery_register_uniffi(arc_ptr as *mut std::ffi::c_void);
}

fn register_gallery_composition(app: &mut nmp_native_runtime::NmpApp) -> bool {
    if !app.claim_composition_root(GALLERY_COMPOSITION_ROOT, GALLERY_COMPOSITION_PROVIDER) {
        return false;
    }
    install_gallery_composition(app);
    true
}

fn install_gallery_composition(app: &mut impl nmp_core::substrate::AppHost) {
    let _substrate = nmp_substrate::install(app, nmp_substrate::SubstrateConfig::default());

    nmp_nip50::register_search_scopes(app);
    nmp_nip50::register_input_scopes(app);

    nmp_nip02::register_follow_actions(app);
    nmp_replies::register_actions(app);
    nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip25::Nip25Descriptor, app);
    nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip18::Nip18Descriptor, app);
    nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip84::Nip84Descriptor, app);
    nmp_nip29::register_input_scopes(app);

    let _wot = nmp_wot::register_runtime(app);
    let _mute = nmp_nip51::register_mute_runtime(app);
    let _bookmarks = nmp_nip51::register_bookmark_runtime(app);
    nmp_nip51::register_bookmark_set_runtime(app);
    nmp_nip51::register_web_bookmark_runtime(app);
    let _search_relays = nmp_nip51::register_search_relay_runtime_with_fallbacks(
        app,
        nmp_nip50::SearchFallbackRelays::default(),
    );
    let _comments = nmp_nip22::register_runtime(app);

    nmp_nip17::register_actions(app);
    nmp_nip17::register_runtime(app);

    nmp_nip23::register_longform_projection(app);
}

/// The NIP-55 (Amber) first-connect permission batch this gallery composition
/// requests (issue #2523 / crate-boundaries.md §9).
///
/// This is a **gallery product decision**, not a framework default — it must
/// be derived from what [`register_gallery_composition`] actually wires, and
/// re-reviewed whenever that composition changes. Every kind below maps to a
/// write path the gallery genuinely owns:
///
/// * `0` — profile metadata (`nmp.publish` `PublishProfile`; baseline
///   `PublishModule`, registered for every `NmpApp`).
/// * `1` — short text notes / NIP-10 replies (`nmp.publish` `PublishRaw` /
///   `PublishReply`, baseline; `nmp.replies.reply` when the target is a
///   kind:1 note).
/// * `3` — contact list (`nmp-nip02` follow / unfollow / follow_many).
/// * `5` — generic deletion, used here for reaction retraction
///   (`nmp-nip25` unreact, via `nmp-nip09`'s deletion grammar).
/// * `6`, `16` — repost / generic repost (`nmp-nip18`).
/// * `7` — reaction (`nmp-nip25` react).
/// * `13` — NIP-17 seal, the event the active signer actually signs for a DM
///   send (`nmp-nip17`; the kind:1059 gift wrap itself is signed locally with
///   an ephemeral key, never by the external signer).
/// * `1111` — NIP-22 comment (`nmp.replies.reply` when the target is a
///   kind:1111 comment; `nmp-nip22` itself only registers the read
///   projection).
/// * `9802` — highlight (`nmp-nip84`).
/// * `10002` — NIP-65 relay list (`nmp_router::register_actions`, wired by
///   `nmp_substrate::install`).
/// * `10003` — bookmark list (`nmp-nip51` add/remove bookmark).
/// * `10006` — blocked-relay list (`nmp_nip51::register_block_relay_actions`,
///   wired by `nmp_substrate::install`).
/// * `10050` — DM relay list (`nmp-nip17` publish_relay_list).
/// * `30003`, `30004` — bookmark sets / curation sets (`nmp-nip51`).
/// * `39701` — NIP-B0 web bookmark (`nmp-nip51`).
///
/// Deliberately excluded: kind:10000 mute-list writes (the gallery only
/// reads the mute list — no write action is registered), kind:9734/9735 zaps
/// (no `nmp-nip57` dependency), kind:10007 search-relay writes (read-only
/// observed projection), and NIP-29 group kinds (the gallery only recognizes
/// `naddr`/`nostr:` group links for navigation; it never publishes into a
/// group).
#[must_use]
fn gallery_nip55_permissions() -> Vec<nmp_signer_iface::Nip55Permission> {
    use nmp_signer_iface::Nip55Permission;
    vec![
        Nip55Permission::sign_event(0),
        Nip55Permission::sign_event(1),
        Nip55Permission::sign_event(3),
        Nip55Permission::sign_event(5),
        Nip55Permission::sign_event(6),
        Nip55Permission::sign_event(7),
        Nip55Permission::sign_event(13),
        Nip55Permission::sign_event(16),
        Nip55Permission::sign_event(1111),
        Nip55Permission::sign_event(9802),
        Nip55Permission::sign_event(10002),
        Nip55Permission::sign_event(10003),
        Nip55Permission::sign_event(10006),
        Nip55Permission::sign_event(10050),
        Nip55Permission::sign_event(30003),
        Nip55Permission::sign_event(30004),
        Nip55Permission::sign_event(39701),
        Nip55Permission::nip44_encrypt(),
        Nip55Permission::nip44_decrypt(),
    ]
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
/// it with [`nmp_app_gallery_free_string`]. Returns NULL for NULL/empty input, a
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
    // single-threaded host decode path.
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
        // above covers the D6 degrade).
        let app = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
        assert!(!app.is_null(), "new_app must produce a non-null app");

        nmp_app_gallery_register(app as *mut c_void);
        unsafe { &*app }.start_runtime(256, 4);
        assert!(
            unsafe { &*app }.is_alive(),
            "registered app must report alive"
        );

        unsafe { drop(Box::from_raw(app)) };
    }

    #[test]
    fn register_gallery_composition_is_one_shot() {
        let mut app = nmp_native_runtime::new_app();

        assert!(register_gallery_composition(&mut app));
        let first_report = app.debug_info_json(nmp_native_runtime::DOMAIN_COMPOSITION);
        let first_count = first_report["count"]
            .as_u64()
            .expect("composition count must be numeric");

        assert!(
            !register_gallery_composition(&mut app),
            "second Gallery composition claim must yield instead of reinstalling"
        );
        let second_report = app.debug_info_json(nmp_native_runtime::DOMAIN_COMPOSITION);
        let second_count = second_report["count"]
            .as_u64()
            .expect("composition count must be numeric");
        assert_eq!(
            second_count,
            first_count + 1,
            "duplicate composition should record only the yielded root claim"
        );

        let records = second_report["records"]
            .as_array()
            .expect("composition records must be an array");
        let root_records: Vec<_> = records
            .iter()
            .filter(|record| {
                record["seam"] == "composition_root" && record["key"] == GALLERY_COMPOSITION_ROOT
            })
            .collect();
        assert_eq!(root_records.len(), 2);
        assert_eq!(root_records[0]["disposition"], "Installed");
        assert_eq!(root_records[1]["disposition"], "YieldedToExisting");
        assert_eq!(root_records[1]["replaced"], GALLERY_COMPOSITION_PROVIDER);
    }
}

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
