//! Native adapter for the shared feed-session compiler.

use std::sync::Arc;

use crate::NmpApp;
use nmp_core::substrate::ObservedProjectionCommandHandle;
use nmp_core::{CommandSender, TypedProjectionData};
use nmp_feed::{
    CustomAdmissionDef, CustomAdmissionId, CustomOrderDef, CustomOrderId, CustomSourceDef,
    CustomSourceId, FeedAuthorRefs, FeedController, FeedWindowSource, PullFn, TeardownAction,
};
use nmp_feed_session::{FeedSessionHost, IdentityChangeObserverId};

impl FeedSessionHost for NmpApp {
    fn active_account_handle(&self) -> nmp_core::slots::ActiveAccountSlot {
        NmpApp::active_account_handle(self)
    }

    fn event_store_handle(&self) -> nmp_core::slots::EventStoreSlot {
        NmpApp::event_store_handle(self)
    }

    fn observed_projection_handle(&self) -> ObservedProjectionCommandHandle {
        NmpApp::observed_projection_handle(self)
    }

    fn register_identity_change_observer<F>(&self, callback: F) -> IdentityChangeObserverId
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
        NmpApp::register_identity_change_observer(self, callback)
    }

    fn unregister_identity_change_observer_action(
        &self,
        id: IdentityChangeObserverId,
    ) -> TeardownAction {
        self.feed_teardown().revoke_identity_observer(id)
    }

    fn feed_pull_fn(&self) -> PullFn {
        NmpApp::feed_pull_fn(self)
    }

    fn command_sender(&self) -> CommandSender {
        NmpApp::command_sender(self)
    }

    fn register_feed(&self, key: String, controller: Arc<dyn FeedController>) {
        NmpApp::register_feed(self, key, controller);
    }

    fn load_older_feed(&self, key: &str) -> bool {
        NmpApp::load_older_feed_by_key(self, key)
    }

    fn register_feed_window_source<S, F>(
        &self,
        feed_key: String,
        source: Arc<FeedWindowSource<S>>,
        encode: F,
    ) where
        S: FeedAuthorRefs + Send + Sync + 'static,
        F: Fn(&S) -> Option<TypedProjectionData> + Send + Sync + 'static,
    {
        NmpApp::register_feed_window_source(self, feed_key, source, encode);
    }

    fn custom_source(&self, id: &CustomSourceId) -> Option<CustomSourceDef> {
        NmpApp::custom_source(self, id)
    }

    fn custom_admission(&self, id: &CustomAdmissionId) -> Option<CustomAdmissionDef> {
        NmpApp::custom_admission(self, id)
    }

    fn custom_order(&self, id: &CustomOrderId) -> Option<CustomOrderDef> {
        NmpApp::custom_order(self, id)
    }

    fn unregister_feed_action(&self, key: String) -> TeardownAction {
        self.feed_teardown().unregister_feed(key)
    }

    fn remove_projection_action(&self, key: String) -> TeardownAction {
        self.feed_teardown().remove_projection(key)
    }

    fn mark_changed_action(&self) -> TeardownAction {
        self.feed_teardown().mark_changed()
    }
}
