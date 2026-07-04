//! NIP-87 mint discovery runtime controller (issue #2880).
//!
//! [`MintDiscoveryRuntime`] wires the read side of mint discovery onto the app
//! exactly the way [`crate::runtime::WalletRuntime`] wires the wallet's own
//! reads: identity-change-reactive [`ObservedProjectionReconciler`]s driven by
//! [`IdentityChangeRegistrar`], each feeding a shared sink. Nothing new is
//! invented — this is the same reconciler recipe `nmp-nip51::register_mute_runtime`
//! established, composed with the reused `nmp-wot` scoring engine.
//!
//! Two observed projections:
//!
//! - a **global** read of every kind:38172 announcement + kind:38000
//!   recommendation ([`mint_discovery_shape`]); and
//! - a **self-scoped** read of the active account's kind:3 / kind:10000
//!   ([`mint_discovery_trust_graph_shape`]) that builds the web-of-trust graph
//!   used to score recommenders.
//!
//! Both feed one [`MintDiscoveryStore`] behind a `Mutex`. The app queries the
//! current view via [`MintDiscoveryRuntime::snapshot`] — the same
//! runtime-holds-the-projection access `WalletRuntime::snapshot` uses. A typed
//! FlatBuffers snapshot sidecar (so a shell can subscribe rather than poll) is
//! a deliberate follow-up, identical to the deferral documented in
//! `register.rs` for the merged `"wallet"` projection: it is schema-design
//! work, not composition wiring, and the ranked view is already produced and
//! unit-tested here.

use std::sync::{Arc, Mutex};

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::substrate::{
    IdentityChangeRegistrar, KernelEvent, ObservedProjectionReconciler,
    ObservedProjectionRegistrar,
};
use nmp_core::ObservedProjectionSink;

use crate::interests::{mint_discovery_shape, mint_discovery_trust_graph_shape};
use crate::mint_discovery::{MintDiscoveryProjection, MintDiscoveryStore};

/// `ObservedProjection::scope` for `InterestScope::Global` (any nonzero value).
/// Mint announcements/recommendations are author-unknown public content that
/// needs the planner's cold-start bootstrap-relay lane, exactly like
/// `nmp-nip57`'s zap-receipt read — so the global read opens `Global`.
const SCOPE_GLOBAL: u32 = 1;

/// `ObservedProjection::scope` for `InterestScope::ActiveAccount`. The trust
/// graph read is an `authors`=self filter with no cold-start bootstrap
/// dependency (mirrors `wallet_self_authored_shape`).
const SCOPE_ACTIVE_ACCOUNT: u32 = 0;

/// Bounded cache replay on (re)open — matches `WalletRuntime`'s replay limit.
const REPLAY_LIMIT: usize = 512;

/// Feeds every observed discovery/graph event into the shared store.
struct MintDiscoverySink {
    store: Arc<Mutex<MintDiscoveryStore>>,
}

impl ObservedProjectionSink for MintDiscoverySink {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if let Ok(mut store) = self.store.lock() {
            store.ingest_kernel_event(event);
        }
    }
}

/// The mint-discovery runtime controller. See module docs.
pub struct MintDiscoveryRuntime {
    store: Arc<Mutex<MintDiscoveryStore>>,
}

impl MintDiscoveryRuntime {
    /// Construct the runtime and wire its two identity-reactive observed
    /// projections onto `app`.
    pub fn new(
        active_pubkey: ActiveAccountSlot,
        app: &(impl ObservedProjectionRegistrar + IdentityChangeRegistrar),
    ) -> Self {
        let store = Arc::new(Mutex::new(MintDiscoveryStore::new()));
        let sink: Arc<dyn ObservedProjectionSink> = Arc::new(MintDiscoverySink {
            store: Arc::clone(&store),
        });

        // Global announcements + recommendations: fixed shape, identity-independent.
        let discovery_reconciler = ObservedProjectionReconciler::new(
            app.observed_projection_registrar_handle(),
            Arc::clone(&sink),
            "nmp.wallet.mint_discovery",
            SCOPE_GLOBAL,
            REPLAY_LIMIT,
            Arc::new(|| Some(mint_discovery_shape())),
        );

        // Self follow/mute graph: re-derived from the active pubkey.
        let graph_pubkey = Arc::clone(&active_pubkey);
        let graph_reconciler = ObservedProjectionReconciler::new(
            app.observed_projection_registrar_handle(),
            sink,
            "nmp.wallet.mint_discovery_trust_graph",
            SCOPE_ACTIVE_ACCOUNT,
            REPLAY_LIMIT,
            Arc::new(move || {
                let pubkey = graph_pubkey.lock().ok()?.clone()?;
                Some(mint_discovery_trust_graph_shape(&pubkey))
            }),
        );

        // Keep the store's scoring viewer aligned with the active account, and
        // re-sync the reconcilers, on every identity change.
        let identity_store = Arc::clone(&store);
        let identity_pubkey = Arc::clone(&active_pubkey);
        let discovery_for_identity = discovery_reconciler.clone();
        let graph_for_identity = graph_reconciler.clone();
        app.register_identity_change_observer(move |_| {
            if let Ok(mut store) = identity_store.lock() {
                let viewer = identity_pubkey.lock().ok().and_then(|slot| slot.clone());
                store.set_viewer(viewer);
            }
            discovery_for_identity.sync();
            graph_for_identity.sync();
        });

        // Eager cold-start: the account may already be active.
        if let Ok(mut store_guard) = store.lock() {
            let viewer = active_pubkey.lock().ok().and_then(|slot| slot.clone());
            store_guard.set_viewer(viewer);
        }
        discovery_reconciler.sync();
        graph_reconciler.sync();

        Self { store }
    }

    /// The current discovered-mints projection (ranked, capability-filtered,
    /// WoT-scoped). Empty until an account is active.
    #[must_use]
    pub fn snapshot(&self) -> MintDiscoveryProjection {
        self.store
            .lock()
            .map(|store| store.snapshot())
            .unwrap_or_default()
    }
}
