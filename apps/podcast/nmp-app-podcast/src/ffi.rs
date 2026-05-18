//! Podcast per-app FFI surface.
//!
//! Six `extern "C"` symbols Swift links against:
//!
//! - [`nmp_app_podcast_register`] — instantiate a [`PodcastApp`] and return
//!   an opaque handle. The `app: *mut NmpApp` argument is reserved for
//!   future event-observer registration (the kernel-snapshot stream that
//!   Chirp uses); this iteration only needs the in-memory library, but
//!   accepting the pointer now keeps the FFI shape stable across the
//!   wiring split.
//! - [`nmp_app_podcast_snapshot`] — serialize the current
//!   `podcast_core::views::LibraryView` into a freshly-allocated nul-
//!   terminated JSON C string. Swift owns the pointer until it calls
//!   `nmp_app_podcast_snapshot_free`.
//! - [`nmp_app_podcast_snapshot_free`] — companion deallocator.
//! - [`nmp_app_podcast_subscribe`] — append a feed URL to the library.
//!   Title/author are optional (NULL → fall back to URL host /
//!   empty-string).
//! - [`nmp_app_podcast_unsubscribe`] — remove a podcast by its ULID string.
//! - [`nmp_app_podcast_unregister`] — drop the handle and free state.
//!
//! ## Doctrine
//!
//! * **D0** — `nmp-core` never depends on `podcast-core`; this crate is the
//!   composition point.
//! * **D6** — every entry point is fire-and-forget. Null pointers, missing
//!   strings, invalid UTF-8, malformed URLs, JSON encode errors, and
//!   poisoned mutexes all degrade silently rather than raising across FFI.

use std::ffi::{c_char, CStr, CString};
use std::str::FromStr;
use std::sync::Arc;

use nmp_core::NmpApp;
use ulid::Ulid;
use url::Url;

use crate::state::PodcastApp;

/// Opaque handle returned by [`nmp_app_podcast_register`]. Boxed on the
/// heap so the address is stable. Swift holds the raw pointer until it
/// calls [`nmp_app_podcast_unregister`].
pub struct PodcastHandle {
    state: Arc<PodcastApp>,
    // Reserved: the kernel app pointer the shell registered with. Future
    // iterations attach this state to the kernel as a `KernelEventObserver`
    // (the way `nmp-app-chirp` does), enabling reactive snapshots driven
    // by Nostr/RSS event ingest. The pointer is parked here so the FFI
    // contract (and the `unregister`-before-`free` ordering rule) stays
    // identical to Chirp's.
    _app: *mut NmpApp,
}

// SAFETY: `PodcastHandle` is owned by Swift; only the `_app` raw pointer
// is `!Send`/`!Sync` material. The mirror of `ChirpHandle`'s rationale: the
// iOS shell serializes its FFI calls on a single bridge dispatch queue, so
// cross-thread mutation of the raw pointer does not happen. The
// `Arc<PodcastApp>` is already safely `Send + Sync` (it wraps an internal
// `Mutex`).
unsafe impl Send for PodcastHandle {}
unsafe impl Sync for PodcastHandle {}

/// Register a Podcast projection against `app`. Returns a non-null
/// `*mut PodcastHandle` on success; `null` if `app` is null.
///
/// `app` MUST outlive the returned handle. Call
/// [`nmp_app_podcast_unregister`] before `nmp_app_free`.
#[no_mangle]
pub extern "C" fn nmp_app_podcast_register(app: *mut NmpApp) -> *mut PodcastHandle {
    if app.is_null() {
        return std::ptr::null_mut();
    }
    let handle = PodcastHandle {
        state: Arc::new(PodcastApp::new()),
        _app: app,
    };
    Box::into_raw(Box::new(handle))
}

/// Serialize the current `LibraryView` into a JSON C string. Returns null
/// on any failure (null handle, JSON encode error, CString nul-byte
/// conflict). The returned pointer is owned by the caller; pass it to
/// [`nmp_app_podcast_snapshot_free`] when done.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_podcast_snapshot(handle: *mut PodcastHandle) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees `handle` came from `nmp_app_podcast_register`
    // and has not yet been freed.
    let handle = unsafe { &*handle };
    let view = handle.state.snapshot();
    let Ok(json) = serde_json::to_string(&view) else {
        return std::ptr::null_mut();
    };
    let Ok(cstr) = CString::new(json) else {
        return std::ptr::null_mut();
    };
    cstr.into_raw()
}

/// Free a snapshot string previously returned by
/// [`nmp_app_podcast_snapshot`]. Null pointer is a silent no-op.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_podcast_snapshot_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: caller guarantees `ptr` came from `CString::into_raw` in
    // `nmp_app_podcast_snapshot` and has not been freed.
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

/// Subscribe to a feed URL. Fire-and-forget. The next call to
/// [`nmp_app_podcast_snapshot`] reflects the new state. Invalid URL,
/// invalid UTF-8 strings, and null handle all degrade to no-ops (D6).
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_podcast_subscribe(
    handle: *mut PodcastHandle,
    feed_url: *const c_char,
    title_or_null: *const c_char,
    author_or_null: *const c_char,
) {
    if handle.is_null() {
        return;
    }
    let Some(url_str) = c_string_opt(feed_url) else {
        return;
    };
    let Ok(url) = Url::parse(&url_str) else {
        return;
    };
    let title = c_string_opt(title_or_null);
    let author = c_string_opt(author_or_null);
    // SAFETY: caller guarantees `handle` is valid.
    let handle = unsafe { &*handle };
    let _ = handle.state.subscribe(url, title, author);
}

