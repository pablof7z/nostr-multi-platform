//! NIP-51 search-relay-list runtime — wires the kind:10007
//! [`SearchRelayListProjection`] into an [`AppHost`] as a declared observed
//! projection.
//!
//! # What callers get
//!
//! [`register_search_relay_runtime`] returns `Arc<SearchRelayListProjection>`.
//! Pass it to [`crate::effective_search_relays`] to get the effective relay list
//! (user's kind:10007 list, else the app-default fallback, else empty). A
//! higher-order NIP-50 search crate that needs to open a relay subscription on
//! the right relays calls that helper rather than reaching into the projection
//! directly.
//!
//! # How kind:10007 events reach the projection
//!
//! An identity-change-driven active observed-projection reconciler opens one
//! concrete `authors=[active] / kinds=[10007]` observed projection after sign-in
//! and closes/reopens it on account switches. Opening through the observed
//! projection path replays matching cached rows before live activation, so
//! `effective_search_relays()` hydrates on cold start without a broad kind-only
//! observer.
//!
//! [`AppHost`]: nmp_core::substrate::AppHost

use std::sync::Arc;

use nmp_core::substrate::{
    HostCapabilities, IdentityChangeRegistrar, ObservedProjectionReconciler,
    ObservedProjectionRegistrar, SnapshotProjectionRegistrar,
};
use nmp_core::ObservedProjectionSink;
use nmp_nip50::SearchRelaySource;
use nmp_planner::InterestShape;

use crate::{search_defaults::SearchDefaults, SearchRelayListProjection};

/// Wire the NIP-51 search-relay-list projection into `app` and return the
/// [`SearchRelayListProjection`] so callers can read the active account's
/// kind:10007 relay list.
///
/// Equivalent to calling [`register_search_relay_runtime_with`] with
/// [`SearchDefaults::default()`] (no app-level fallback relay).
pub fn register_search_relay_runtime(
    app: &(impl ObservedProjectionRegistrar
          + HostCapabilities
          + SnapshotProjectionRegistrar
          + IdentityChangeRegistrar),
) -> Arc<SearchRelayListProjection> {
    register_search_relay_runtime_with(app, SearchDefaults::default())
}

/// [`register_search_relay_runtime`] with an explicit [`SearchDefaults`] for the
/// app-default fallback that backs the transparently-wired default search-relay
/// source. `register_search_relay_runtime` is the `SearchDefaults::default()`
/// convenience, which declares no app-default relay.
pub fn register_search_relay_runtime_with(
    app: &(impl ObservedProjectionRegistrar
          + HostCapabilities
          + SnapshotProjectionRegistrar
          + IdentityChangeRegistrar),
    defaults: SearchDefaults,
) -> Arc<SearchRelayListProjection> {
    // ── 1. Active-pubkey slot ────────────────────────────────────────────────
    let projection = Arc::new(SearchRelayListProjection::new(app.active_pubkey()));

    // ── 2. Active observed projection ──────────────────────────────────────
    let observer = Arc::clone(&projection) as Arc<dyn ObservedProjectionSink>;
    let active_pubkey = app.active_pubkey();
    let reconciler = ObservedProjectionReconciler::new(
        app.observed_projection_registrar_handle(),
        observer,
        "nmp.nip51.search_relays",
        1,
        64,
        Arc::new(move || {
            let pubkey = active_pubkey.lock().ok()?.clone()?;
            Some(InterestShape {
                kinds: [nmp_kinds::KIND_SEARCH_RELAYS].into_iter().collect(),
                authors: [pubkey].into_iter().collect(),
                ..Default::default()
            })
        }),
    );
    let reconciler_for_identity = reconciler.clone();
    app.register_identity_change_observer(move |_| reconciler_for_identity.sync());
    // Eager sync for cold-start: account may already be set.
    reconciler.sync();

    // ── 3. TRANSPARENCY GLUE — auto-wire the default search-relay source ──────
    //
    // So a plain app that calls only `open_search(.., UserPreferred)` fans out
    // to the user's published kind:10007 relays with ZERO app code. The source
    // reads the SAME live `SearchRelayListProjection` registered above for
    // `user_preferred()`, and the app-default `SearchDefaults` for the
    // fallback (possibly empty) — exactly the `effective_search_relays`
    // preference order, but exposed through the `nmp-nip50` read seam instead
    // of requiring the app to call `effective_search_relays` itself.
    nmp_nip50::install_search_relay_source(
        app,
        Arc::new(DefaultSearchRelaySource {
            projection: Arc::clone(&projection),
            defaults,
        }),
    );

    projection
}

/// The default search-relay source the composition root auto-wires onto the
/// host. `user_preferred()` returns the active account's live kind:10007 list;
/// `app_default()` returns the app-configured [`SearchDefaults`] relay list.
struct DefaultSearchRelaySource {
    projection: Arc<SearchRelayListProjection>,
    defaults: SearchDefaults,
}

impl SearchRelaySource for DefaultSearchRelaySource {
    fn user_preferred(&self) -> Vec<String> {
        self.projection.snapshot().relays
    }

    fn app_default(&self) -> Vec<String> {
        self.defaults.default_relays.clone()
    }
}
