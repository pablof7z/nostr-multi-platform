//! Browser feed-session lifecycle hooks.
//!
//! Kept out of `handle.rs` so the public handle stays a compact owner of slots
//! and API methods while feed-specific open/close/drop behavior is co-located.

use std::sync::Arc;

use super::BrowserRuntimeHandle;
use crate::feed::{open_browser_feed_session, FeedRuntimeAccess};

/// Borrowed browser feed-session facade.
///
/// This type owns no state. It delegates to `BrowserRuntimeHandle`'s existing
/// handle-owned feed lifecycle so browser app code can use the same
/// `runtime.feeds().open/open_spec/load_older/close` shape without seeing
/// compiler or registry wiring.
pub struct BrowserFeedSessions<'a> {
    handle: &'a mut BrowserRuntimeHandle,
}

impl<'a> BrowserFeedSessions<'a> {
    fn new(handle: &'a mut BrowserRuntimeHandle) -> Self {
        Self { handle }
    }

    /// Open a caller-owned browser feed session through the standard NMP feed
    /// compiler.
    pub fn open(&mut self, params: nmp_feed::FeedParams) -> Option<nmp_feed::FeedHandle> {
        self.handle.open_feed(params)
    }

    /// Open an ergonomic feed spec through the standard NMP feed compiler.
    ///
    /// The spec is first compiled into canonical [`nmp_feed::FeedParams`].
    /// Invalid specs and compiler failures both return `None`, matching the
    /// existing browser lifecycle style.
    pub fn open_spec(
        &mut self,
        key: nmp_feed::FeedKey,
        spec: nmp_feed::FeedSpec,
    ) -> Option<nmp_feed::FeedHandle> {
        let params = spec.into_params(key).ok()?;
        self.open(params)
    }

    /// Register a custom source definition for browser feed declarations.
    #[must_use]
    pub fn register_custom_source(
        &self,
        id: nmp_feed::CustomSourceId,
        def: nmp_feed::CustomSourceDef,
    ) -> bool {
        self.handle.register_custom_source(id, def)
    }

    /// Register a custom admission-gate definition for browser feed declarations.
    #[must_use]
    pub fn register_custom_admission(
        &self,
        id: nmp_feed::CustomAdmissionId,
        def: nmp_feed::CustomAdmissionDef,
    ) -> bool {
        self.handle.register_custom_admission(id, def)
    }

    /// Register a custom order definition for browser feed declarations.
    #[must_use]
    pub fn register_custom_order(
        &self,
        id: nmp_feed::CustomOrderId,
        def: nmp_feed::CustomOrderDef,
    ) -> bool {
        self.handle.register_custom_order(id, def)
    }

    /// The custom source definition registered under `id`, or `None`.
    #[must_use]
    pub fn custom_source(
        &self,
        id: &nmp_feed::CustomSourceId,
    ) -> Option<nmp_feed::CustomSourceDef> {
        self.handle.custom_source(id)
    }

    /// The custom admission-gate definition registered under `id`, or `None`.
    #[must_use]
    pub fn custom_admission(
        &self,
        id: &nmp_feed::CustomAdmissionId,
    ) -> Option<nmp_feed::CustomAdmissionDef> {
        self.handle.custom_admission(id)
    }

    /// The custom order definition registered under `id`, or `None`.
    #[must_use]
    pub fn custom_order(&self, id: &nmp_feed::CustomOrderId) -> Option<nmp_feed::CustomOrderDef> {
        self.handle.custom_order(id)
    }

    /// Page an open browser feed by its returned handle.
    #[must_use]
    pub fn load_older(&mut self, handle: &nmp_feed::FeedHandle) -> bool {
        self.handle.load_older_feed(handle)
    }

    /// Page an open browser feed and return the Rust-owned stop reason.
    #[must_use]
    pub fn load_older_status(&mut self, handle: &nmp_feed::FeedHandle) -> nmp_feed::FeedLoadStatus {
        self.handle.load_older_feed_status(handle)
    }

    /// Close an open browser feed by its returned handle.
    #[must_use]
    pub fn close(&mut self, handle: &nmp_feed::FeedHandle) -> bool {
        self.handle.close_feed(handle)
    }
}

