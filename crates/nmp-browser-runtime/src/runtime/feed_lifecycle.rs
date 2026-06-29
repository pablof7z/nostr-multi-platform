//! Browser feed-session lifecycle hooks.
//!
//! Kept out of `handle.rs` so the public handle stays a compact owner of slots
//! and API methods while feed-specific open/close/drop behavior is co-located.

use std::sync::Arc;

use nmp_core::substrate::ContactsLookup;
use nmp_core::CommandSender;

use super::BrowserRuntimeHandle;
use crate::feed::{default_home_feed_params, open_default_home_feed, FeedRuntimeAccess};

impl BrowserRuntimeHandle {
    pub(crate) fn open_default_startup_feed(
        &mut self,
        contacts_lookup: Option<Arc<dyn ContactsLookup>>,
    ) {
        let Some(contacts_lookup) = contacts_lookup else {
            return;
        };
        let access = FeedRuntimeAccess {
            reducer: &mut self.runtime.reducer,
            contacts_lookup,
            observed_projection_registrar: self.observed_projection_registrar.clone(),
            command_sender: CommandSender::new_bounded(self.inbox_tx.clone()),
        };
        self.home_feed_session =
            open_default_home_feed(&self.feed_sessions, access, default_home_feed_params());
    }

    pub(crate) fn sync_feed_sessions_after_identity_change(&self) {
        if let Some(session) = &self.home_feed_session {
            session.sync_identity_change();
        }
    }

    pub(crate) fn close_home_feed_session(&mut self) -> bool {
        let Some(session) = self.home_feed_session.take() else {
            return false;
        };
        self.feed_sessions.close(&session.handle.session_id)
    }
}

impl Drop for BrowserRuntimeHandle {
    fn drop(&mut self) {
        let _ = self.close_home_feed_session();
    }
}
