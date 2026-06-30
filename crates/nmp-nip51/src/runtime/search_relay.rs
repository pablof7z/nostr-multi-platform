//! NIP-51 search-relay-list runtime — wires the kind:10007
//! [`SearchRelayListProjection`] into a host as a declared observed
//! projection.
//!
//! # What callers get
//!
//! `register_search_relay_runtime_with_fallbacks` returns
//! `Arc<SearchRelayListProjection>`. Pass it to [`crate::effective_search_relays`] to get
//! the effective relay list (user's kind:10007 list, else the app-supplied
//! fallback, else empty). A higher-order NIP-50 search crate that needs to open
//! a relay subscription on the right relays calls that helper rather than
//! reaching into the projection directly.
//!
//! # How kind:10007 events reach the projection (#1817)
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
use nmp_nip50::{SearchFallbackRelays, SearchRelaySource};
use nmp_planner::InterestShape;

use crate::SearchRelayListProjection;

/// Wire the NIP-51 search-relay-list projection into `app` and return the
/// [`SearchRelayListProjection`] so callers can read the active account's
/// kind:10007 relay list.
///
/// # What this function does
///
/// 1. **Pubkey slot bridge** — hands [`SearchRelayListProjection`] the shared
///    `AppHost::active_pubkey()` hex slot (populated by the kernel for EVERY
///    backend including bunker). The projection reads it at event-ingest time
///    and at query time, so it is always consistent with the active account.
/// 2. **Active observed projection** — opens `SearchRelayListProjection` with a
///    concrete `authors=[active] / kinds=[10007]` shape, so cached rows hydrate
///    before live activation and future delivery is author-scoped.
/// 3. **Returns the `Arc<SearchRelayListProjection>`** — the caller passes it
///    to [`crate::effective_search_relays`] to resolve the
///    effective search relay set (user list, else app default, else empty).
///
/// # Account-switch safety
///
/// [`SearchRelayListProjection`] is self-contained: the read path re-reads the
/// live `active_pubkey` slot on every call and gates against the
/// `owner_pubkey` stored inside the `SearchRelaySet`. If the active account
/// changed between the last kind:10007 ingest and the read, methods return a
/// default empty list — stale data from the prior account is invisible. The
/// active observed-projection reconciler closes the prior author shape and
/// opens the new one on account change.
///
/// # D0 hygiene
///
/// This function names `kind:10007` only as a numeric literal inside
/// `nmp-nip51`. The term "search relays" enters `nmp-core` nowhere. The
/// composition crate (here) is entitled to name NIP constants directly per
/// ADR-0046.
///
/// Exposed as a named per-feature installer so app composition roots can wire
/// search-relay support without pulling in any defaults bundle. Fallback relay
/// policy is explicit at every call site; pass
/// [`SearchFallbackRelays::default()`] only when the app deliberately wants no
/// fallback relays.
pub fn register_search_relay_runtime_with_fallbacks(
    app: &(impl ObservedProjectionRegistrar
          + HostCapabilities
          + SnapshotProjectionRegistrar
          + IdentityChangeRegistrar),
    fallback_relays: SearchFallbackRelays,
) -> Arc<SearchRelayListProjection> {
    // ── 1. Active-pubkey slot ────────────────────────────────────────────────
    let projection = Arc::new(SearchRelayListProjection::new(app.active_pubkey()));

    // ── 2. Active observed projection ──────────────────────────────────────
    //
    // Identity-change-driven: no tick polling. The live_shape closure reads the
    // active pubkey slot directly, returning Some(shape) when signed in and
    // None on logout/reset so no stale subscription lingers.
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
    // `user_preferred()`, and the app-supplied fallback relays for the
    // fallback (possibly empty) — exactly the `effective_search_relays`
    // preference order, but exposed through the `nmp-nip50` read seam instead
    // of requiring the app to call `effective_search_relays` itself.
    //
    // `nmp_nip50::install_search_relay_source` installs the source through the
    // substrate-generic `HostCapabilities::install_preferred_relay_source` seam
    // (default no-op, overridden by `NmpApp`). So this works for EVERY `AppHost`
    // — a minimal / scaffolded host compiles for free (no-op), a real host wires
    // it for real. D0: `nmp-nip50` never names the host type.
    //
    // Depends ONLY on the `projection` handle (the kind:10007 read model the
    // active observed-projection reconciler populates).
    nmp_nip50::install_search_relay_source(
        app,
        Arc::new(DefaultSearchRelaySource {
            projection: Arc::clone(&projection),
            fallback_relays,
        }),
    );

    projection
}

/// The default search-relay source the composition root auto-wires onto the
/// host. `user_preferred()` returns the active account's live kind:10007 list;
/// `app_default()` returns the app-configured fallback relay list.
///
/// Mirrors [`crate::effective_search_relays`]'s preference order (user list →
/// default), but `nmp_nip50::resolve_search_relays` applies the fallback itself,
/// so this source exposes the two lists independently.
struct DefaultSearchRelaySource {
    projection: Arc<SearchRelayListProjection>,
    fallback_relays: SearchFallbackRelays,
}

impl SearchRelaySource for DefaultSearchRelaySource {
    fn user_preferred(&self) -> Vec<String> {
        self.projection.snapshot().relays
    }

    fn app_default(&self) -> Vec<String> {
        self.fallback_relays.relays.clone()
    }
}
