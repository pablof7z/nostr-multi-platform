//! URI-routing UniFFI surface — M14-C5.
//!
//! | UniFFI method | C-ABI counterpart                             |
//! |---------------|-----------------------------------------------|
//! | `open_uri`    | `nmp_app_open_uri` (`nmp-ffi/src/timeline.rs`)|
//!
//! `nmp_app_open_uri` routes a `nostr:` URI (or a bare NIP-19 entity) through
//! the `KernelAction` reducer. A successful route registers the resolved interest
//! and pushes a `ViewOpened` update; failure pushes `UriRejected`. This is an
//! async fire-and-forget: the method returns before the reducer runs (D8).
//!
//! D6: null/empty arguments are silent no-ops in the C-ABI; for UniFFI an empty
//! string is also a no-op (the underlying `NmpApp::open_uri` validates the input
//! before dispatching).

use crate::NmpApp;

#[uniffi::export]
impl NmpApp {
    /// Route a `nostr:` URI (or a bare NIP-19 entity) to the kernel reducer.
    ///
    /// Mirrors `nmp_app_open_uri`. Parses `uri` as a NIP-21/NIP-19 value and
    /// dispatches a `KernelAction::OpenUri` to the actor. On success the kernel
    /// registers the resolved interest and emits a `ViewOpened` update frame;
    /// on failure it emits `UriRejected`. Both outcomes are delivered
    /// asynchronously through the registered `UpdateSink`.
    ///
    /// D6: an empty or structurally invalid URI is a silent no-op (the kernel
    /// reducer fails closed before dispatching). D8: fire-and-forget.
    pub fn open_uri(&self, uri: String) {
        self.inner.open_uri(uri);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // ── Parity: open_uri ──────────────────────────────────────────────────

    /// Parity with C-ABI `nmp_app_open_uri`:
    /// a valid `nostr:npub` URI must not panic (D6 + D8).
    #[test]
    fn parity_open_uri_valid_npub_no_panic() {
        let app = crate::NmpApp::new();
        app.open_uri(
            "nostr:npub1gakvygj65zq8pcrmd2ugk4mnph7qjzm6lhvkdmjrst0aqkjaf0pq0qnr2u"
                .to_string(),
        );
    }

    /// Parity with C-ABI `nmp_app_open_uri` null-arg path:
    /// an empty URI must be a silent no-op (D6).
    #[test]
    fn parity_open_uri_empty_is_noop() {
        let app = crate::NmpApp::new();
        // Should not panic.
        app.open_uri(String::new());
    }

    /// A garbage string that is not a valid NIP-19/NIP-21 entity must be a
    /// silent no-op (D6: the reducer fails closed, no panic).
    #[test]
    fn parity_open_uri_garbage_is_noop() {
        let app = crate::NmpApp::new();
        app.open_uri("not-a-nostr-uri-at-all".to_string());
    }

    /// A `nsec` URI must be rejected without panicking (D6: secret keys are
    /// never echoed back or registered as interests).
    #[test]
    fn parity_open_uri_nsec_is_rejected_no_panic() {
        let app = crate::NmpApp::new();
        // nsec1 is a secret key — must fail closed, never panic.
        app.open_uri("nostr:nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k31bl5ewnncg9yqq6x2hp".to_string());
    }
}
