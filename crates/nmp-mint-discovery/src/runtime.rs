//! NIP-87 mint discovery runtime controller (issue #2880).
//!
//! [`MintDiscoveryRuntime`] wires the read side of mint discovery onto the
//! app: identity-change-reactive `ObservedProjectionReconciler`s driven by
//! `IdentityChangeRegistrar`, each feeding a shared sink. Nothing new is
//! invented — this is the same reconciler recipe `nmp-nip51::register_mute_runtime`
//! established, composed with the reused `nmp-wot` scoring engine.
//!
//! Two or three observed projections:
//!
//! - a **global** read of every kind:38172 announcement + kind:38000
//!   recommendation ([`crate::interests::mint_discovery_shape`]);
//! - a **self-scoped** read of the active account's kind:3 / kind:10000
//!   ([`crate::interests::mint_discovery_trust_graph_shape`]) that builds the
//!   web-of-trust graph used to score recommenders; and
//! - when [`DiscoveryPolicy::fallback_root`] is configured, a **fixed** read
//!   of that seed's OWN kind:3 / kind:10000 (same shape function, called with
//!   the seed's pubkey instead of the viewer's). Without this, a cold viewer
//!   who reroutes scoring through the seed (`nmp_wot::WotGraph::score_rooted`)
//!   would find no `seed -> follows` edges in the graph at all — the seed's
//!   kind:3 was never fetched — so every recommender scores 0 and the
//!   fallback is wired into scoring but starved of data. This interest is
//!   keyed on the configured seed, not the active account, so it never
//!   reopens on identity change.
//!
//! All feed one [`MintDiscoveryStore`] behind a `Mutex`. The app queries the
//! current view via [`MintDiscoveryRuntime::snapshot`] — the same
//! runtime-holds-the-projection access other reusable NMP runtimes use.
//!
//! `register.rs` registers [`MintDiscoveryRuntime::snapshot`] as this crate's
//! OWN typed FlatBuffers snapshot projection (`"mint_discovery"`, see
//! `projection_wire.rs`) — unlike the pre-extraction `nmp-wallet` version,
//! which folded this view into the wallet's own merged sidecar. Any app that
//! composes this crate gets that projection for free; it does not need to be
//! a wallet.

use std::sync::{Arc, Mutex};

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::substrate::{
    IdentityChangeRegistrar, KernelEvent, ObservedProjectionReconciler,
    ObservedProjectionRegistrar,
};
use nmp_core::ObservedProjectionSink;

use crate::discovery::{DiscoveryPolicy, MintDiscoveryProjection, MintDiscoveryStore};
use crate::interests::{mint_discovery_shape, mint_discovery_trust_graph_shape};

/// `ObservedProjection::scope` for `InterestScope::Global` (any nonzero
/// value). Used for two shapes that must never re-route on account switch:
/// mint announcements/recommendations (author-unknown public content that
/// needs the planner's cold-start bootstrap-relay lane, exactly like
/// `nmp-nip57`'s zap-receipt read), and the `fallback_root` seed's own
/// follow/mute graph (a fixed pubkey unrelated to the active account).
const SCOPE_GLOBAL: u32 = 1;

/// `ObservedProjection::scope` for `InterestScope::ActiveAccount`. The trust
/// graph read is an `authors`=self filter with no cold-start bootstrap
/// dependency (mirrors a self-authored shape).
const SCOPE_ACTIVE_ACCOUNT: u32 = 0;

