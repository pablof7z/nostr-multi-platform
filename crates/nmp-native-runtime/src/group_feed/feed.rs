//! Shared hydrating-feed machinery for the per-open NIP-29 read views.
//!
//! Split out of `group_feed.rs` (file-size cap) so that file owns only the
//! public per-view session entry points + their identity constants, while this
//! submodule owns the common open/teardown plumbing every view drives. The
//! methods are `pub(super)` / `pub(crate)` so the parent module's view APIs can
//! call them across the module boundary.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use nmp_core::substrate::{ObservedProjection, ObservedProjectionRegistrar};
use nmp_feed::DEFAULT_FEED_WINDOW_LIMIT;

use crate::app_struct::NmpApp;

use super::{GroupFeedSession, NEXT_GROUP_READ_HANDLE_ID};

impl NmpApp {
    /// Shared open path for the hydrating NIP-29 read views.
    ///
    /// Idempotently tears down any prior session under `key` first (singleton
    /// semantics), registers the typed sidecar, registers the projection MUTED,
    /// then opens the relay-pinned observed interest with read-cache replay
    /// shapes derived from the same wire filter (so the in-memory cache is
    /// hydrated to the muted observer before it is activated — the #2088 fix).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn open_group_feed(
        &self,
        key: &str,
        consumer: &str,
        scope: u32,
        relay_pin: Option<String>,
        filter_json: String,
        observer: Arc<dyn nmp_core::ObservedProjectionSink>,
        register_sidecar: impl FnOnce(&NmpApp),
    ) -> u64 {
        // Singleton: drop any prior session under this key first. Teardown must
        // run BEFORE the replacement registers — both sessions share the same
        // projection key, so a late key-based teardown would remove the new
        // view's sidecar.
        self.close_group_feed(key);
        let handle_id = NEXT_GROUP_READ_HANDLE_ID.fetch_add(1, Ordering::Relaxed);

        register_sidecar(self);

        // The in-memory read-cache replay (ADR-0062) matches cached events by
        // the SAME wire shape the live filter uses — `matches_event_with_id`
        // honours the `#h` generic-tag + kind dimensions. A malformed filter
        // yields no shapes; `open_observed_projection` validates the filter and
        // no-ops the interest open while returning the observer id.
        let replay_shapes: Vec<nmp_planner::InterestShape> =
            nmp_planner::InterestShape::from_filter_json(&filter_json)
                .map(|mut shape| {
                    shape.relay_pin = relay_pin.clone();
                    shape
                })
                .into_iter()
                .collect();

        let observer_id = self.open_observed_projection(ObservedProjection {
            observer,
            filter_json,
            consumer_id: consumer.to_string(),
            scope,
            relay_pin,
            replay_shapes,
            replay_limit: DEFAULT_FEED_WINDOW_LIMIT,
        });
        if observer_id.0 == 0 {
            self.remove_snapshot_projection(key);
            return handle_id;
        }

        let Ok(mut sessions) = self.group_feed_sessions.lock() else {
            self.close_observed_projection(observer_id);
            self.remove_snapshot_projection(key);
            return handle_id;
        };
        sessions.insert(
            key.to_string(),
            GroupFeedSession {
                projection_key: key.to_string(),
                handle_id,
                observer_id,
            },
        );
        handle_id
    }

    /// Tear down the NIP-29 read view registered under `key`: detach the pinned
    /// interest, revoke the observer, and remove the typed sidecar. Idempotent —
    /// closing an unknown view is a harmless no-op (D6).
    pub(crate) fn close_group_feed(&self, key: &str) {
        let session = self
            .group_feed_sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(key));
        let Some(session) = session else {
            return;
        };
        self.close_observed_projection(session.observer_id);
        self.remove_snapshot_projection(&session.projection_key);
    }

    pub(super) fn close_group_feed_handle(&self, key: &str, handle_id: u64) {
        let session = self
            .group_feed_sessions
            .lock()
            .ok()
            .and_then(|mut sessions| {
                let should_remove = sessions
                    .get(key)
                    .map(|session| session.handle_id == handle_id)
                    .unwrap_or(false);
                if should_remove {
                    sessions.remove(key)
                } else {
                    None
                }
            });
        let Some(session) = session else {
            return;
        };
        self.close_observed_projection(session.observer_id);
        self.remove_snapshot_projection(&session.projection_key);
    }
}
