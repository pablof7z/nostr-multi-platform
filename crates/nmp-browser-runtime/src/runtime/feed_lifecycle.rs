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

    /// Page a browser feed session opened by [`Self::open_feed`].
    ///
    /// The returned handle is the public lifecycle token. Browser runtime uses
    /// the session registry to resolve the live projection key before touching
    /// the internal controller registry, so a stale id or mismatched forged
    /// handle is a silent no-op.
    pub fn load_older_feed(&mut self, handle: &nmp_feed::FeedHandle) -> bool {
        let Some(projection_key) = self.feed_sessions.projection_key(&handle.session_id) else {
            return false;
        };
        if projection_key != handle.projection_key {
            return false;
        }
        let changed = self.feed_registry.load_older(projection_key.as_str());
        if changed {
            self.command_sender().mark_changed_since_emit();
        }
        changed
    }

    /// Close a browser feed session opened by [`Self::open_feed`].
    ///
    /// Idempotent: an unknown or already-closed handle returns `false`.
    pub fn close_feed(&mut self, handle: &nmp_feed::FeedHandle) -> bool {
        let Some(projection_key) = self.feed_sessions.projection_key(&handle.session_id) else {
            return false;
        };
        if projection_key != handle.projection_key {
            return false;
        }
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
