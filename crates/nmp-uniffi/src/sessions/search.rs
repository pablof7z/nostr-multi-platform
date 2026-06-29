//! NIP-50 search session UniFFI surface — M14-C5.
//!
//! | UniFFI method      | C-ABI counterpart                           |
//! |--------------------|---------------------------------------------|
//! | `search_open`      | `nmp_app_search_open` (`nmp-ffi/src/search.rs`)  |
//! | `search_close`     | `nmp_app_search_close` (`nmp-ffi/src/search.rs`) |
//! | `search_snapshot`  | `nmp_app_search_snapshot` (`nmp-ffi/src/search.rs`)|
//!
//! ## Session model
//!
//! Sessions are keyed by a caller-supplied `session_id` string (exactly as in
//! the C-ABI). The `session_id` IS the subscription handle — pass the same
//! string to `search_open` and later to `search_close` / `search_snapshot`.
//!
//! Re-opening an existing `session_id` first tears down the prior session
//! (idempotent re-open). Closing an unknown or already-closed `session_id` is a
//! silent no-op (D6). An empty `session_id` is also a silent no-op (parity with
//! the C-ABI null-filter path).
//!
//! ## Snapshot return
//!
//! `search_snapshot` returns `Option<Vec<u8>>` directly — the caller gets the
//! raw FlatBuffers `N50S` bytes without managing an output buffer. `None` means
//! no session is open under `session_id` or the session has no results yet.
//! This is the typed UniFFI form of the C-ABI's `out_buf + len_return` pair,
//! backed by the same `NmpApp::search_session_snapshot_bytes` call.

use nmp_native_runtime::{parse_search_request, Nip50SearchSession};

use crate::NmpApp;

#[uniffi::export]
impl NmpApp {
    /// Open a NIP-50 search session from a JSON query payload.
    ///
    /// Mirrors `nmp_app_search_open`. `request_json` must be a JSON object
    /// `{"query":"…","scope":…,"targets":…}` parseable by
    /// `nmp_native_runtime::parse_search_request`. `session_id` is a
    /// caller-chosen non-empty key that scopes the session for teardown and
    /// snapshot access.
    ///
    /// Re-opening the same `session_id` first closes the prior session (the
    /// relay interests and projection are rebuilt from the new request). The
    /// snapshot projection key is `"nmp.nip50.search.<session_id>"`.
    ///
    /// D6: an empty `session_id`, an unparseable `request_json`, or a poisoned
    /// mutex are all silent no-ops. D8: the relay-fan-out is async; the first
    /// synchronous cache scan runs before this returns.
    pub fn search_open(&self, request_json: String, session_id: String) {
        if session_id.is_empty() {
            return;
        }
        let Some(request) = parse_search_request(&request_json) else {
            return;
        };
        let Ok(mut search_handles) = self.search_handles.lock() else {
            return;
        };
        let handle = self
            .inner
            .open_search_session(Nip50SearchSession::new(request, session_id.clone()));
        search_handles.insert(session_id, handle);
    }

    /// Close a NIP-50 search session previously opened via `search_open`.
    ///
    /// Mirrors `nmp_app_search_close`. Tears down the relay interests and
    /// removes the typed snapshot projection for `session_id`. An empty or
    /// unknown `session_id` is a silent no-op (D6).
    pub fn search_close(&self, session_id: String) {
        if session_id.is_empty() {
            return;
        }
        let handle = self
            .search_handles
            .lock()
            .ok()
            .and_then(|mut search_handles| search_handles.remove(&session_id));
        if let Some(handle) = handle {
            self.inner.close_search_session(&handle);
        }
    }

