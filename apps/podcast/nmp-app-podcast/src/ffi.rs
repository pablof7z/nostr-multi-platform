//! Podcast per-app FFI surface.
//!
//! Eight `extern "C"` symbols:
//!
//! - [`nmp_app_podcast_register`] — instantiate a [`PodcastApp`] and return
//!   an opaque handle.
//! - [`nmp_app_podcast_snapshot`] — serialize the current
//!   `podcast_core::views::LibraryView` as a nul-terminated JSON C string.
//! - [`nmp_app_podcast_snapshot_free`] — companion deallocator (also frees
//!   strings returned by `episodes` and `ingest_bytes`).
//! - [`nmp_app_podcast_subscribe`] — append a feed URL to the library.
//! - [`nmp_app_podcast_unsubscribe`] — remove a podcast by its ULID string.
//! - [`nmp_app_podcast_ingest_bytes`] — inject raw RSS/Atom feed bytes for
//!   a subscribed podcast URL; populates the episode list. Returns a JSON
//!   status the caller can surface as a toast on error.
//! - [`nmp_app_podcast_episodes`] — serialize the episode list for one
//!   podcast as a `podcast_core::views::FeedView` JSON string.
//! - [`nmp_app_podcast_unregister`] — drop the handle and free state.
//!
//! ## HTTP-fetch gap (T-podcast-gap-3)
//!
//! Feed fetching is NOT performed here — that requires a host HTTP capability
//! (Android OkHttp / iOS URLSession). The host fetches the bytes and passes
//! them through `nmp_app_podcast_ingest_bytes`. Until the capability lands,
//! episode lists are empty but correct (D6).
//!
//! ## Doctrine
//!
//! * **D0** — `nmp-core` never depends on `podcast-core`; this crate is the
//!   composition point.
//! * **D6** — every entry point degrades silently on null pointers, lock
//!   poisoning, serialization failure, or malformed input.

use std::ffi::{c_char, CStr, CString};
use std::str::FromStr;
use std::sync::Arc;

use nmp_core::NmpApp;
use ulid::Ulid;
use url::Url;

use crate::state::{IngestResult, PodcastApp};

/// Opaque handle returned by [`nmp_app_podcast_register`]. Boxed on the
/// heap so the address is stable. The host holds the raw pointer until it
/// calls [`nmp_app_podcast_unregister`].
pub struct PodcastHandle {
    state: Arc<PodcastApp>,
    _app: *mut NmpApp,
}

// SAFETY: see ffi.rs of nmp-app-chirp for the rationale.
unsafe impl Send for PodcastHandle {}
unsafe impl Sync for PodcastHandle {}

/// Register a Podcast projection against `app`. Returns a non-null
/// `*mut PodcastHandle` on success; `null` if `app` is null.
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
/// on any failure. The returned pointer is owned by the caller; pass it to
/// [`nmp_app_podcast_snapshot_free`] when done.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_podcast_snapshot(handle: *mut PodcastHandle) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
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

/// Free a C string returned by any podcast FFI function that returns
/// `*mut c_char`. Null pointer is a silent no-op.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_podcast_snapshot_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: caller guarantees `ptr` came from a `CString::into_raw` in
    // this module and has not been freed.
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

/// Subscribe to a feed URL. Fire-and-forget.
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

/// Unsubscribe a podcast by its ULID string. Idempotent.
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
    let handle = unsafe { &*handle };
    let _ = handle.state.unsubscribe(id);
}

