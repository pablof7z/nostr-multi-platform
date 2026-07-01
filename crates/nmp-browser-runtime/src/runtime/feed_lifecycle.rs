//! Browser feed-session lifecycle hooks.
//!
//! Kept out of `handle.rs` so the public handle stays a compact owner of slots
//! and API methods while feed-specific open/close/drop behavior is co-located.

use std::sync::Arc;

use super::BrowserRuntimeHandle;
use crate::feed::{open_browser_feed_session, FeedRuntimeAccess};

impl BrowserRuntimeHandle {
    /// Open a caller-owned browser feed session.
    ///
    /// The caller supplies the full [`nmp_feed::FeedParams`], including the
    /// projection key. Browser runtime only wires those params into the shared
    /// NMP feed machinery; it does not mint a product/default feed key.
    pub fn open_feed(&mut self, params: nmp_feed::FeedParams) -> Option<nmp_feed::FeedHandle> {
        let observed_projection_registrar = self.observed_projection_registrar.clone();
        let command_sender = self.command_sender();
        let feed_registry = Arc::clone(&self.feed_registry);
        let identity_observers = Arc::clone(&self.runtime.identity_change_observers);
        let identity_observer_next_id = Arc::clone(&self.identity_observer_next_id);
        let opened = open_browser_feed_session(
            &self.feed_sessions,
            FeedRuntimeAccess::new(
                &self.runtime.reducer,
                observed_projection_registrar,
                command_sender,
                feed_registry,
                identity_observers,
                identity_observer_next_id,
            ),
            params,
        )?;
        let handle = opened.handle.clone();
        self.feed_session_runtimes
            .insert(handle.session_id.clone(), opened);
        Some(handle)
    }

    /// Close a browser feed session opened by [`Self::open_feed`].
    ///
    /// Idempotent: an unknown or already-closed handle returns `false`.
    pub fn close_feed(&mut self, handle: &nmp_feed::FeedHandle) -> bool {
        let Some(session) = self.feed_session_runtimes.remove(&handle.session_id) else {
            return false;
        };
        self.feed_sessions.close(&session.handle.session_id)
    }
}

impl Drop for BrowserRuntimeHandle {
    fn drop(&mut self) {
        let sessions = std::mem::take(&mut self.feed_session_runtimes);
        for (_, session) in sessions {
            let _ = self.feed_sessions.close(&session.handle.session_id);
        }
    }
}
