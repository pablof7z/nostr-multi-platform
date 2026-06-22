//! NIP-51 search-relay-list runtime — wires the kind:10007
//! [`SearchRelayListProjection`] into an [`AppHost`] as a kernel event observer.
//!
//! The `register_search_relay_runtime` entry point is re-exported from
//! `runtimes` and from `nmp_defaults` so callers reach it at
//! `runtimes::register_search_relay_runtime` /
//! `nmp_defaults::register_search_relay_runtime`.
//!
//! # What callers get
//!
//! `register_search_relay_runtime` returns `Arc<SearchRelayListProjection>`.
//! Pass it to [`crate::search_defaults::effective_search_relays`] to get
//! the effective relay list (user's kind:10007 list, else the app-default
//! fallback). A higher-order NIP-50 search crate that needs to open a relay
//! subscription on the right relays calls that helper rather than reaching
//! into the projection directly.
//!
//! # How kind:10007 events reach the projection (#1817)
//!
//! The active account's own kind:10007 search-relay list is fetched by the
//! kernel's cold-start **self-kinds tailing bundle** — the proven path that
//! also fetches kind:0 / kind:3 / kind:10002 / kind:10006. kind:10007 was added
//! to `SELF_KINDS_TAILING` in `nmp-core`'s `kernel/requests/startup.rs`, so the
//! self-fetch REQ is compiled and routed by the planner's Case A author lane and
//! survives account switches (the bootstrap slot's author is replaced in place
//! on switch). All this runtime has to do is register the projection as a
//! [`KernelEventObserver`] so the kernel fan-out delivers those events to it.
//!
//! The prior implementation pushed a bespoke `authors=[active] / kinds=[10007]`
//! interest from a per-tick `SearchRelayRuntimeController`. That interest never
//! reached the wire (the self-fetch was never compiled into a routed REQ), so
//! `effective_search_relays()` stayed empty and transparent NIP-50
//! `open_search(UserPreferred)` never fanned out. Routing kind:10007 through the
//! proven self-kinds bundle — exactly like kind:10006 (blocked relays), another
//! account-specific replaceable list — fixes it without a parallel mechanism.
//!
//! [`AppHost`]: nmp_core::substrate::AppHost

use std::sync::Arc;

use nmp_core::substrate::{EventObserverRegistrar, HostCapabilities};
use nmp_core::KernelEventObserver;
use nmp_nip51::SearchRelayListProjection;

/// Wire the NIP-51 search-relay-list observer into `app` and return the
/// [`SearchRelayListProjection`] so callers can read the active account's
/// kind:10007 relay list.
///
/// # What this function does
///
/// 1. **Pubkey slot bridge** — hands [`SearchRelayListProjection`] the shared
///    `AppHost::active_pubkey()` hex slot (populated by the kernel for EVERY
///    backend including bunker). The projection reads it at event-ingest time
///    and at query time, so it is always consistent with the active account.
/// 2. **Ingest observer** — registers `SearchRelayListProjection` as a
///    [`KernelEventObserver`] so the kernel fan-out delivers kind:10007 events
///    to the projection. The active account's kind:10007 self-fetch is carried
///    by the kernel's `SELF_KINDS_TAILING` bundle (see module doc), so no
///    bespoke interest push is needed. The projection filters for the active
///    account's author (account-switch safety enforced at read time by the
///    owner-pubkey gate inside the projection).
/// 3. **Returns the `Arc<SearchRelayListProjection>`** — the caller passes it
///    to [`crate::search_defaults::effective_search_relays`] to resolve the
///    effective search relay set (user list, else app default).
///
/// # Account-switch safety
///
/// [`SearchRelayListProjection`] is self-contained: the read path re-reads the
/// live `active_pubkey` slot on every call and gates against the
/// `owner_pubkey` stored inside the `SearchRelaySet`. If the active account
/// changed between the last kind:10007 ingest and the read, methods return a
/// default empty list — stale data from the prior account is invisible. The
/// kernel's self-kinds bundle re-targets its tailing REQ onto the new account
/// on switch (the bootstrap slot's author is replaced in place), so the new
/// account's kind:10007 flows in and the prior account's never leaks.
///
/// # D0 hygiene
///
/// This function names `kind:10007` only as a numeric literal inside
/// `nmp-nip51`. The term "search relays" enters `nmp-core` nowhere — the kernel
/// only tails a kind number in `SELF_KINDS_TAILING`, exactly as it does for
/// kind:10006. The composition crate (here) is entitled to name NIP constants
/// directly per ADR-0046.
///
/// Called by [`crate::register_defaults`]; exposed `pub` so an app crate that
/// opts out of the wholesale defaults can still wire just the search-relay
/// projection by itself.
pub fn register_search_relay_runtime(
    app: &(impl EventObserverRegistrar + HostCapabilities),
) -> Arc<SearchRelayListProjection> {
    // ── 1. Active-pubkey slot ────────────────────────────────────────────────
    let projection = Arc::new(SearchRelayListProjection::new(app.active_pubkey()));

    // ── 2. Register as ingest observer ──────────────────────────────────────
    //
    // The kernel's self-kinds tailing bundle (kind:0/3/10002/10006/10007, see
    // `nmp-core` `kernel/requests/startup.rs`) fetches the active account's
    // kind:10007 and fans it to every registered observer. Registering the
    // projection here is the only wiring this runtime needs.
    app.register_event_observer(Arc::clone(&projection) as Arc<dyn KernelEventObserver>);

    projection
}