/// Bounded cache replay on (re)open.
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
    /// Construct the runtime and wire its identity-reactive observed
    /// projections onto `app`, using [`DiscoveryPolicy::default`] (no
    /// `fallback_root`, so only the two identity-reactive projections open).
    #[must_use]
    pub fn new(
        active_pubkey: ActiveAccountSlot,
        app: &(impl ObservedProjectionRegistrar + IdentityChangeRegistrar),
    ) -> Self {
        Self::with_policy(active_pubkey, app, DiscoveryPolicy::default())
    }

    /// Construct the runtime with a caller-supplied [`DiscoveryPolicy`] and
    /// wire its observed projections onto `app`: the two identity-reactive
    /// ones always, plus a third fixed one for `policy.fallback_root`'s own
    /// follow/mute graph when that policy field is `Some` (see module docs).
    #[must_use]
    pub fn with_policy(
        active_pubkey: ActiveAccountSlot,
        app: &(impl ObservedProjectionRegistrar + IdentityChangeRegistrar),
        policy: DiscoveryPolicy,
    ) -> Self {
        // Grabbed before `policy` is moved into the store: the fixed
        // cold-start trust seed, if configured, also needs its own graph
        // fetched (see the fallback reconciler below).
        let fallback_root = policy.fallback_root.clone();

        let store = Arc::new(Mutex::new(MintDiscoveryStore::with_policy(policy)));
        let sink: Arc<dyn ObservedProjectionSink> = Arc::new(MintDiscoverySink {
            store: Arc::clone(&store),
        });

        // Global announcements + recommendations: fixed shape, identity-independent.
        let discovery_reconciler = ObservedProjectionReconciler::new(
            app.observed_projection_registrar_handle(),
            Arc::clone(&sink),
            "nmp.mint_discovery.announcements",
            SCOPE_GLOBAL,
            REPLAY_LIMIT,
            Arc::new(|| Some(mint_discovery_shape())),
        );

        // Self follow/mute graph: re-derived from the active pubkey.
        let graph_pubkey = Arc::clone(&active_pubkey);
        let graph_reconciler = ObservedProjectionReconciler::new(
            app.observed_projection_registrar_handle(),
            Arc::clone(&sink),
            "nmp.mint_discovery.trust_graph",
            SCOPE_ACTIVE_ACCOUNT,
            REPLAY_LIMIT,
            Arc::new(move || {
                let pubkey = graph_pubkey.lock().ok()?.clone()?;
                Some(mint_discovery_trust_graph_shape(&pubkey))
            }),
        );

        // Fallback-root ("cold-start trust seed") follow/mute graph: only
        // opened when the policy configures one. `score_rooted` reroutes a
        // cold viewer's scoring through `fallback_root`'s perspective, but
        // that is only useful if the seed's own kind:3/kind:10000 are
        // actually in the WoT graph — otherwise every recommender scores 0
        // and the fallback is wired into scoring but starved of data (the
        // bug this fixes). Reuses the same self-scoped shape function, just
        // called with the seed's pubkey instead of the viewer's, and feeds
        // the SAME store sink so `MintDiscoveryStore::ingest_kernel_event`
        // handles the seed's kind:3/kind:10000 exactly like any other
        // follow/mute-list event.
        //
        // This shape is FIXED to the configured seed, not the active
        // account, so — unlike `graph_reconciler` above — it is never
        // re-synced from `register_identity_change_observer`: the seed does
        // not change when the signed-in account changes.
        //
        // Depth-1 only, symmetric with the self-graph read above: this
        // fetches the seed's own kind:3/kind:10000, not its followees'
        // graphs. Fetching those too (depth-2 enrichment) is a future
        // enhancement, not attempted here.
        if let Some(seed) = fallback_root {
            let fallback_reconciler = ObservedProjectionReconciler::new(
                app.observed_projection_registrar_handle(),
                sink,
                "nmp.mint_discovery.fallback_trust_graph",
                SCOPE_GLOBAL,
                REPLAY_LIMIT,
                Arc::new(move || Some(mint_discovery_trust_graph_shape(&seed))),
            );
            fallback_reconciler.sync();
        }

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
    /// WoT-scoped). Empty until an account is active (unless the configured
    /// policy's `fallback_root` still scores against a seed). Cheap on the
    /// steady-state emit path: [`MintDiscoveryStore::snapshot`] is memoized,
    /// so this serves a cached value unless an ingested event has dirtied it
    /// (the `mut` lock is only to update that memo, not to re-aggregate every
    /// call).
    #[must_use]
    pub fn snapshot(&self) -> MintDiscoveryProjection {
        self.store
            .lock()
            .map(|mut store| store.snapshot())
            .unwrap_or_default()
    }
}
