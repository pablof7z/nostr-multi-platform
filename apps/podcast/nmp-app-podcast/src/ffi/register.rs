//! The `pub extern "C"` registration entry point Swift links against to wire
//! the canonical NMP defaults and obtain a [`PodcastHandle`].

use std::ffi::c_char;

use nmp_ffi::NmpApp;

use super::handle::PodcastHandle;

/// Register the canonical NMP composition against `app` and return an opaque
/// handle. Returns a non-null `*mut PodcastHandle` on success; `null` on a
/// null `app` pointer.
///
/// This call wires NIP-02 / NIP-17 / NIP-57 / NIP-65 action modules, the
/// kind:10050 ingest parser, the production routing substrate
/// (`GenericOutboxRouter` + `InMemoryMailboxCache`), the D2 coverage hook,
/// and the DM-inbox + zap-receipts runtime controllers via
/// `nmp_app_template::register_defaults`.
///
/// Podcast-specific registrations are added in later milestones. At M0.A the
/// snapshot returns a stub JSON payload
/// (`{"running":true,"rev":0,"schema_version":1}`).
///
/// `viewer_pubkey` is accepted for API symmetry with Chirp but is currently
/// unused. NULL is permitted.
///
/// `app` MUST outlive the returned handle. Call [`nmp_app_podcast_unregister`]
/// before `nmp_app_free`.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_podcast_register(
    app: *mut NmpApp,
    _viewer_pubkey: *const c_char,
) -> *mut PodcastHandle {
    if app.is_null() {
        return std::ptr::null_mut();
    }

    // Inherit the canonical NMP composition through one call — NIP-02 /
    // NIP-17 / NIP-57 / NIP-65 action modules, the kind:10050 ingest
    // parser, the production routing substrate
    // (`GenericOutboxRouter` + `InMemoryMailboxCache`), the D2 coverage
    // hook, and the DM-inbox + zap-receipts runtime controllers.
    //
    // SAFETY: caller guarantees `app` is a valid pointer from `nmp_app_new`.
    // No other reference aliases it here.
    nmp_app_template::register_defaults(unsafe { &mut *app });

    Box::into_raw(Box::new(PodcastHandle { app }))
}
