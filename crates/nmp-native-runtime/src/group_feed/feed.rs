//! Shared hydrating-feed machinery for native group-scoped composed reads.
//!
//! Pure NIP-29 group reads now use `nmp_nip29`'s concept-owned doorway. This
//! helper remains for native app-layer reads that compose multiple concepts,
//! currently the NIP-25 reaction aggregate scoped by a NIP-29 group.

use std::sync::Arc;

use nmp_core::ObservedProjectionSink;
use nmp_feed::DEFAULT_FEED_WINDOW_LIMIT;
use nmp_read_session::{
    close_read, open_read, ReadDemand, ReadHandle, ReadHost, ReadOutputEncoder, ReadReplayPolicy,
    ReadSpec,
};

use crate::app_struct::NmpApp;

impl NmpApp {
    /// Shared open path for hydrating group-scoped composed reads.
    ///
    /// Idempotently tears down any prior session under `key` first (singleton
    /// semantics), registers the typed sidecar, registers the projection MUTED,
    /// then opens the relay-pinned observed interest with read-cache replay
    /// shapes derived from the same wire filter (so the in-memory cache is
    /// hydrated to the muted observer before it is activated — the #2088 fix).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn open_group_feed(
        &self,
        key: nmp_ownership::DeclaredProjectionKey,
        consumer: &str,
        scope: u32,
        relay_pin: Option<String>,
        filter_json: String,
        observer: Arc<dyn ObservedProjectionSink>,
        output_encoder: ReadOutputEncoder,
    ) -> ReadHandle {
        // Singleton: drop any prior session under this key first. Teardown must
        // run BEFORE the replacement registers — both sessions share the same
        // projection key, so a late key-based teardown would remove the new
        // view's sidecar.
        let _ = self.close_read_session_by_projection_key(key.as_str());

        // The engine derives the read-cache replay shape from the SAME wire filter
        // the live demand uses, so `#h` generic-tag + kind matching cannot drift
        // from the interest that will tail live after replay.
        open_read(
            self,
            ReadSpec {
                projection_key: key.into(),
                demands: vec![ReadDemand {
                    filter_json,
                    consumer_id: consumer.to_string(),
                    scope,
                    relay_pin,
                    replay_limit: DEFAULT_FEED_WINDOW_LIMIT,
                    replay: ReadReplayPolicy::Structural,
                }],
                observer,
                output_encoder,
                dependent_demands: Vec::new(),
                keep_open_without_live_demand: false,
            },
        )
    }

    pub(super) fn close_group_feed_handle(&self, handle: &ReadHandle) {
        let _ = close_read(self, handle);
    }
}