    /// Copy the current typed `N50S` search-results snapshot for a session.
    ///
    /// Mirrors `nmp_app_search_snapshot` but returns bytes directly instead of
    /// writing into a caller-provided buffer. Returns `None` when no live
    /// session is registered under `session_id` or when the session has no
    /// results yet. The returned bytes are a FlatBuffers `N50S` frame; the
    /// caller should validate the file identifier before parsing.
    ///
    /// D6: an empty `session_id` or a poisoned mutex returns `None`.
    pub fn search_snapshot(&self, session_id: String) -> Option<Vec<u8>> {
        if session_id.is_empty() {
            return None;
        }
        let handle = self
            .search_handles
            .lock()
            .ok()
            .and_then(|search_handles| search_handles.get(&session_id).cloned())?;
        self.inner.search_session_snapshot_bytes(&handle)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // ── Parity: search_open ───────────────────────────────────────────────

    /// Parity with C-ABI `nmp_app_search_open`:
    /// a valid request JSON + non-empty session id must not panic (D6 + D8).
    #[test]
    fn parity_search_open_valid_request_no_panic() {
        let app = crate::NmpApp::new();
        app.search_open(
            r#"{"query":"hello","scope":"Global","targets":"UserPreferred"}"#.to_string(),
            "test-session-1".to_string(),
        );
    }

    /// Parity with C-ABI `nmp_app_search_open` empty-session-id path:
    /// an empty `session_id` must be a silent no-op (D6).
    #[test]
    fn parity_search_open_empty_session_id_is_noop() {
        let app = crate::NmpApp::new();
        app.search_open(
            r#"{"query":"hello","scope":"Global","targets":"UserPreferred"}"#.to_string(),
            String::new(),
        );
    }

    /// Parity with C-ABI `nmp_app_search_open` null-request path:
    /// an invalid/empty `request_json` must be a silent no-op (D6).
    #[test]
    fn parity_search_open_invalid_request_is_noop() {
        let app = crate::NmpApp::new();
        app.search_open("{}".to_string(), "test-session-2".to_string());
    }

    // ── Parity: search_close ──────────────────────────────────────────────

    /// Parity with C-ABI `nmp_app_search_close`:
    /// closing an unknown session must be a silent no-op (D6).
    #[test]
    fn parity_search_close_unknown_session_is_noop() {
        let app = crate::NmpApp::new();
        app.search_close("never-opened".to_string());
    }

    /// An empty session id must be a silent no-op (D6).
    #[test]
    fn parity_search_close_empty_session_id_is_noop() {
        let app = crate::NmpApp::new();
        app.search_close(String::new());
    }

    // ── Parity: search_snapshot ───────────────────────────────────────────

    /// Parity with C-ABI `nmp_app_search_snapshot` null-session path:
    /// an unknown session must return `None` (D6).
    #[test]
    fn parity_search_snapshot_unknown_session_returns_none() {
        let app = crate::NmpApp::new();
        let result = app.search_snapshot("never-opened".to_string());
        assert!(result.is_none(), "unknown session must return None");
    }

    /// An empty session id must return `None` (D6).
    #[test]
    fn parity_search_snapshot_empty_session_id_returns_none() {
        let app = crate::NmpApp::new();
        let result = app.search_snapshot(String::new());
        assert!(result.is_none(), "empty session_id must return None");
    }

    // ── Teardown / idempotency ─────────────────────────────────────────────

    /// Open → close → close (idempotent): the second close must not panic (D6).
    /// This is the canonical teardown test for search sessions.
    #[test]
    fn teardown_search_open_then_close_then_close_idempotent() {
        let app = crate::NmpApp::new();
        let session_id = "teardown-test-session".to_string();

        app.search_open(
            r#"{"query":"idempotent","scope":"Global","targets":"UserPreferred"}"#.to_string(),
            session_id.clone(),
        );

        // First close — tears down the session.
        app.search_close(session_id.clone());

        // Second close — no session to close; must be a silent no-op (D6).
        app.search_close(session_id.clone());
    }

    /// Re-opening the same session id must not panic (idempotent re-open path).
    #[test]
    fn teardown_search_reopen_same_session_id_no_panic() {
        let app = crate::NmpApp::new();
        let session_id = "reopen-test-session".to_string();

        app.search_open(
            r#"{"query":"first","scope":"Global","targets":"UserPreferred"}"#.to_string(),
            session_id.clone(),
        );
        // Second open tears down the first and opens fresh.
        app.search_open(
            r#"{"query":"second","scope":"Global","targets":"UserPreferred"}"#.to_string(),
            session_id.clone(),
        );

        app.search_close(session_id);
    }
}