/// Inject raw RSS/Atom feed bytes for a subscribed podcast URL. The host
/// platform is responsible for fetching the bytes (T-podcast-gap-3).
///
/// Returns a JSON status string:
/// - `{"ok":true,"episode_count":N}` on success
/// - `{"ok":false,"reason":"..."}` on failure
///
/// The returned pointer is owned by the caller — free with
/// [`nmp_app_podcast_snapshot_free`]. Returns null if `handle` or
/// `bytes_ptr` is null.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_podcast_ingest_bytes(
    handle: *mut PodcastHandle,
    feed_url: *const c_char,
    bytes_ptr: *const u8,
    bytes_len: usize,
) -> *mut c_char {
    if handle.is_null() || bytes_ptr.is_null() {
        return std::ptr::null_mut();
    }
    let Some(url_str) = c_string_opt(feed_url) else {
        return json_cstr(r#"{"ok":false,"reason":"invalid-feed-url"}"#);
    };
    let Ok(url) = Url::parse(&url_str) else {
        return json_cstr(r#"{"ok":false,"reason":"invalid-feed-url"}"#);
    };
    // SAFETY: caller guarantees `bytes_ptr` is valid for `bytes_len` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(bytes_ptr, bytes_len) };
    let handle = unsafe { &*handle };
    let result = handle.state.ingest_feed_bytes(&url, bytes);
    let json = match result {
        IngestResult::Updated { episode_count, .. } => {
            format!(r#"{{"ok":true,"episode_count":{episode_count}}}"#)
        }
        IngestResult::PodcastNotFound => {
            r#"{"ok":false,"reason":"podcast-not-found"}"#.to_string()
        }
        IngestResult::ParseError(msg) => {
            let safe = msg.replace('"', "'").replace('\\', "/");
            format!(r#"{{"ok":false,"reason":"parse-error","detail":"{safe}"}}"#)
        }
    };
    json_cstr(&json)
}

/// Return the episode list for one podcast as a
/// `podcast_core::views::FeedView` JSON string. Unknown podcast ids return
/// `{"episodes":[]}` (honest empty state). Null on null handle or encode error.
///
/// Free the returned pointer with [`nmp_app_podcast_snapshot_free`].
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_podcast_episodes(
    handle: *mut PodcastHandle,
    podcast_id: *const c_char,
) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    // Unknown / missing id → return empty FeedView.
    let id = c_string_opt(podcast_id)
        .and_then(|s| Ulid::from_str(&s).ok())
        .unwrap_or_default();
    let handle = unsafe { &*handle };
    let view = handle.state.episodes_for(id);
    let Ok(json) = serde_json::to_string(&view) else {
        return std::ptr::null_mut();
    };
    let Ok(cstr) = CString::new(json) else {
        return std::ptr::null_mut();
    };
    cstr.into_raw()
}

/// Drop the handle and release the state. Null pointer is a silent no-op.
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

// --- private helpers --------------------------------------------------------

fn c_string_opt(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees `ptr` is a valid nul-terminated C string.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(|s| s.to_owned())
}

fn json_cstr(s: &str) -> *mut c_char {
    CString::new(s)
        .unwrap_or_else(|_| CString::new("{}").unwrap())
        .into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::nmp_app_free;
    use nmp_core::nmp_app_new;
    use podcast_core::views::{FeedView, LibraryView};

    fn cstr(s: &str) -> CString {
        CString::new(s).expect("no internal nul")
    }

    fn rss2_one_episode() -> Vec<u8> {
        br#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>FFI Podcast</title>
    <description>A test</description>
    <item>
      <title>Episode One</title>
      <guid>ep-001</guid>
      <pubDate>Mon, 01 Jan 2024 00:00:00 +0000</pubDate>
      <enclosure url="https://example.com/ep1.mp3" type="audio/mpeg" length="100"/>
      <description>First ep</description>
    </item>
  </channel>
</rss>"#
            .to_vec()
    }

    /// End-to-end FFI round trip: register → subscribe → snapshot → unregister.
    #[test]
    fn register_subscribe_snapshot_unregister_round_trip() {
        let app = nmp_app_new();
        let handle = nmp_app_podcast_register(app);
        assert!(!handle.is_null(), "register returned null");

        // Empty snapshot.
        let snap = nmp_app_podcast_snapshot(handle);
        assert!(!snap.is_null());
        let json = unsafe { CStr::from_ptr(snap) }.to_str().unwrap().to_owned();
        nmp_app_podcast_snapshot_free(snap);
        let view: LibraryView = serde_json::from_str(&json).expect("decode empty view");
        assert!(view.podcasts.is_empty());

        // Subscribe.
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
        assert_eq!(view.podcasts[0].episode_count, 0, "no ingest yet");

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
        let eps = nmp_app_podcast_episodes(std::ptr::null_mut(), std::ptr::null());
        assert!(eps.is_null());
        let ingest = nmp_app_podcast_ingest_bytes(
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            0,
        );
        assert!(ingest.is_null());
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

    #[test]
    fn ingest_bytes_populates_episodes_via_ffi() {
        let app = nmp_app_new();
        let handle = nmp_app_podcast_register(app);

        let feed_url_cstr = cstr("https://feeds.example.com/show.xml");
        let title_cstr = cstr("Show");
        nmp_app_podcast_subscribe(
            handle,
            feed_url_cstr.as_ptr(),
            title_cstr.as_ptr(),
            std::ptr::null(),
        );

        // Confirm episode_count = 0 before ingest.
        let snap = nmp_app_podcast_snapshot(handle);
        let json = unsafe { CStr::from_ptr(snap) }.to_str().unwrap().to_owned();
        nmp_app_podcast_snapshot_free(snap);
        let view: LibraryView = serde_json::from_str(&json).unwrap();
        assert_eq!(view.podcasts[0].episode_count, 0);

        // Ingest real RSS bytes.
        let bytes = rss2_one_episode();
        let ingest_result = nmp_app_podcast_ingest_bytes(
            handle,
            feed_url_cstr.as_ptr(),
            bytes.as_ptr(),
            bytes.len(),
        );
        assert!(!ingest_result.is_null());
        let result_json =
            unsafe { CStr::from_ptr(ingest_result) }.to_str().unwrap().to_owned();
        nmp_app_podcast_snapshot_free(ingest_result);
        assert!(
            result_json.contains("\"ok\":true"),
            "ingest should succeed: {result_json}"
        );
        assert!(
            result_json.contains("\"episode_count\":1"),
            "should count 1 episode: {result_json}"
        );

        // Confirm episode_count = 1 in library snapshot.
        let snap = nmp_app_podcast_snapshot(handle);
        let json = unsafe { CStr::from_ptr(snap) }.to_str().unwrap().to_owned();
        nmp_app_podcast_snapshot_free(snap);
        let view: LibraryView = serde_json::from_str(&json).unwrap();
        assert_eq!(view.podcasts[0].episode_count, 1);

        // Get episodes via FFI.
        let podcast_id_str = &view.podcasts[0].id;
        let podcast_id_cstr = cstr(podcast_id_str);
        let eps_raw = nmp_app_podcast_episodes(handle, podcast_id_cstr.as_ptr());
        assert!(!eps_raw.is_null());
        let eps_json = unsafe { CStr::from_ptr(eps_raw) }.to_str().unwrap().to_owned();
        nmp_app_podcast_snapshot_free(eps_raw);
        let feed_view: FeedView = serde_json::from_str(&eps_json).unwrap();
        assert_eq!(feed_view.episodes.len(), 1);
        assert_eq!(feed_view.episodes[0].title, "Episode One");

        nmp_app_podcast_unregister(handle);
        nmp_app_free(app);
    }

    #[test]
    fn ingest_malformed_feed_returns_error_json_no_fake_episodes() {
        let app = nmp_app_new();
        let handle = nmp_app_podcast_register(app);

        let feed_url_cstr = cstr("https://feeds.example.com/bad.xml");
        nmp_app_podcast_subscribe(
            handle,
            feed_url_cstr.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
        );

        let bad_bytes = b"NOT VALID XML";
        let ingest_result = nmp_app_podcast_ingest_bytes(
            handle,
            feed_url_cstr.as_ptr(),
            bad_bytes.as_ptr(),
            bad_bytes.len(),
        );
        let result_json =
            unsafe { CStr::from_ptr(ingest_result) }.to_str().unwrap().to_owned();
        nmp_app_podcast_snapshot_free(ingest_result);
        assert!(
            result_json.contains("\"ok\":false"),
            "malformed feed must return ok=false: {result_json}"
        );

        // No fake episodes stored.
        let snap = nmp_app_podcast_snapshot(handle);
        let json = unsafe { CStr::from_ptr(snap) }.to_str().unwrap().to_owned();
        nmp_app_podcast_snapshot_free(snap);
        let view: LibraryView = serde_json::from_str(&json).unwrap();
        assert_eq!(view.podcasts[0].episode_count, 0, "no fake episodes after parse error");

        nmp_app_podcast_unregister(handle);
        nmp_app_free(app);
    }
}