/// Unsubscribe a podcast by its ULID string. Idempotent — unknown ids are
/// a no-op. Invalid UTF-8 / malformed ULID / null handle all silently
/// degrade.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_podcast_unsubscribe(
    handle: *mut PodcastHandle,
    podcast_id: *const c_char,
) {
    if handle.is_null() {
        return;
    }
    let Some(id_str) = c_string_opt(podcast_id) else {
        return;
    };
    let Ok(id) = Ulid::from_str(&id_str) else {
        return;
    };
    // SAFETY: caller guarantees `handle` is valid.
    let handle = unsafe { &*handle };
    let _ = handle.state.unsubscribe(id);
}

/// Drop the handle and release the state. Idempotent: null pointer is a
/// silent no-op. The handle MUST NOT be used after this call.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_podcast_unregister(handle: *mut PodcastHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: caller guarantees `handle` came from `nmp_app_podcast_register`
    // and has not already been freed.
    unsafe {
        let _ = Box::from_raw(handle);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::nmp_app_free;
    use nmp_core::nmp_app_new;
    use podcast_core::views::LibraryView;

    fn cstr(s: &str) -> CString {
        CString::new(s).expect("no internal nul")
    }

    /// End-to-end FFI round trip through a real `NmpApp`. Mirrors the
    /// `register_snapshot_unregister_round_trip` test in `nmp-app-chirp`.
    /// Verifies that:
    /// - register returns a non-null handle
    /// - empty snapshot is `{"podcasts":[]}`
    /// - subscribe → snapshot reflects the new row
    /// - unregister + free are clean
    #[test]
    fn register_subscribe_snapshot_unregister_round_trip() {
        let app = nmp_app_new();
        let handle = nmp_app_podcast_register(app);
        assert!(!handle.is_null(), "register returned null");

        // Empty snapshot — no subscriptions yet.
        let snap = nmp_app_podcast_snapshot(handle);
        assert!(!snap.is_null());
        // SAFETY: snap is a valid C string from our own CString.
        let json = unsafe { CStr::from_ptr(snap) }.to_str().unwrap().to_owned();
        nmp_app_podcast_snapshot_free(snap);
        let view: LibraryView = serde_json::from_str(&json).expect("decode empty view");
        assert!(view.podcasts.is_empty());

        // Subscribe and re-snapshot.
        let feed = cstr("https://feeds.megaphone.fm/lex-fridman");
        let title = cstr("Lex Fridman Podcast");
        let author = cstr("Lex Fridman");
        nmp_app_podcast_subscribe(handle, feed.as_ptr(), title.as_ptr(), author.as_ptr());

        let snap = nmp_app_podcast_snapshot(handle);
        let json = unsafe { CStr::from_ptr(snap) }.to_str().unwrap().to_owned();
        nmp_app_podcast_snapshot_free(snap);
        let view: LibraryView = serde_json::from_str(&json).expect("decode after subscribe");
        assert_eq!(view.podcasts.len(), 1);
        assert_eq!(view.podcasts[0].title, "Lex Fridman Podcast");
        assert_eq!(view.podcasts[0].author, "Lex Fridman");

        nmp_app_podcast_unregister(handle);
        nmp_app_free(app);
    }

    #[test]
    fn null_handle_paths_are_silent_noops() {
        nmp_app_podcast_unregister(std::ptr::null_mut());
        let snap = nmp_app_podcast_snapshot(std::ptr::null_mut());
        assert!(snap.is_null());
        nmp_app_podcast_snapshot_free(std::ptr::null_mut());
        nmp_app_podcast_subscribe(
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
        );
        nmp_app_podcast_unsubscribe(std::ptr::null_mut(), std::ptr::null());
    }

    #[test]
    fn register_with_null_app_returns_null() {
        let handle = nmp_app_podcast_register(std::ptr::null_mut());
        assert!(handle.is_null());
    }

    #[test]
    fn subscribe_dedupes_via_ffi() {
        let app = nmp_app_new();
        let handle = nmp_app_podcast_register(app);
        let feed = cstr("https://feeds.example.com/show.xml");
        nmp_app_podcast_subscribe(handle, feed.as_ptr(), std::ptr::null(), std::ptr::null());
        nmp_app_podcast_subscribe(handle, feed.as_ptr(), std::ptr::null(), std::ptr::null());
        let snap = nmp_app_podcast_snapshot(handle);
        let json = unsafe { CStr::from_ptr(snap) }.to_str().unwrap().to_owned();
        nmp_app_podcast_snapshot_free(snap);
        let view: LibraryView = serde_json::from_str(&json).expect("decode");
        assert_eq!(view.podcasts.len(), 1, "second subscribe must dedupe");
        nmp_app_podcast_unregister(handle);
        nmp_app_free(app);
    }

    #[test]
    fn invalid_url_is_silent_noop() {
        let app = nmp_app_new();
        let handle = nmp_app_podcast_register(app);
        let bad = cstr("not a url");
        nmp_app_podcast_subscribe(handle, bad.as_ptr(), std::ptr::null(), std::ptr::null());
        let snap = nmp_app_podcast_snapshot(handle);
        let json = unsafe { CStr::from_ptr(snap) }.to_str().unwrap().to_owned();
        nmp_app_podcast_snapshot_free(snap);
        let view: LibraryView = serde_json::from_str(&json).expect("decode");
        assert!(view.podcasts.is_empty(), "invalid URL must not subscribe");
        nmp_app_podcast_unregister(handle);
        nmp_app_free(app);
    }
}