impl BrowserRuntimeHandle {
    /// App-facing feed-session facade.
    #[must_use]
    pub fn feeds(&mut self) -> BrowserFeedSessions<'_> {
        BrowserFeedSessions::new(self)
    }

    /// Register a CLOSED-DATA custom source definition under an opaque source id.
    ///
    /// Register-once: returns `true` when newly registered, `false` if the id
    /// already existed or the registry lock is poisoned.
    #[must_use]
    pub fn register_custom_source(
        &self,
        id: nmp_feed::CustomSourceId,
        def: nmp_feed::CustomSourceDef,
    ) -> bool {
        self.custom_feed_policies.register_source(id, def)
    }

    /// Register a CLOSED-DATA custom admission-gate definition.
    #[must_use]
    pub fn register_custom_admission(
        &self,
        id: nmp_feed::CustomAdmissionId,
        def: nmp_feed::CustomAdmissionDef,
    ) -> bool {
        self.custom_feed_policies.register_admission(id, def)
    }

    /// Register a CLOSED-DATA custom order definition.
    #[must_use]
    pub fn register_custom_order(
        &self,
        id: nmp_feed::CustomOrderId,
        def: nmp_feed::CustomOrderDef,
    ) -> bool {
        self.custom_feed_policies.register_order(id, def)
    }

    /// The custom source definition registered under `id`, or `None`.
    #[must_use]
    pub fn custom_source(
        &self,
        id: &nmp_feed::CustomSourceId,
    ) -> Option<nmp_feed::CustomSourceDef> {
        self.custom_feed_policies.get_source(id)
    }

    /// The custom admission-gate definition registered under `id`, or `None`.
    #[must_use]
    pub fn custom_admission(
        &self,
        id: &nmp_feed::CustomAdmissionId,
    ) -> Option<nmp_feed::CustomAdmissionDef> {
        self.custom_feed_policies.get_admission(id)
    }

    /// The custom order definition registered under `id`, or `None`.
    #[must_use]
    pub fn custom_order(&self, id: &nmp_feed::CustomOrderId) -> Option<nmp_feed::CustomOrderDef> {
        self.custom_feed_policies.get_order(id)
    }

    /// Test/diagnostic — count of registered custom feed policies.
    #[must_use]
    pub fn custom_feed_policy_count(&self) -> usize {
        self.custom_feed_policies.len()
    }

    /// Open a caller-owned browser feed session.
    ///
    /// The caller supplies the full [`nmp_feed::FeedParams`], including the
    /// output key and item projection. Browser runtime only wires those params
    /// into the shared NMP feed machinery; it does not mint a product/default
    /// feed key.
    pub fn open_feed(&mut self, params: nmp_feed::FeedParams) -> Option<nmp_feed::FeedHandle> {
        let observed_projection_registrar = self.observed_projection_registrar.clone();
        let command_sender = self.command_sender();
        let feed_registry = Arc::clone(&self.feed_registry);
        let custom_feed_policies = Arc::clone(&self.custom_feed_policies);
        let identity_observers = Arc::clone(&self.runtime.identity_change_observers);
        let identity_observer_next_id = Arc::clone(&self.identity_observer_next_id);
        let opened = open_browser_feed_session(
            &self.feed_sessions,
            FeedRuntimeAccess::new(
                &self.runtime.reducer,
                observed_projection_registrar,
                command_sender,
                feed_registry,
                custom_feed_policies,
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

    /// Open a caller-owned browser feed spec.
    pub fn open_feed_spec(
        &mut self,
        key: nmp_feed::FeedKey,
        spec: nmp_feed::FeedSpec,
    ) -> Option<nmp_feed::FeedHandle> {
        let params = spec.into_params(key).ok()?;
        self.open_feed(params)
    }

    /// Page a browser feed session opened by [`Self::open_feed`].
    ///
    /// The returned handle is the public lifecycle token. Browser runtime uses
    /// the session registry to resolve the live projection key before touching
    /// the internal controller registry, so a stale id or mismatched forged
    /// handle is a silent no-op.
    pub fn load_older_feed(&mut self, handle: &nmp_feed::FeedHandle) -> bool {
        self.load_older_feed_status(handle).changed
    }

    /// Page a browser feed session and return the typed load status.
    pub fn load_older_feed_status(
        &mut self,
        handle: &nmp_feed::FeedHandle,
    ) -> nmp_feed::FeedLoadStatus {
        let Some(projection_key) = self.feed_sessions.projection_key(&handle.session_id) else {
            return nmp_feed::FeedLoadStatus::session_unavailable();
        };
        if projection_key != handle.projection_key {
            return nmp_feed::FeedLoadStatus::session_unavailable();
        }
        let status = self
            .feed_registry
            .load_older_status(projection_key.as_str());
        if status.changed {
            self.command_sender().mark_changed_since_emit();
        }
        status
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
